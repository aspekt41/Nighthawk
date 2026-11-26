//! Positions and directions.

use serde::{Deserialize, Serialize};

/// A position in the world, in tiles.
///
/// Positions are always 3D (decision D8). `z` is the deck: 0 is the deck you
/// start on, and higher numbers are further up the ship.
///
/// `Copy` means a `Pos` is copied when you pass it around, like an integer,
/// so you never need to borrow it.
///
/// `Serialize` and `Deserialize` let it be written into save files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Pos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// The neighbouring position one step in `dir`.
    pub fn step(self, dir: Dir) -> Pos {
        let (dx, dy) = dir.delta();
        Pos::new(self.x + dx, self.y + dy, self.z)
    }

    /// The same spot one deck up or down.
    pub fn climb(self, way: Vertical) -> Pos {
        Pos::new(self.x, self.y, self.z + way.dz())
    }
}

/// Up or down between decks. Up is `z + 1`: higher decks have higher numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vertical {
    Up,
    Down,
}

impl Vertical {
    pub fn dz(self) -> i32 {
        match self {
            Vertical::Up => 1,
            Vertical::Down => -1,
        }
    }
}

/// The eight horizontal directions. Moving between decks (up and down)
/// arrives with stairs and hatches in M0.4.
///
/// Screen convention: `y` grows downwards, so North is `y - 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

impl Dir {
    /// All eight directions, clockwise from North.
    pub const ALL: [Dir; 8] = [
        Dir::North,
        Dir::NorthEast,
        Dir::East,
        Dir::SouthEast,
        Dir::South,
        Dir::SouthWest,
        Dir::West,
        Dir::NorthWest,
    ];

    /// The `(dx, dy)` this direction moves by.
    pub fn delta(self) -> (i32, i32) {
        match self {
            Dir::North => (0, -1),
            Dir::NorthEast => (1, -1),
            Dir::East => (1, 0),
            Dir::SouthEast => (1, 1),
            Dir::South => (0, 1),
            Dir::SouthWest => (-1, 1),
            Dir::West => (-1, 0),
            Dir::NorthWest => (-1, -1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_direction_is_one_distinct_step() {
        let deltas: Vec<_> = Dir::ALL.iter().map(|d| d.delta()).collect();
        for &(dx, dy) in &deltas {
            assert!((-1..=1).contains(&dx) && (-1..=1).contains(&dy));
            assert_ne!((dx, dy), (0, 0), "no direction stands still");
        }
        let mut unique = deltas.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 8, "all eight directions differ");
    }

    #[test]
    fn opposite_directions_cancel_out() {
        // ALL is clockwise, so the opposite of ALL[i] is ALL[i + 4].
        for i in 0..4 {
            let (a, b) = (Dir::ALL[i].delta(), Dir::ALL[i + 4].delta());
            assert_eq!((a.0 + b.0, a.1 + b.1), (0, 0), "{:?}", Dir::ALL[i]);
        }
    }

    #[test]
    fn north_is_up_the_screen() {
        assert_eq!(Dir::North.delta(), (0, -1));
        assert_eq!(Dir::East.delta(), (1, 0));
    }

    #[test]
    fn climbing_changes_only_the_deck() {
        let p = Pos::new(3, 4, 1);
        assert_eq!(p.climb(Vertical::Up), Pos::new(3, 4, 2));
        assert_eq!(p.climb(Vertical::Down), Pos::new(3, 4, 0));
    }

    #[test]
    fn step_moves_one_tile_and_keeps_the_deck() {
        let p = Pos::new(5, 5, 2);
        assert_eq!(p.step(Dir::North), Pos::new(5, 4, 2));
        assert_eq!(p.step(Dir::SouthWest), Pos::new(4, 6, 2));
    }
}
