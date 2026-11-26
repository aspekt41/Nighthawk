//! The tile grid, and the air and heat in it.

use serde::{Deserialize, Serialize};

use crate::air::{Mix, vent_ppm, vented_amount};
use crate::hash::Fnv64;
use crate::heat::{self, ROOM_TEMPERATURE_MK};
use crate::pos::{Dir, Pos, Vertical};

/// What a single tile is made of. This grows into material-backed terrain
/// later (decision D10). For now it's just enough to walk around a room.
///
/// `Deserialize` lets map files name tiles in JSON. `rename_all` means the
/// JSON spelling is lowercase: `"wall"`, `"floor"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tile {
    Floor,
    Wall,
    /// Stairs (or a ladder) to the deck above. Must stand directly under a
    /// `StairsDown` on that deck; see [`Map::check_stairs`].
    StairsUp,
    /// Stairs (or a ladder) to the deck below.
    StairsDown,
    /// A shut door. Blocks movement (and, from M1.2, air). Walking into it
    /// opens it.
    DoorClosed,
    /// An open door: walkable, and air passes through.
    DoorOpen,
    /// A small crack in the hull (about 2 × 2 cm): solid, but the space
    /// beside it slowly leaks to vacuum.
    HullCrack,
    /// A hole torn in the hull (about 20 × 20 cm): solid, but the space
    /// beside it vents to vacuum in seconds.
    HullBreach,
}

impl Tile {
    pub fn is_walkable(self) -> bool {
        match self {
            Tile::Floor | Tile::StairsUp | Tile::StairsDown | Tile::DoorOpen => true,
            Tile::Wall | Tile::DoorClosed | Tile::HullCrack | Tile::HullBreach => false,
        }
    }

    /// Whether air can be in this tile and move through it. Walls and
    /// closed doors are solid: air on either side can't get past them.
    pub fn holds_air(self) -> bool {
        self.is_walkable()
    }

    /// The area of the hole to space in this tile, in cm². Zero for
    /// everything except damaged hull.
    pub fn hole_cm2(self) -> u64 {
        match self {
            Tile::HullCrack => 4,
            Tile::HullBreach => 400,
            _ => 0,
        }
    }

    /// Whether the player can get through this tile, possibly by opening it
    /// first. Used to check every room can be reached.
    pub fn is_passable(self) -> bool {
        self.is_walkable() || self == Tile::DoorClosed
    }

    /// A fixed number per tile type, for [`Map::hash_into`]. These numbers
    /// are part of the determinism contract: never change or reuse one.
    fn hash_id(self) -> u8 {
        match self {
            Tile::Floor => 0,
            Tile::Wall => 1,
            Tile::StairsUp => 2,
            Tile::StairsDown => 3,
            Tile::DoorClosed => 4,
            Tile::DoorOpen => 5,
            Tile::HullCrack => 6,
            Tile::HullBreach => 7,
        }
    }

    /// A short name for messages, like "You bump into the wall."
    pub fn name(self) -> &'static str {
        match self {
            Tile::Floor => "floor",
            Tile::Wall => "bulkhead",
            Tile::StairsUp => "ladder up",
            Tile::StairsDown => "ladder down",
            Tile::DoorClosed => "closed door",
            Tile::DoorOpen => "open door",
            Tile::HullCrack => "cracked hull",
            Tile::HullBreach => "hull breach",
        }
    }
}

/// The tile that names `i`'s group in a union-find: follow parents until a
/// tile is its own parent. On the way back, point every tile passed straight
/// at that root ("path compression"), so later lookups are quick.
fn root(parent: &mut [usize], i: usize) -> usize {
    let mut r = i;
    while parent[r] != r {
        r = parent[r];
    }
    let mut j = i;
    while parent[j] != r {
        let next = parent[j];
        parent[j] = r;
        j = next;
    }
    r
}

/// Stairs at `at` lead `way`, but there's no matching stairs there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StairsError {
    pub at: Pos,
    pub way: Vertical,
}

impl std::fmt::Display for StairsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (here, there) = match self.way {
            Vertical::Up => ("stairs up", "stairs down"),
            Vertical::Down => ("stairs down", "stairs up"),
        };
        let other = self.at.climb(self.way);
        write!(
            f,
            "{here} at deck {}, row {}, column {} need {there} at the same spot on deck {}",
            self.at.z, self.at.y, self.at.x, other.z
        )
    }
}

impl std::error::Error for StairsError {}

/// A 3D grid of tiles: `width` × `height` on each of `depth` decks.
///
/// The tiles live in one flat `Vec`, deck after deck. That's the
/// "dense array" layout from the tech-stack research: later grid systems such
/// as air and heat will use the same shape, so their solvers are simple loops
/// over contiguous memory.
///
/// It's saved as-is in save files. Because a save file could be edited or
/// damaged, `check_consistent` verifies a loaded map before use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Map {
    width: i32,
    height: i32,
    depth: i32,
    tiles: Vec<Tile>,
    /// The air in each tile, in the same order as `tiles`. Tiles that can't
    /// hold air are vacuum, except a closed door, which keeps whatever air
    /// was in the doorway when it shut until it opens again.
    air: Vec<Mix>,
    /// How much of each gas has escaped to space through hull damage, in
    /// pascal-tiles, in [`crate::Gas::ALL`] order. Air on the ship plus air
    /// vented always equals the air the ship started with.
    vented: [u64; 3],
    /// The heat in each tile's air, in the same order as `tiles`: its
    /// pressure (Pa) times its temperature (mK). See [`crate::heat`].
    /// Tiles with no air have no heat.
    heat: Vec<u64>,
    /// Heat lost to space so far, through the hull or with vented air.
    /// Heat on the ship plus heat lost always equals the heat the ship
    /// started with.
    heat_lost: u64,
}

