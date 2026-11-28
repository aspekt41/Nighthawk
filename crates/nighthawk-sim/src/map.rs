//! The tile grid, and the air and heat in it.

use serde::{Deserialize, Serialize};

use crate::air::{Mix, vent_ppm, vented_amount};
use crate::hash::Fnv64;
use crate::heat::{self, ROOM_TEMPERATURE_MK};
use crate::pos::{Dir, Pos, Vertical};
use crate::space::{Space, Survey};

/// What a tile is like, for the simulation: the properties of its
/// structure (decision D47), like whether you can walk on it and whether it
/// leaks to space. Which structure a tile is, and its material, are kept
/// separately; this copy of its properties sits beside them so the air and
/// heat loops, which visit every tile every second, read it directly.
///
/// It's not saved (it's rebuilt from each tile's structure when a game is
/// loaded), so it can never disagree with the structure it came from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tile {
    /// You can walk on it, and air fills it and moves through it. A tile
    /// that isn't walkable is solid and airtight.
    pub walkable: bool,
    /// The size of its hole to space, in cm² (a hull crack is 4, a breach
    /// 400). Only solid tiles leak: the space beside them loses air.
    pub leak_cm2: u32,
    /// A ladder: which way it goes, up to the deck above or down to the one
    /// below. Must be matched by a ladder the other way there.
    pub climb: Option<Vertical>,
    /// A solid tile you can see through, like a window. (Walkable tiles
    /// never block sight.)
    pub see_through: bool,
    /// How many tiles its light reaches, if it gives light (0 if not).
    pub light: u8,
}

impl Tile {
    pub fn is_walkable(self) -> bool {
        self.walkable
    }

    /// Whether air can be in this tile and move through it. Solid tiles
    /// (walls, shut doors) aren't: air on either side can't get past them.
    pub fn holds_air(self) -> bool {
        self.walkable
    }

    /// The area of the hole to space in this tile, in cm².
    pub fn hole_cm2(self) -> u64 {
        u64::from(self.leak_cm2)
    }

    /// Whether this tile stops sight: solid, and not see-through.
    pub fn blocks_sight(self) -> bool {
        !self.walkable && !self.see_through
    }
}

