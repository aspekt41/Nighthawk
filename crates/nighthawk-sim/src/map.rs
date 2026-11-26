//! The tile grid.

use serde::{Deserialize, Serialize};

use crate::hash::Fnv64;
use crate::pos::{Pos, Vertical};

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
}

impl Tile {
    pub fn is_walkable(self) -> bool {
        match self {
            Tile::Floor | Tile::StairsUp | Tile::StairsDown | Tile::DoorOpen => true,
            Tile::Wall | Tile::DoorClosed => false,
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
        }
    }
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
        }
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
        self.check_stairs().map_err(|e| e.to_string())
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
}