impl Map {
    /// A map where every tile is `fill`.
    pub fn filled(width: i32, height: i32, depth: i32, fill: Tile) -> Self {
        assert!(
            width > 0 && height > 0 && depth > 0,
            "map dimensions must be positive"
        );
        let len = (width * height * depth) as usize;
        Self {
            width,
            height,
            depth,
            tiles: vec![fill; len],
            air: vec![Mix::VACUUM; len],
            vented: [0; 3],
            heat: vec![0; len],
            heat_lost: 0,
        }
    }

    /// The air in the tile at `p`, or `None` if `p` is off the map.
    pub fn air(&self, p: Pos) -> Option<Mix> {
        self.index(p).map(|i| self.air[i])
    }

    /// Sets the air in the tile at `p`, at the temperature the tile's air
    /// already had (or room temperature if it was vacuum). Positions off
    /// the map are ignored.
    pub fn set_air(&mut self, p: Pos, mix: Mix) {
        let mk = self.temperature(p).unwrap_or(ROOM_TEMPERATURE_MK);
        if let Some(i) = self.index(p) {
            self.air[i] = mix;
            self.heat[i] = mix.total() * u64::from(mk);
        }
    }

    /// The temperature of the air at `p` in millikelvin (thousandths of a
    /// degree above absolute zero), or `None` if it's vacuum or off the map.
    pub fn temperature(&self, p: Pos) -> Option<u32> {
        let i = self.index(p)?;
        heat::temperature(self.heat[i], self.air[i].total())
    }

    /// Sets the temperature of the air at `p`, in millikelvin. Vacuum has
    /// no temperature, so this does nothing there, or off the map.
    pub fn set_temperature(&mut self, p: Pos, mk: u32) {
        if let Some(i) = self.index(p) {
            self.heat[i] = self.air[i].total() * u64::from(mk);
        }
    }

    /// Sets the temperature of all the air on deck `z`. Used when a map is
    /// first built, after its air is in.
    pub(crate) fn set_deck_temperature(&mut self, z: i32, mk: u32) {
        let on_deck: Vec<Pos> = self.positions().filter(|p| p.z == z).collect();
        for p in on_deck {
            self.set_temperature(p, mk);
        }
    }

    /// All the heat on the ship. Together with [`Map::heat_lost`] it always
    /// adds up to the heat the ship started with.
    pub fn total_heat(&self) -> u64 {
        self.heat.iter().sum()
    }

    /// Heat lost to space so far, through the hull or with vented air.
    pub fn heat_lost(&self) -> u64 {
        self.heat_lost
    }

    /// One second of heat moving (decision D34): neighbouring tiles of air
    /// even out a little, and tiles against the outer hull lose some to
    /// space. (Heat carried by moving air is handled by
    /// [`Map::spread_air`], as the air moves.)
    pub(crate) fn spread_heat(&mut self) {
        let pressure: Vec<u64> = self.air.iter().map(|mix| mix.total()).collect();
        // Work out the links first: `exchange` borrows `self.heat` mutably,
        // and Rust won't let `self` be borrowed again while that's going on.
        let links = self.air_links();
        heat::exchange(&mut self.heat, &pressure, &links);
        for (i, &pa) in pressure.iter().enumerate() {
            let faces = self.hull_faces(i);
            let lost = heat::hull_loss(self.heat[i], pa, faces);
            self.heat[i] -= lost;
            self.heat_lost += lost;
        }
    }

    /// How many of tile `i`'s four sides face the outer hull: the solid
    /// tiles around the map's border, which have deep space behind them.
    /// Only tiles with air lose heat, so other tiles count none.
    fn hull_faces(&self, i: usize) -> u64 {
        if !self.tiles[i].holds_air() {
            return 0;
        }
        let p = self.pos_of(i);
        let sides = [Dir::North, Dir::East, Dir::South, Dir::West].map(|d| p.step(d));
        let is_hull = |q: Pos| {
            let on_border = q.x == 0 || q.y == 0 || q.x == self.width - 1 || q.y == self.height - 1;
            on_border && self.get(q).is_some_and(|tile| !tile.holds_air())
        };
        sides.into_iter().filter(|&q| is_hull(q)).count() as u64
    }

    /// Fills every air-holding tile on deck `z` with `mix`. Used when a map
    /// is first built.
    pub(crate) fn fill_deck_air(&mut self, z: i32, mix: Mix) {
        let on_deck: Vec<Pos> = self.positions().filter(|p| p.z == z).collect();
        for p in on_deck {
            if self.get(p).is_some_and(Tile::holds_air) {
                self.set_air(p, mix);
            }
        }
    }

    /// The total of each gas over the whole map, in [`crate::Gas::ALL`]
    /// order. In a sealed ship this never changes.
    pub fn total_air(&self) -> [u64; 3] {
        self.air.iter().fold([0; 3], |sum, mix| {
            let [o2, n2, co2] = sum;
            [
                o2 + u64::from(mix.o2),
                n2 + u64::from(mix.n2),
                co2 + u64::from(mix.co2),
            ]
        })
    }

    /// How much of each gas has escaped to space so far, in [`crate::Gas::ALL`]
    /// order. Together with [`Map::total_air`] it always adds up to the air
    /// the ship started with.
    pub fn vented(&self) -> [u64; 3] {
        self.vented
    }

    /// One second of air movement (decisions D30 and D33).
    ///
    /// Tiles are 1 m and a step of the simulation is 1 s. Air pressure evens
    /// out at about the speed of sound, 340 m/s, so within one second every
    /// connected open space (tiles joined through open floor, open doors and
    /// ladders) reaches a single, even pressure and mix. A space beside hull
    /// damage also loses a share of its air to vacuum, sized by the holes'
    /// area against the space's volume (see [`vent_ppm`]).
    ///
    /// Returns the tiles of every space that lost air this second, so the
    /// game can tell the player when the air around them is escaping.
    pub(crate) fn spread_air(&mut self) -> Vec<usize> {
        let mut venting = Vec::new();
        for space in self.connected_spaces() {
            let hole = self.hole_area_around(&space);
            self.settle(&space, hole);
            if hole > 0 {
                venting.extend_from_slice(&space);
            }
        }
        venting
    }