/// Tiles with each kind of property, for tests that build maps directly.
#[cfg(test)]
impl Tile {
    pub(crate) const WALL: Tile = Tile {
        walkable: false,
        leak_cm2: 0,
        climb: None,
        see_through: false,
        light: 0,
    };
    pub(crate) const WINDOW: Tile = Tile {
        see_through: true,
        ..Tile::WALL
    };
    pub(crate) const FLOOR: Tile = Tile {
        walkable: true,
        ..Tile::WALL
    };
    pub(crate) const LADDER_UP: Tile = Tile {
        climb: Some(Vertical::Up),
        ..Tile::FLOOR
    };
    pub(crate) const LADDER_DOWN: Tile = Tile {
        climb: Some(Vertical::Down),
        ..Tile::FLOOR
    };
    pub(crate) const CRACK: Tile = Tile {
        leak_cm2: 4,
        ..Tile::WALL
    };
    pub(crate) const BREACH: Tile = Tile {
        leak_cm2: 400,
        ..Tile::WALL
    };
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
    /// What each tile is like, deck by deck, row by row: a copy of its
    /// structure's properties. Not saved (`#[serde(skip)]` leaves it out,
    /// and empty when loaded); [`Map::rebuild_tiles`] fills it in again
    /// from `structure`.
    #[serde(skip)]
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
    /// What each tile is made of, in the same order as `tiles`: an index
    /// into the world's list of materials (decision D35).
    made_of: Vec<u16>,
    /// What each tile is, in the same order as `tiles`: an index into the
    /// world's list of structures (decision D37). `tiles` holds its
    /// properties.
    structure: Vec<u16>,
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
            made_of: vec![0; len],
            structure: vec![0; len],
        }
    }

    /// What the tile at `p` is made of: an index into the world's list of
    /// materials. `None` if `p` is off the map.
    pub fn material_index(&self, p: Pos) -> Option<u16> {
        self.index(p).map(|i| self.made_of[i])
    }

    /// Sets what the tile at `p` is made of. Positions off the map are
    /// ignored.
    pub(crate) fn set_material(&mut self, p: Pos, material: u16) {
        if let Some(i) = self.index(p) {
            self.made_of[i] = material;
        }
    }

    /// The highest material index any tile uses, for checking a saved map
    /// against its list of materials.
    pub(crate) fn highest_material(&self) -> u16 {
        self.made_of.iter().copied().max().unwrap_or(0)
    }

    /// What the tile at `p` is: an index into the world's list of
    /// structures. `None` if `p` is off the map.
    pub fn structure_index(&self, p: Pos) -> Option<u16> {
        self.index(p).map(|i| self.structure[i])
    }

    /// Makes the tile at `p` structure number `structure`, which is like
    /// `tile`. Setting both together keeps them matching. Positions off
    /// the map are ignored.
    pub(crate) fn place(&mut self, p: Pos, structure: u16, tile: Tile) {
        if let Some(i) = self.index(p) {
            self.structure[i] = structure;
            self.tiles[i] = tile;
        }
    }

    /// Fills in every tile's properties from its structure, after loading
    /// a save (which doesn't store them). `tile_of` gives the properties
    /// of a structure number, or `None` if there's no such structure,
    /// which is an error naming the first tile that has one.
    pub(crate) fn rebuild_tiles(
        &mut self,
        tile_of: impl Fn(u16) -> Option<Tile>,
    ) -> Result<(), String> {
        let mut tiles = Vec::with_capacity(self.structure.len());
        for (i, &structure) in self.structure.iter().enumerate() {
            let tile = tile_of(structure)
                .ok_or_else(|| format!("the tile at {:?} has no structure", self.pos_of(i)))?;
            tiles.push(tile);
        }
        self.tiles = tiles;
        Ok(())
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
        sides.into_iter().filter(|&q| self.is_outer_hull(q)).count() as u64
    }

    /// Whether `p` is part of the outer hull: a solid tile on the map's
    /// border, with deep space behind it.
    pub fn is_outer_hull(&self, p: Pos) -> bool {
        let on_border = p.x == 0 || p.y == 0 || p.x == self.width - 1 || p.y == self.height - 1;
        on_border && self.get(p).is_some_and(|tile| !tile.holds_air())
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
            // Only a space that really lost air: one already at vacuum
            // has nothing left to hiss out.
            if self.settle(&space, hole) > 0 {
                venting.extend_from_slice(&space);
            }
        }
        venting
    }

    /// Every connected open space, summed up, and which one each tile is
    /// in (decision D65): what the breach interlock on a pressure door,
    /// its gauges and the air overlay ask about.
    pub fn survey(&self) -> Survey {
        let mut space_of = vec![None; self.air.len()];
        let mut spaces = Vec::new();
        for (number, tiles) in self.connected_spaces().iter().enumerate() {
            for &i in tiles {
                space_of[i] = Some(number);
            }
            let air: u64 = tiles.iter().map(|&i| self.air[i].total()).sum();
            // A space has at least one tile, so the division is safe.
            let count = tiles.len() as u64;
            spaces.push(Space {
                tiles: u32::try_from(tiles.len()).unwrap_or(u32::MAX),
                hole_cm2: self.hole_area_around(tiles),
                pressure_pa: air / count,
            });
        }
        Survey::new((self.width, self.height, self.depth), space_of, spaces)
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
    /// Heat moves with the air: see [`heat::carry`]. Returns how much air
    /// escaped, in pascal-tiles over all three gases.
    fn settle(&mut self, space: &[usize], hole_cm2: u64) -> u64 {
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
        let mut escaped = 0;
        for (gas, total) in totals.iter_mut().enumerate() {
            let lost = vented_amount(*total, ppm);
            *total -= lost;
            self.vented[gas] += lost;
            escaped += lost;
        }
        for (k, &i) in space.iter().enumerate() {
            let share = totals.map(|total| total / count + u64::from((k as u64) < total % count));
            self.air[i] = Mix::from_amounts(share);
        }
        let after: Vec<u64> = space.iter().map(|&i| self.air[i].total()).collect();
        self.heat_lost += heat::carry(&mut self.heat, space, &before, &after);
        escaped
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
        if self.get(p).and_then(|t| t.climb) == Some(Vertical::Up) {
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

    /// Sets the tile at `p`, for tests of the physics that need no
    /// structures. (Outside tests, tiles change only with their structure,
    /// through [`Map::place`].) Positions off the map are ignored.
    #[cfg(test)]
    pub(crate) fn set(&mut self, p: Pos, tile: Tile) {
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
        if self.made_of.len() != self.tiles.len() {
            return Err(format!(
                "map has {found} tiles but materials for {}",
                self.made_of.len()
            ));
        }
        if self.structure.len() != self.tiles.len() {
            return Err(format!(
                "map has {found} tiles but structures for {}",
                self.structure.len()
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

    /// Checks that ladders come in pairs: every ladder up has a ladder down
    /// at the same spot on the deck above, and every ladder down has a
    /// ladder up below it. That's what lets climbing simply move you one
    /// deck up or down, knowing there's a way back.
    pub fn check_stairs(&self) -> Result<(), StairsError> {
        for at in self.positions() {
            let Some(way) = self.get(at).and_then(|t| t.climb) else {
                continue;
            };
            let back = self.get(at.climb(way)).and_then(|t| t.climb);
            if back != Some(way.opposite()) {
                return Err(StairsError { at, way });
            }
        }
        Ok(())
    }

    /// Feeds the map's size and every tile into a state hash. A tile's
    /// properties come from its structure, which is hashed (with the
    /// world's content), so they needn't be hashed again.
    pub(crate) fn hash_into(&self, h: &mut Fnv64) {
        h.i32(self.width).i32(self.height).i32(self.depth);
        for lost in self.vented {
            h.u64(lost);
        }
        h.u64(self.heat_lost);
        for &material in &self.made_of {
            h.u64(u64::from(material));
        }
        for &structure in &self.structure {
            h.u64(u64::from(structure));
        }
        for &heat in &self.heat {
            h.u64(heat);
        }
        for mix in &self.air {
            h.u64(u64::from(mix.o2))
                .u64(u64::from(mix.n2))
                .u64(u64::from(mix.co2));
        }
    }

    /// How many tiles the map has, over all its decks.
    pub(crate) fn tile_count(&self) -> usize {
        self.structure.len()
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
    fn walkable_tiles_hold_air_and_others_are_solid() {
        assert!(Tile::FLOOR.is_walkable() && Tile::FLOOR.holds_air());
        assert!(!Tile::WALL.is_walkable() && !Tile::WALL.holds_air());
        assert_eq!(Tile::default(), Tile::WALL, "solid unless it says");
    }

    #[test]
    fn solid_tiles_block_sight_unless_see_through() {
        assert!(Tile::WALL.blocks_sight() && Tile::CRACK.blocks_sight());
        assert!(!Tile::FLOOR.blocks_sight() && !Tile::LADDER_UP.blocks_sight());
        assert!(!Tile::WINDOW.blocks_sight() && !Tile::WINDOW.holds_air());
    }

    #[test]
    fn get_and_set_respect_all_three_axes() {
        let mut map = Map::filled(4, 3, 2, Tile::FLOOR);
        map.set(Pos::new(1, 2, 1), Tile::WALL);
        assert_eq!(map.get(Pos::new(1, 2, 1)), Some(Tile::WALL));
        // Same x and y on the other deck is untouched.
        assert_eq!(map.get(Pos::new(1, 2, 0)), Some(Tile::FLOOR));
    }

    #[test]
    fn off_map_positions_are_none() {
        let map = Map::filled(4, 3, 1, Tile::FLOOR);
        assert_eq!(map.get(Pos::new(-1, 0, 0)), None);
        assert_eq!(map.get(Pos::new(4, 0, 0)), None);
        assert_eq!(map.get(Pos::new(0, 0, 1)), None);
    }

    /// A 2×1 map with two decks: stairs up at (0, 0, 0), stairs down above.
    fn stairs_map() -> Map {
        let mut map = Map::filled(2, 1, 2, Tile::FLOOR);
        map.set(Pos::new(0, 0, 0), Tile::LADDER_UP);
        map.set(Pos::new(0, 0, 1), Tile::LADDER_DOWN);
        map
    }

    #[test]
    fn ladders_are_walkable_and_say_which_way_they_go() {
        assert!(Tile::LADDER_UP.is_walkable() && Tile::LADDER_DOWN.is_walkable());
        assert_eq!(Tile::LADDER_UP.climb, Some(Vertical::Up));
        assert_eq!(Tile::LADDER_DOWN.climb, Some(Vertical::Down));
        assert_eq!(Tile::FLOOR.climb, None);
    }

    // --- Air ------------------------------------------------------------------

    /// A one-deck corridor `w` tiles long, all floor, full of standard air.
    fn corridor(w: i32) -> Map {
        let mut map = Map::filled(w, 1, 1, Tile::FLOOR);
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
        for tile in [Tile::FLOOR, Tile::LADDER_UP, Tile::LADDER_DOWN, Tile::FLOOR] {
            assert!(tile.holds_air(), "{tile:?}");
        }
        assert!(!Tile::WALL.holds_air());
        assert!(!Tile::WALL.holds_air());
    }

    #[test]
    fn filling_a_deck_skips_walls_and_other_decks() {
        let mut map = Map::filled(2, 1, 2, Tile::FLOOR);
        map.set(Pos::new(1, 0, 0), Tile::WALL);
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
        let mut map = Map::filled(3, 1, 1, Tile::FLOOR);
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
        let mut map = Map::filled(3, 1, 1, Tile::FLOOR);
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
        for barrier in [Tile::WALL, Tile::WALL] {
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
        let mut map = Map::filled(5, 1, 1, Tile::FLOOR);
        map.set(Pos::new(2, 0, 0), Tile::WALL);
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
        map.set(Pos::new(1, 0, 0), Tile::WALL); // the doorway still has air
        map.set_air(Pos::new(2, 0, 0), Mix::VACUUM);
        for _ in 0..10 {
            map.spread_air();
        }
        assert_eq!(
            map.air(Pos::new(1, 0, 0)),
            Some(Mix::STANDARD),
            "trapped, not lost"
        );
        map.set(Pos::new(1, 0, 0), Tile::FLOOR);
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
        let mut map = Map::filled(1, 1, 2, Tile::FLOOR);
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
        let mut map = Map::filled(5, 3, 1, Tile::WALL);
        for x in 1..4 {
            map.set(Pos::new(x, 1, 0), Tile::FLOOR);
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
        for tile in [Tile::CRACK, Tile::BREACH] {
            assert!(!tile.is_walkable() && !tile.holds_air(), "{tile:?}");
        }
        assert_eq!(Tile::CRACK.hole_cm2(), 4);
        assert_eq!(Tile::BREACH.hole_cm2(), 400);
        assert_eq!(Tile::WALL.hole_cm2(), 0);
    }

    #[test]
    fn a_breach_vents_a_small_room_to_vacuum_in_seconds() {
        let mut map = holed_room(Tile::BREACH);
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
    fn a_survey_sums_up_each_space_its_size_holes_and_pressure() {
        let holed = holed_room(Tile::BREACH).survey();
        let room = holed.at(Pos::new(2, 1, 0)).unwrap();
        assert_eq!(room.tiles, 3);
        assert_eq!(room.hole_cm2, 400);
        assert_eq!(room.pressure_pa, Mix::STANDARD.total());
        assert!(room.is_venting() && holed.is_venting(Pos::new(1, 1, 0)));
        assert_eq!(holed.at(Pos::new(0, 1, 0)), None, "the breach holds no air");
        let sealed = holed_room(Tile::WALL).survey();
        assert!(!sealed.is_venting(Pos::new(2, 1, 0)));
        let beside_breach = holed.beside(Pos::new(0, 1, 0));
        assert_eq!(beside_breach, vec![(Dir::East, room)]);
    }

    #[test]
    fn a_crack_leaks_slowly() {
        let mut map = holed_room(Tile::CRACK);
        for _ in 0..10 {
            map.spread_air();
        }
        assert!(room_pressure(&map) > Mix::STANDARD.total() * 9 / 10);
        assert!(room_pressure(&map) < Mix::STANDARD.total());
    }

    #[test]
    fn air_on_board_plus_air_vented_never_changes() {
        let mut map = holed_room(Tile::BREACH);
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
        let mut breached = holed_room(Tile::BREACH);
        let room: Vec<usize> = (1..4)
            .map(|x| breached.index_of(Pos::new(x, 1, 0)).unwrap())
            .collect();
        assert_eq!(breached.spread_air(), room);
        assert!(
            holed_room(Tile::WALL).spread_air().is_empty(),
            "sealed rooms report nothing"
        );
        for _ in 0..30 {
            breached.spread_air();
        }
        assert_eq!(room_pressure(&breached), 0);
        assert!(
            breached.spread_air().is_empty(),
            "nor do breached ones with nothing left to lose"
        );
    }

    #[test]
    fn a_closed_door_protects_the_room_behind_it() {
        // `X.+.#`: breach, room, closed door, room.
        let mut map = holed_room(Tile::BREACH);
        map.set(Pos::new(2, 1, 0), Tile::WALL);
        let behind = Pos::new(3, 1, 0);
        for _ in 0..60 {
            map.spread_air();
        }
        assert_eq!(map.air(behind), Some(Mix::STANDARD), "sealed off");
        map.set(Pos::new(2, 1, 0), Tile::FLOOR);
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
        let mut map = Map::filled(3, 3, 1, Tile::FLOOR);
        map.set(Pos::new(1, 1, 0), Tile::BREACH);
        let space: Vec<usize> = map.positions().filter_map(|p| map.air_index(p)).collect();
        assert_eq!(map.hole_area_around(&space), 400);
    }

    #[test]
    fn positions_and_indexes_convert_both_ways() {
        let map = Map::filled(3, 2, 2, Tile::FLOOR);
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
        let a = holed_room(Tile::BREACH);
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
        let mut map = Map::filled(5, 5, 1, Tile::WALL);
        for y in 1..4 {
            for x in 1..4 {
                map.set(Pos::new(x, y, 0), Tile::FLOOR);
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
    fn the_outer_hull_is_the_solid_border() {
        let mut map = room();
        assert!(map.is_outer_hull(Pos::new(0, 2, 0)));
        assert!(!map.is_outer_hull(Pos::new(2, 2, 0)), "floor inside");
        assert!(!map.is_outer_hull(Pos::new(9, 9, 9)), "off the map");
        map.set(Pos::new(0, 2, 0), Tile::FLOOR);
        assert!(
            !map.is_outer_hull(Pos::new(0, 2, 0)),
            "open border tiles aren't hull"
        );
    }

    #[test]
    fn inner_bulkheads_are_not_hull() {
        // A wall inside the map has ship on both sides, not space.
        let mut map = Map::filled(7, 3, 1, Tile::WALL);
        for x in 1..6 {
            map.set(Pos::new(x, 1, 0), Tile::FLOOR);
        }
        map.set(Pos::new(3, 1, 0), Tile::WALL);
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
        map.set(Pos::new(2, 0, 0), Tile::BREACH);
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

#[cfg(test)]
mod material_tests {
    use super::*;

    #[test]
    fn tiles_start_as_material_0_and_can_be_changed() {
        let mut map = Map::filled(2, 1, 1, Tile::WALL);
        let p = Pos::new(1, 0, 0);
        assert_eq!(map.material_index(p), Some(0));
        map.set_material(p, 3);
        map.set_material(Pos::new(9, 9, 9), 3); // off the map: ignored
        assert_eq!(map.material_index(p), Some(3));
        assert_eq!(map.highest_material(), 3);
        assert_eq!(map.material_index(Pos::new(5, 0, 0)), None);
    }

    #[test]
    fn tiles_start_as_structure_0_and_are_placed_with_their_properties() {
        let mut map = Map::filled(2, 1, 1, Tile::WALL);
        let p = Pos::new(1, 0, 0);
        assert_eq!(map.structure_index(p), Some(0));
        map.place(p, 4, Tile::FLOOR);
        map.place(Pos::new(9, 9, 9), 4, Tile::FLOOR); // off the map: ignored
        assert_eq!(map.structure_index(p), Some(4));
        assert_eq!(map.get(p), Some(Tile::FLOOR), "both change together");
        assert_eq!(map.structure_index(Pos::new(5, 0, 0)), None);
    }

    #[test]
    fn tiles_are_rebuilt_from_their_structures_after_loading() {
        let mut map = Map::filled(2, 1, 1, Tile::WALL);
        map.place(Pos::new(1, 0, 0), 1, Tile::FLOOR);
        let mut loaded: Map = serde_json::from_value(serde_json::to_value(&map).unwrap()).unwrap();
        assert!(loaded.tiles.is_empty(), "tiles aren't saved");
        let tile_of = |s: u16| [Tile::WALL, Tile::FLOOR].get(usize::from(s)).copied();
        loaded.rebuild_tiles(tile_of).unwrap();
        assert_eq!(loaded, map);
        loaded.place(Pos::new(0, 0, 0), 7, Tile::WALL);
        assert_eq!(
            loaded.rebuild_tiles(tile_of),
            Err("the tile at Pos { x: 0, y: 0, z: 0 } has no structure".to_string())
        );
    }

    #[test]
    fn mismatched_structure_data_is_inconsistent() {
        let mut map = Map::filled(2, 1, 1, Tile::WALL);
        map.structure.pop();
        assert_eq!(
            map.check_consistent(),
            Err("map has 2 tiles but structures for 1".to_string())
        );
    }

    #[test]
    fn structures_are_part_of_the_state_hash() {
        let hash = |m: &Map| {
            let mut h = Fnv64::new();
            m.hash_into(&mut h);
            h.finish()
        };
        let a = Map::filled(2, 1, 1, Tile::WALL);
        let mut b = a.clone();
        b.place(Pos::new(0, 0, 0), 1, Tile::WALL);
        assert_ne!(hash(&a), hash(&b));
    }

    #[test]
    fn mismatched_material_data_is_inconsistent() {
        let mut map = Map::filled(2, 1, 1, Tile::WALL);
        map.made_of.pop();
        assert_eq!(
            map.check_consistent(),
            Err("map has 2 tiles but materials for 1".to_string())
        );
    }

    #[test]
    fn materials_are_part_of_the_state_hash() {
        let hash = |m: &Map| {
            let mut h = Fnv64::new();
            m.hash_into(&mut h);
            h.finish()
        };
        let a = Map::filled(2, 1, 1, Tile::WALL);
        let mut b = a.clone();
        b.set_material(Pos::new(0, 0, 0), 1);
        assert_ne!(hash(&a), hash(&b));
    }
}
