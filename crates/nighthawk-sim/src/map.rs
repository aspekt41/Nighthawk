//! The tile grid.

use serde::{Deserialize, Serialize};

use crate::hash::Fnv64;
use crate::pos::Pos;

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
}

impl Tile {
    pub fn is_walkable(self) -> bool {
        match self {
            Tile::Floor => true,
            Tile::Wall => false,
        }
    }

    /// A fixed number per tile type, for [`Map::hash_into`]. These numbers
    /// are part of the determinism contract: never change or reuse one.
    fn hash_id(self) -> u8 {
        match self {
            Tile::Floor => 0,
            Tile::Wall => 1,
        }
    }

    /// A short name for messages, like "You bump into the wall."
    pub fn name(self) -> &'static str {
        match self {
            Tile::Floor => "floor",
            Tile::Wall => "bulkhead",
        }
    }
}

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
}