    /// The total hole area (cm²) of the hull damage touching `space`: every
    /// cracked or breached tile beside one of its tiles, each counted once.
    fn hole_area_around(&self, space: &[usize]) -> u64 {
        let mut holes: Vec<usize> = space.iter().flat_map(|&i| self.damage_beside(i)).collect();
        holes.sort_unstable();
        holes.dedup();
        holes.iter().map(|&h| self.tiles[h].hole_cm2()).sum()
    }

    /// The hull-damage tiles on the four sides of tile `i` (same deck).
    fn damage_beside(&self, i: usize) -> Vec<usize> {
        let p = self.pos_of(i);
        let sides = [Dir::North, Dir::East, Dir::South, Dir::West].map(|d| p.step(d));
        sides
            .into_iter()
            .filter_map(|q| self.index(q))
            .filter(|&q| self.tiles[q].hole_cm2() > 0)
            .collect()
    }

    /// The position of index `i` in the flat tile and air vectors: the
    /// reverse of `index`.
    fn pos_of(&self, i: usize) -> Pos {
        let i = i32::try_from(i).expect("map index fits in i32");
        let per_deck = self.width * self.height;
        Pos::new(i % self.width, i % per_deck / self.width, i / per_deck)
    }

    /// Shares the air in `space` (indexes into `air`, in ascending order)
    /// evenly between its tiles, after letting out what escapes through
    /// `hole_cm2` of hull damage this second.
    ///
    /// Each gas's total is split into equal whole pascals; the few left
    /// over go one each to the first tiles in the list. So nothing is lost
    /// to rounding, tiles differ by at most 1 Pa per gas, and the same
    /// space always splits the same way.
    ///
    /// Heat moves with the air: see [`heat::carry`].
    fn settle(&mut self, space: &[usize], hole_cm2: u64) {
        let before: Vec<u64> = space.iter().map(|&i| self.air[i].total()).collect();
        let mut totals = space.iter().fold([0u64; 3], |sum, &i| {
            let amounts = self.air[i].amounts();
            [
                sum[0] + amounts[0],
                sum[1] + amounts[1],
                sum[2] + amounts[2],
            ]
        });
        let count = space.len() as u64;
        let ppm = vent_ppm(hole_cm2, count);
        for (gas, total) in totals.iter_mut().enumerate() {
            let lost = vented_amount(*total, ppm);
            *total -= lost;
            self.vented[gas] += lost;
        }
        for (k, &i) in space.iter().enumerate() {
            let share = totals.map(|total| total / count + u64::from((k as u64) < total % count));
            self.air[i] = Mix::from_amounts(share);
        }
        let after: Vec<u64> = space.iter().map(|&i| self.air[i].total()).collect();
        self.heat_lost += heat::carry(&mut self.heat, space, &before, &after);
    }

    /// Every connected open space, as lists of indexes into `air`. Each list
    /// is in ascending order and the lists are ordered by their first
    /// tile, so the result never depends on how it was worked out.
    ///
    /// It uses *union-find*: every tile starts as its own group, and each
    /// link between two tiles merges their groups. `root` follows a tile's
    /// chain of parents up to the tile that names its group.
    fn connected_spaces(&self) -> Vec<Vec<usize>> {
        let mut parent: Vec<usize> = (0..self.air.len()).collect();
        for (a, b) in self.air_links() {
            let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
            // Point the larger root at the smaller one: always the same
            // choice, whatever order the links come in.
            parent[ra.max(rb)] = ra.min(rb);
        }
        let mut group_of_root: Vec<Option<usize>> = vec![None; self.air.len()];
        let mut spaces: Vec<Vec<usize>> = Vec::new();
        for i in (0..self.air.len()).filter(|&i| self.tiles[i].holds_air()) {
            let r = root(&mut parent, i);
            let group = *group_of_root[r].get_or_insert_with(|| {
                spaces.push(Vec::new());
                spaces.len() - 1
            });
            spaces[group].push(i);
        }
        spaces
    }

    /// Every pair of tiles air can move between, as indexes into `air`.
    /// Each pair is listed once: a tile links to its east and south
    /// neighbours, and to the deck above if it's a ladder up.
    fn air_links(&self) -> Vec<(usize, usize)> {
        self.positions().flat_map(|p| self.links_from(p)).collect()
    }

    fn links_from(&self, p: Pos) -> Vec<(usize, usize)> {
        let Some(from) = self.air_index(p) else {
            return Vec::new();
        };
        let mut targets = vec![Pos::new(p.x + 1, p.y, p.z), Pos::new(p.x, p.y + 1, p.z)];
        if self.get(p) == Some(Tile::StairsUp) {
            targets.push(p.climb(Vertical::Up));
        }
        targets
            .into_iter()
            .filter_map(|t| Some((from, self.air_index(t)?)))
            .collect()
    }

