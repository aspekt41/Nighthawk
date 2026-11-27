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

    /// How many steps apart two spots on the same deck are, counting a
    /// diagonal step as one (like a king in chess). `None` on different
    /// decks, where "how far" isn't a number of steps.
    pub fn steps_to(self, other: Pos) -> Option<u32> {
        (self.z == other.z).then(|| self.x.abs_diff(other.x).max(self.y.abs_diff(other.y)))
    }

    /// The tiles a straight line from here to `other` passes over, not
    /// counting either end: what a laser beam would cross. Uses
    /// Bresenham's line algorithm, which steps one tile at a time with
    /// whole numbers only, so it's exact and the same on every machine.
    /// Both ends should be on the same deck; the line stays on this one.
    pub fn line_to(self, other: Pos) -> Vec<Pos> {
        let (dx, dy) = ((other.x - self.x).abs(), -(other.y - self.y).abs());
        let (sx, sy) = ((other.x - self.x).signum(), (other.y - self.y).signum());
        let (mut x, mut y, mut err) = (self.x, self.y, dx + dy);
        let mut between = Vec::new();
        loop {
            // `err` tracks how far the line has drifted from the ideal one;
            // each step moves along whichever axis brings it back closest.
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
            if (x, y) == (other.x, other.y) {
                return between;
            }
            between.push(Pos::new(x, y, self.z));
        }
    }
}

/// Up or down between decks. Up is `z + 1`: higher decks have higher numbers.
/// In data files it's `"up"` or `"down"` (a ladder's `climb`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

    /// The other way: up for down, down for up.
    pub fn opposite(self) -> Vertical {
        match self {
            Vertical::Up => Vertical::Down,
            Vertical::Down => Vertical::Up,
        }
    }

    /// A fixed number per way, for the state hash.
    pub(crate) fn hash_id(self) -> u8 {
        match self {
            Vertical::Up => 0,
            Vertical::Down => 1,
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

    /// Whether this is one of the four diagonals. A diagonal step covers
    /// about 1.4 m instead of 1 m, so it takes longer.
    pub fn is_diagonal(self) -> bool {
        let (dx, dy) = self.delta();
        dx != 0 && dy != 0
    }

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
    fn exactly_four_directions_are_diagonal() {
        let diagonals: Vec<Dir> = Dir::ALL.into_iter().filter(|d| d.is_diagonal()).collect();
        assert_eq!(
            diagonals,
            vec![
                Dir::NorthEast,
                Dir::SouthEast,
                Dir::SouthWest,
                Dir::NorthWest
            ]
        );
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

#[cfg(test)]
mod reach_tests {
    use super::*;

    #[test]
    fn steps_count_diagonals_as_one_and_need_the_same_deck() {
        let here = Pos::new(1, 1, 0);
        assert_eq!(here.steps_to(here), Some(0));
        assert_eq!(here.steps_to(Pos::new(1, 3, 0)), Some(2));
        assert_eq!(here.steps_to(Pos::new(3, 3, 0)), Some(2), "diagonal");
        assert_eq!(here.steps_to(Pos::new(4, 2, 0)), Some(3));
        assert_eq!(here.steps_to(Pos::new(1, 1, 1)), None, "another deck");
    }

    #[test]
    fn a_line_lists_the_tiles_between_but_not_the_ends() {
        let here = Pos::new(1, 1, 0);
        assert_eq!(here.line_to(Pos::new(1, 3, 0)), vec![Pos::new(1, 2, 0)]);
        assert_eq!(here.line_to(Pos::new(3, 3, 0)), vec![Pos::new(2, 2, 0)]);
        assert_eq!(
            here.line_to(Pos::new(5, 1, 0)),
            vec![Pos::new(2, 1, 0), Pos::new(3, 1, 0), Pos::new(4, 1, 0)]
        );
        assert_eq!(here.line_to(Pos::new(2, 1, 0)), vec![], "neighbours");
    }

    #[test]
    fn lines_work_in_every_direction() {
        let here = Pos::new(5, 5, 2);
        assert_eq!(here.line_to(Pos::new(3, 5, 2)), vec![Pos::new(4, 5, 2)]);
        assert_eq!(here.line_to(Pos::new(5, 3, 2)), vec![Pos::new(5, 4, 2)]);
        assert_eq!(here.line_to(Pos::new(3, 3, 2)), vec![Pos::new(4, 4, 2)]);
        // A knight's-move line passes over one of the two in-between tiles.
        assert_eq!(here.line_to(Pos::new(7, 6, 2)).len(), 1);
    }
}
