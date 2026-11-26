//! Positions and directions.

/// A position in the world, in tiles.
///
/// Positions are always 3D (decision D8). `z` is the deck: 0 is the deck you
/// start on, and higher numbers are further up the ship.
///
/// `Copy` means a `Pos` is copied when you pass it around, like an integer,
/// so you never need to borrow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    fn step_moves_one_tile_and_keeps_the_deck() {
        let p = Pos::new(5, 5, 2);
        assert_eq!(p.step(Dir::North), Pos::new(5, 4, 2));
        assert_eq!(p.step(Dir::SouthWest), Pos::new(4, 6, 2));
    }
}