    /// The index of `p` in `air`, if `p` is on the map and can hold air.
    fn air_index(&self, p: Pos) -> Option<usize> {
        self.get(p).filter(|tile| tile.holds_air())?;
        self.index(p)
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn depth(&self) -> i32 {
        self.depth
    }

    pub fn in_bounds(&self, p: Pos) -> bool {
        (0..self.width).contains(&p.x)
            && (0..self.height).contains(&p.y)
            && (0..self.depth).contains(&p.z)
    }

    /// The tile at `p`, or `None` if `p` is off the map.
    pub fn get(&self, p: Pos) -> Option<Tile> {
        self.index(p).map(|i| self.tiles[i])
    }

    /// Sets the tile at `p`. Positions off the map are ignored.
    pub fn set(&mut self, p: Pos, tile: Tile) {
        if let Some(i) = self.index(p) {
            self.tiles[i] = tile;
        }
    }

    /// Checks that the size is positive and the tile list holds exactly one
    /// tile per position. A map built in code is always consistent. This
    /// exists for maps read back from save files.
    pub(crate) fn check_consistent(&self) -> Result<(), String> {
        let (w, h, d) = (self.width, self.height, self.depth);
        if w <= 0 || h <= 0 || d <= 0 {
            return Err(format!("map size {w}×{h}×{d} must be positive"));
        }
        let expected = i64::from(w) * i64::from(h) * i64::from(d);
        let found = self.tiles.len() as i64;
        if found != expected {
            return Err(format!("map should have {expected} tiles but has {found}"));
        }
        if self.air.len() != self.tiles.len() {
            return Err(format!(
                "map has {found} tiles but air for {}",
                self.air.len()
            ));
        }
        if self.heat.len() != self.tiles.len() {
            return Err(format!(
                "map has {found} tiles but heat for {}",
                self.heat.len()
            ));
        }
        self.check_heat()?;
        self.check_stairs().map_err(|e| e.to_string())
    }

    /// Checks no tile has heat without air to hold it.
    fn check_heat(&self) -> Result<(), String> {
        let warm_vacuum =
            (0..self.heat.len()).find(|&i| self.heat[i] > 0 && self.air[i].total() == 0);
        match warm_vacuum {
            Some(i) => Err(format!("tile at {:?} has heat but no air", self.pos_of(i))),
            None => Ok(()),
        }
    }

    /// Every position on the map, deck by deck, row by row.
    pub fn positions(&self) -> impl Iterator<Item = Pos> + '_ {
        (0..self.depth).flat_map(move |z| {
            (0..self.height).flat_map(move |y| (0..self.width).map(move |x| Pos::new(x, y, z)))
        })
    }

    /// Checks that stairs come in pairs: every `StairsUp` has a `StairsDown`
    /// at the same spot on the deck above, and every `StairsDown` has a
    /// `StairsUp` below it. That's what lets climbing simply move you one
    /// deck up or down, knowing there's a way back.
    pub fn check_stairs(&self) -> Result<(), StairsError> {
        for at in self.positions() {
            let needed = match self.get(at) {
                Some(Tile::StairsUp) => (Vertical::Up, Tile::StairsDown),
                Some(Tile::StairsDown) => (Vertical::Down, Tile::StairsUp),
                _ => continue,
            };
            let (way, partner) = needed;
            if self.get(at.climb(way)) != Some(partner) {
                return Err(StairsError { at, way });
            }
        }
        Ok(())
    }

    /// Feeds the map's size and every tile into a state hash.
    pub(crate) fn hash_into(&self, h: &mut Fnv64) {
        h.i32(self.width).i32(self.height).i32(self.depth);
        for tile in &self.tiles {
            h.u8(tile.hash_id());
        }
        for lost in self.vented {
            h.u64(lost);
        }
        h.u64(self.heat_lost);
        for &heat in &self.heat {
            h.u64(heat);
        }
        for mix in &self.air {
            h.u64(u64::from(mix.o2))
                .u64(u64::from(mix.n2))
                .u64(u64::from(mix.co2));
        }
    }

    /// Where `p` lives in the flat tile and air vectors, for matching it
    /// against the tiles [`Map::spread_air`] reports.
    pub(crate) fn index_of(&self, p: Pos) -> Option<usize> {
        self.index(p)
    }

    /// Where `p` lives in the flat `tiles` vector.
    fn index(&self, p: Pos) -> Option<usize> {
        if !self.in_bounds(p) {
            return None;
        }
        let i = p.x + p.y * self.width + p.z * self.width * self.height;
        Some(i as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_floor_is_walkable() {
        assert!(Tile::Floor.is_walkable());
        assert!(!Tile::Wall.is_walkable());
    }

    #[test]
    fn tiles_have_names_for_messages() {
        assert_eq!(Tile::Floor.name(), "floor");
        assert_eq!(Tile::Wall.name(), "bulkhead");
    }

    #[test]
    fn get_and_set_respect_all_three_axes() {
        let mut map = Map::filled(4, 3, 2, Tile::Floor);
        map.set(Pos::new(1, 2, 1), Tile::Wall);
        assert_eq!(map.get(Pos::new(1, 2, 1)), Some(Tile::Wall));
        // Same x and y on the other deck is untouched.
        assert_eq!(map.get(Pos::new(1, 2, 0)), Some(Tile::Floor));
    }

    #[test]
    fn off_map_positions_are_none() {
        let map = Map::filled(4, 3, 1, Tile::Floor);
        assert_eq!(map.get(Pos::new(-1, 0, 0)), None);
        assert_eq!(map.get(Pos::new(4, 0, 0)), None);
        assert_eq!(map.get(Pos::new(0, 0, 1)), None);
    }

    /// A 2×1 map with two decks: stairs up at (0, 0, 0), stairs down above.
    fn stairs_map() -> Map {
        let mut map = Map::filled(2, 1, 2, Tile::Floor);
        map.set(Pos::new(0, 0, 0), Tile::StairsUp);
        map.set(Pos::new(0, 0, 1), Tile::StairsDown);
        map
    }

    #[test]
    fn stairs_are_walkable_and_named() {
        assert!(Tile::StairsUp.is_walkable() && Tile::StairsDown.is_walkable());
        assert_eq!(Tile::StairsUp.name(), "ladder up");
        assert_eq!(Tile::StairsDown.name(), "ladder down");
    }

    #[test]
    fn every_tile_has_its_own_hash_id() {
        let ids = [Tile::Floor, Tile::Wall, Tile::StairsUp, Tile::StairsDown].map(Tile::hash_id);
        assert_eq!(
            ids,
            [0, 1, 2, 3],
            "hash ids are part of the determinism contract"
        );
    }

    #[test]
    fn positions_cover_every_tile_once_in_reading_order() {
        let map = Map::filled(3, 2, 2, Tile::Floor);
        let all: Vec<Pos> = map.positions().collect();
        assert_eq!(all.len(), 12);
        assert_eq!(all[0], Pos::new(0, 0, 0));
        assert_eq!(all[1], Pos::new(1, 0, 0), "x changes fastest");
        assert_eq!(all[3], Pos::new(0, 1, 0));
        assert_eq!(all[6], Pos::new(0, 0, 1), "then the next deck");
    }

    #[test]
    fn paired_stairs_pass_the_check() {
        assert_eq!(stairs_map().check_stairs(), Ok(()));
        assert_eq!(Map::filled(2, 2, 1, Tile::Floor).check_stairs(), Ok(()));
    }

    #[test]
    fn stairs_up_without_stairs_down_above_fail() {
        let mut map = stairs_map();
        map.set(Pos::new(0, 0, 1), Tile::Floor);
        let err = map.check_stairs().unwrap_err();
        assert_eq!(
            err,
            StairsError {
                at: Pos::new(0, 0, 0),
                way: Vertical::Up
            }
        );
        assert_eq!(
            err.to_string(),
            "stairs up at deck 0, row 0, column 0 need stairs down at the same spot on deck 1"
        );
    }

    #[test]
    fn stairs_down_without_stairs_up_below_fail() {
        let mut map = stairs_map();
        map.set(Pos::new(0, 0, 0), Tile::Floor);
        let err = map.check_stairs().unwrap_err();
        assert_eq!(
            err,
            StairsError {
                at: Pos::new(0, 0, 1),
                way: Vertical::Down
            }
        );
        assert!(err.to_string().starts_with("stairs down at deck 1"));
    }

    #[test]
    fn stairs_off_the_top_or_bottom_of_the_ship_fail() {
        let mut top = Map::filled(1, 1, 1, Tile::StairsUp);
        assert!(top.check_stairs().is_err());
        top.set(Pos::new(0, 0, 0), Tile::StairsDown);
        assert!(top.check_stairs().is_err());
    }

    #[test]
    fn a_map_with_broken_stairs_is_inconsistent() {
        let mut map = stairs_map();
        map.set(Pos::new(0, 0, 1), Tile::Wall);
        assert!(
            map.check_consistent()
                .unwrap_err()
                .contains("need stairs down")
        );
    }

    #[test]
    fn doors_block_when_closed_and_let_you_through_when_open() {
        assert!(!Tile::DoorClosed.is_walkable());
        assert!(Tile::DoorOpen.is_walkable());
        assert!(Tile::DoorClosed.is_passable(), "you can open it");
        assert!(!Tile::Wall.is_passable());
        assert!(Tile::Floor.is_passable());
        assert_eq!(Tile::DoorClosed.name(), "closed door");
        assert_eq!(Tile::DoorOpen.name(), "open door");
    }

    // --- Air ------------------------------------------------------------------

    /// A one-deck corridor `w` tiles long, all floor, full of standard air.
    fn corridor(w: i32) -> Map {
        let mut map = Map::filled(w, 1, 1, Tile::Floor);
        map.fill_deck_air(0, Mix::STANDARD);
        map
    }

    fn total_pressure(map: &Map) -> u64 {
        map.total_air().iter().sum()
    }

    /// The largest difference in any one gas between any two tiles at `ps`.
    fn spread_between(map: &Map, ps: &[Pos]) -> u64 {
        let mixes: Vec<[u64; 3]> = ps.iter().map(|&p| map.air(p).unwrap().amounts()).collect();
        (0..3)
            .map(|g| {
                let values = mixes.iter().map(|m| m[g]);
                values.clone().max().unwrap() - values.min().unwrap()
            })
            .max()
            .unwrap()
    }

    #[test]
    fn only_open_tiles_hold_air() {
        for tile in [
            Tile::Floor,
            Tile::StairsUp,
            Tile::StairsDown,
            Tile::DoorOpen,
        ] {
            assert!(tile.holds_air(), "{tile:?}");
        }
        assert!(!Tile::Wall.holds_air());
        assert!(!Tile::DoorClosed.holds_air());
    }

    #[test]
    fn filling_a_deck_skips_walls_and_other_decks() {
        let mut map = Map::filled(2, 1, 2, Tile::Floor);
        map.set(Pos::new(1, 0, 0), Tile::Wall);
        map.fill_deck_air(0, Mix::STANDARD);
        assert_eq!(map.air(Pos::new(0, 0, 0)), Some(Mix::STANDARD));
        assert_eq!(
            map.air(Pos::new(1, 0, 0)),
            Some(Mix::VACUUM),
            "a wall holds no air"
        );
        assert_eq!(
            map.air(Pos::new(0, 0, 1)),
            Some(Mix::VACUUM),
            "the other deck"
        );
        assert_eq!(map.air(Pos::new(9, 9, 9)), None);
    }

    #[test]
    fn a_connected_space_evens_out_in_one_second() {
        // All the air at one end of a 30 m corridor: one second later it's
        // spread evenly along it, as real air would be.
        let mut map = corridor(30);
        for x in 1..30 {
            map.set_air(Pos::new(x, 0, 0), Mix::VACUUM);
        }
        map.spread_air();
        let all: Vec<Pos> = (0..30).map(|x| Pos::new(x, 0, 0)).collect();
        assert!(
            spread_between(&map, &all) <= 1,
            "tiles differ by at most 1 Pa per gas"
        );
        assert_eq!(total_pressure(&map), Mix::STANDARD.total(), "nothing lost");
    }

    #[test]
    fn evening_out_splits_exactly_with_leftovers_going_first() {
        // 10 Pa of O2 over 3 tiles: 4, 3, 3 (the leftover pascal goes to the
        // first tile), never 3.33 and never a lost pascal.
        let mut map = Map::filled(3, 1, 1, Tile::Floor);
        map.set_air(
            Pos::new(2, 0, 0),
            Mix {
                o2: 10,
                n2: 0,
                co2: 0,
            },
        );
        map.spread_air();
        let o2: Vec<u32> = (0..3)
            .map(|x| map.air(Pos::new(x, 0, 0)).unwrap().o2)
            .collect();
        assert_eq!(o2, vec![4, 3, 3]);
    }

    #[test]
    fn an_even_space_stays_exactly_as_it_is() {
        let mut map = Map::filled(3, 1, 1, Tile::Floor);
        map.set_air(
            Pos::new(2, 0, 0),
            Mix {
                o2: 10,
                n2: 0,
                co2: 0,
            },
        );
        map.spread_air();
        let settled = map.clone();
        for _ in 0..100 {
            map.spread_air();
        }
        assert_eq!(map, settled, "no creeping or oscillating");
    }

    #[test]
    fn a_sealed_space_never_gains_or_loses_a_single_pascal() {
        let mut map = corridor(12);
        for x in 1..12 {
            map.set_air(Pos::new(x, 0, 0), Mix::VACUUM);
        }
        map.set_air(
            Pos::new(0, 0, 0),
            Mix {
                o2: 500_000,
                n2: 1_000_000,
                co2: 7_777,
            },
        );
        let start = map.total_air();
        for _ in 0..1_000 {
            map.spread_air();
            assert_eq!(map.total_air(), start);
        }
    }

    #[test]
    fn walls_and_closed_doors_keep_spaces_apart() {
        for barrier in [Tile::Wall, Tile::DoorClosed] {
            let mut map = corridor(3);
            map.set(Pos::new(1, 0, 0), barrier);
            map.set_air(Pos::new(1, 0, 0), Mix::VACUUM);
            map.set_air(Pos::new(2, 0, 0), Mix::VACUUM);
            for _ in 0..100 {
                map.spread_air();
            }
            assert_eq!(
                map.air(Pos::new(0, 0, 0)),
                Some(Mix::STANDARD),
                "{barrier:?}"
            );
            assert_eq!(map.air(Pos::new(2, 0, 0)), Some(Mix::VACUUM), "{barrier:?}");
        }
    }

    #[test]
    fn separate_spaces_even_out_on_their_own() {
        // Two rooms either side of a wall, each lopsided inside.
        let mut map = Map::filled(5, 1, 1, Tile::Floor);
        map.set(Pos::new(2, 0, 0), Tile::Wall);
        map.set_air(
            Pos::new(0, 0, 0),
            Mix {
                o2: 100,
                n2: 0,
                co2: 0,
            },
        );
        map.set_air(
            Pos::new(4, 0, 0),
            Mix {
                o2: 0,
                n2: 60,
                co2: 0,
            },
        );
        map.spread_air();
        assert_eq!(
            map.air(Pos::new(1, 0, 0)),
            Some(Mix {
                o2: 50,
                n2: 0,
                co2: 0
            })
        );
        assert_eq!(
            map.air(Pos::new(3, 0, 0)),
            Some(Mix {
                o2: 0,
                n2: 30,
                co2: 0
            })
        );
    }

    #[test]
    fn a_closed_door_keeps_its_air_and_releases_it_when_opened() {
        let mut map = corridor(3);
        map.set(Pos::new(1, 0, 0), Tile::DoorClosed); // the doorway still has air
        map.set_air(Pos::new(2, 0, 0), Mix::VACUUM);
        for _ in 0..10 {
            map.spread_air();
        }
        assert_eq!(
            map.air(Pos::new(1, 0, 0)),
            Some(Mix::STANDARD),
            "trapped, not lost"
        );
        map.set(Pos::new(1, 0, 0), Tile::DoorOpen);
        map.spread_air();
        let all: Vec<Pos> = (0..3).map(|x| Pos::new(x, 0, 0)).collect();
        assert!(spread_between(&map, &all) <= 1, "one even space once open");
    }

    #[test]
    fn air_moves_between_decks_through_ladders() {
        let mut map = stairs_map(); // ladder at (0, 0): up on deck 0, down on deck 1
        map.fill_deck_air(0, Mix::STANDARD);
        let before = total_pressure(&map);
        map.spread_air();
        let all: Vec<Pos> = map.positions().collect();
        assert!(
            spread_between(&map, &all) <= 1,
            "both decks share one space"
        );
        assert!(
            map.air(Pos::new(1, 0, 1)).unwrap().total() > 0,
            "air reached upstairs"
        );
        assert_eq!(total_pressure(&map), before);
    }

    #[test]
    fn no_air_moves_between_decks_without_a_ladder() {
        let mut map = Map::filled(1, 1, 2, Tile::Floor);
        map.fill_deck_air(0, Mix::STANDARD);
        for _ in 0..10 {
            map.spread_air();
        }
        assert_eq!(map.air(Pos::new(0, 0, 1)), Some(Mix::VACUUM));
    }

    #[test]
    fn evening_out_does_not_favour_either_end() {
        // Mirror images end as mirror images, give or take the 1 Pa of
        // leftover each gas can have.
        let mut left = corridor(5);
        let mut right = corridor(5);
        for x in 1..5 {
            left.set_air(Pos::new(x, 0, 0), Mix::VACUUM);
            right.set_air(Pos::new(4 - x, 0, 0), Mix::VACUUM);
        }
        left.spread_air();
        right.spread_air();
        for x in 0..5 {
            let (l, r) = (
                left.air(Pos::new(x, 0, 0)).unwrap(),
                right.air(Pos::new(4 - x, 0, 0)).unwrap(),
            );
            assert!(
                l.total().abs_diff(r.total()) <= 3,
                "x = {x}: {l:?} vs {r:?}"
            );
        }
    }

    #[test]
    fn union_find_roots_point_every_tile_at_its_group() {
        // 0 -> 1 -> 2 -> 2: the root of 0 is 2, and afterwards 0 points
        // straight at it.
        let mut parent = vec![1, 2, 2];
        assert_eq!(root(&mut parent, 0), 2);
        assert_eq!(parent, vec![2, 2, 2]);
    }

    #[test]
    fn mismatched_air_data_is_inconsistent() {
        let mut map = corridor(3);
        assert_eq!(map.check_consistent(), Ok(()));
        map.air.pop();
        assert_eq!(
            map.check_consistent(),
            Err("map has 3 tiles but air for 2".to_string())
        );
    }

    #[test]
    fn air_is_part_of_the_state_hash() {
        let a = corridor(3);
        let mut b = corridor(3);
        b.set_air(Pos::new(1, 0, 0), Mix::STANDARD.scaled(99));
        let hash = |m: &Map| {
            let mut h = Fnv64::new();
            m.hash_into(&mut h);
            h.finish()
        };
        assert_ne!(hash(&a), hash(&b));
    }

    // --- Hull damage and venting -------------------------------------------------

    /// A 3-tile room `#...#` in a wall, full of normal air, with `damage`
    /// in the west wall: `D...#`.
    fn holed_room(damage: Tile) -> Map {
        let mut map = Map::filled(5, 3, 1, Tile::Wall);
        for x in 1..4 {
            map.set(Pos::new(x, 1, 0), Tile::Floor);
        }
        map.fill_deck_air(0, Mix::STANDARD);
        map.set(Pos::new(0, 1, 0), damage);
        map
    }

    fn room_pressure(map: &Map) -> u64 {
        map.air(Pos::new(2, 1, 0)).unwrap().total()
    }

    #[test]
    fn hull_damage_is_solid_airless_and_has_a_hole() {
        for tile in [Tile::HullCrack, Tile::HullBreach] {
            assert!(
                !tile.is_walkable() && !tile.holds_air() && !tile.is_passable(),
                "{tile:?}"
            );
        }
        assert_eq!(Tile::HullCrack.hole_cm2(), 4);
        assert_eq!(Tile::HullBreach.hole_cm2(), 400);
        assert_eq!(Tile::Wall.hole_cm2(), 0);
        assert_eq!(Tile::HullCrack.name(), "cracked hull");
        assert_eq!(Tile::HullBreach.name(), "hull breach");
        assert_eq!(
            [Tile::HullCrack, Tile::HullBreach].map(Tile::hash_id),
            [6, 7]
        );
    }

    #[test]
    fn a_breach_vents_a_small_room_to_vacuum_in_seconds() {
        let mut map = holed_room(Tile::HullBreach);
        for _ in 0..10 {
            map.spread_air();
        }
        assert!(
            room_pressure(&map) < Mix::STANDARD.total() / 100,
            "{}",
            room_pressure(&map)
        );
    }

    #[test]
    fn a_crack_leaks_slowly() {
        let mut map = holed_room(Tile::HullCrack);
        for _ in 0..10 {
            map.spread_air();
        }
        assert!(room_pressure(&map) > Mix::STANDARD.total() * 9 / 10);
        assert!(room_pressure(&map) < Mix::STANDARD.total());
    }

    #[test]
    fn air_on_board_plus_air_vented_never_changes() {
        let mut map = holed_room(Tile::HullBreach);
        let start = map.total_air();
        for _ in 0..100 {
            map.spread_air();
            let (on_board, vented) = (map.total_air(), map.vented());
            let accounted = [0, 1, 2].map(|g| on_board[g] + vented[g]);
            assert_eq!(accounted, start);
        }
        assert_eq!(map.total_air(), [0, 0, 0], "fully vented in the end");
    }

    #[test]
    fn spread_air_reports_the_spaces_that_lost_air() {
        let mut breached = holed_room(Tile::HullBreach);
        let room: Vec<usize> = (1..4)
            .map(|x| breached.index_of(Pos::new(x, 1, 0)).unwrap())
            .collect();
        assert_eq!(breached.spread_air(), room);
        assert!(
            holed_room(Tile::Wall).spread_air().is_empty(),
            "sealed rooms report nothing"
        );
    }

    #[test]
    fn a_closed_door_protects_the_room_behind_it() {
        // `X.+.#`: breach, room, closed door, room.
        let mut map = holed_room(Tile::HullBreach);
        map.set(Pos::new(2, 1, 0), Tile::DoorClosed);
        let behind = Pos::new(3, 1, 0);
        for _ in 0..60 {
            map.spread_air();
        }
        assert_eq!(map.air(behind), Some(Mix::STANDARD), "sealed off");
        map.set(Pos::new(2, 1, 0), Tile::DoorOpen);
        for _ in 0..60 {
            map.spread_air();
        }
        assert!(
            map.air(behind).unwrap().total() < Mix::STANDARD.total() / 100,
            "vented once open"
        );
    }

    #[test]
    fn a_hole_touching_a_space_twice_counts_once() {
        // A breach in a corner, beside two tiles of the same L-shaped room.
        let mut map = Map::filled(3, 3, 1, Tile::Floor);
        map.set(Pos::new(1, 1, 0), Tile::HullBreach);
        let space: Vec<usize> = map.positions().filter_map(|p| map.air_index(p)).collect();
        assert_eq!(map.hole_area_around(&space), 400);
    }

    #[test]
    fn positions_and_indexes_convert_both_ways() {
        let map = Map::filled(3, 2, 2, Tile::Floor);
        for p in map.positions() {
            assert_eq!(map.pos_of(map.index_of(p).unwrap()), p);
        }
    }

    #[test]
    fn vented_air_is_part_of_the_state_hash() {
        let hash = |m: &Map| {
            let mut h = Fnv64::new();
            m.hash_into(&mut h);
            h.finish()
        };
        let a = holed_room(Tile::HullBreach);
        let mut b = a.clone();
        b.vented = [1, 0, 0];
        assert_ne!(hash(&a), hash(&b));
    }
}

#[cfg(test)]
mod heat_tests {
    use super::*;
    use crate::heat::celsius_to_mk;

    /// A 5×5 deck: hull all round a 3×3 room of normal air at 20 °C.
    fn room() -> Map {
        let mut map = Map::filled(5, 5, 1, Tile::Wall);
        for y in 1..4 {
            for x in 1..4 {
                map.set(Pos::new(x, y, 0), Tile::Floor);
            }
        }
        map.fill_deck_air(0, Mix::STANDARD);
        map
    }

    fn total_accounted(map: &Map) -> u64 {
        map.total_heat() + map.heat_lost()
    }

    #[test]
    fn new_air_comes_in_at_room_temperature_and_keeps_its_temperature() {
        let mut map = room();
        let p = Pos::new(2, 2, 0);
        assert_eq!(map.temperature(p), Some(ROOM_TEMPERATURE_MK));
        map.set_temperature(p, celsius_to_mk(-5));
        map.set_air(p, Mix::STANDARD.scaled(50));
        assert_eq!(
            map.temperature(p),
            Some(celsius_to_mk(-5)),
            "thinner, same temperature"
        );
    }

    #[test]
    fn vacuum_and_off_map_tiles_have_no_temperature() {
        let mut map = room();
        let wall = Pos::new(0, 0, 0);
        map.set_temperature(wall, celsius_to_mk(50));
        assert_eq!(map.temperature(wall), None, "a wall holds no air to warm");
        assert_eq!(map.temperature(Pos::new(9, 9, 9)), None);
        map.set_temperature(Pos::new(9, 9, 9), 1); // ignored, no panic
    }

    #[test]
    fn only_tiles_beside_the_outer_hull_face_it() {
        let map = room();
        let faces = |x, y| map.hull_faces(map.index_of(Pos::new(x, y, 0)).unwrap());
        assert_eq!(faces(1, 1), 2, "a corner touches two hull tiles");
        assert_eq!(faces(2, 1), 1);
        assert_eq!(faces(2, 2), 0, "the middle touches none");
        assert_eq!(faces(0, 0), 0, "the hull itself holds no air");
    }

    #[test]
    fn inner_bulkheads_are_not_hull() {
        // A wall inside the map has ship on both sides, not space.
        let mut map = Map::filled(7, 3, 1, Tile::Wall);
        for x in 1..6 {
            map.set(Pos::new(x, 1, 0), Tile::Floor);
        }
        map.set(Pos::new(3, 1, 0), Tile::Wall);
        // (2, 1) has hull above and below, and the inner wall to its east.
        assert_eq!(map.hull_faces(map.index_of(Pos::new(2, 1, 0)).unwrap()), 2);
    }

    #[test]
    fn the_hull_slowly_cools_a_room_and_the_heat_is_accounted_for() {
        let mut map = room();
        let start = map.total_heat();
        for _ in 0..600 {
            map.spread_heat();
            assert_eq!(total_accounted(&map), start);
        }
        let middle = map.temperature(Pos::new(2, 2, 0)).unwrap();
        assert!(middle < ROOM_TEMPERATURE_MK, "cooler after ten minutes");
        assert!(middle > celsius_to_mk(15), "but only a little: {middle} mK");
        assert!(map.heat_lost() > 0);
    }

    #[test]
    fn a_cold_spot_spreads_out_and_warms_up() {
        let mut map = room();
        let middle = Pos::new(2, 2, 0);
        map.set_temperature(middle, celsius_to_mk(-50));
        map.spread_heat();
        let next_door = map.temperature(Pos::new(2, 1, 0)).unwrap();
        assert!(
            next_door < ROOM_TEMPERATURE_MK,
            "the cold reached next door"
        );
        assert!(map.temperature(middle).unwrap() > celsius_to_mk(-50));
    }

    #[test]
    fn venting_air_takes_its_heat_to_space() {
        let mut map = room();
        map.set(Pos::new(2, 0, 0), Tile::HullBreach);
        let start = map.total_heat();
        for _ in 0..120 {
            map.spread_air();
            assert_eq!(total_accounted(&map), start);
        }
        assert_eq!(map.total_heat(), 0, "vacuum holds no heat");
        assert_eq!(map.temperature(Pos::new(2, 2, 0)), None);
    }

    #[test]
    fn mismatched_heat_data_is_inconsistent() {
        let mut map = room();
        map.heat.pop();
        assert_eq!(
            map.check_consistent(),
            Err("map has 25 tiles but heat for 24".to_string())
        );
    }

    #[test]
    fn heat_without_air_is_inconsistent() {
        let mut map = room();
        let wall = map.index_of(Pos::new(4, 4, 0)).unwrap();
        map.heat[wall] = 1;
        assert_eq!(
            map.check_consistent(),
            Err("tile at Pos { x: 4, y: 4, z: 0 } has heat but no air".to_string())
        );
    }

    #[test]
    fn heat_and_heat_lost_are_part_of_the_state_hash() {
        let hash = |m: &Map| {
            let mut h = Fnv64::new();
            m.hash_into(&mut h);
            h.finish()
        };
        let a = room();
        let mut b = room();
        b.set_temperature(Pos::new(2, 2, 0), celsius_to_mk(19));
        let mut c = room();
        c.heat_lost = 1;
        assert_ne!(hash(&a), hash(&b));
        assert_ne!(hash(&a), hash(&c));
    }
}
