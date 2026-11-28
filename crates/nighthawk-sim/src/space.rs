//! Spaces: the connected pockets of air on the map, summed up (decision
//! D65).
//!
//! Air evens out across each connected open space every second (D30), so
//! a space is the natural unit for questions like "is this room venting
//! to space?" or "how much air is behind that door?". A [`Survey`] works
//! out every space once, so the game can ask many such questions (one per
//! map cell, when the air overlay is drawn) without searching the map
//! each time.

use crate::pos::{Dir, Pos};

/// One connected open space, summed up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Space {
    /// How many tiles of air it has.
    pub tiles: u32,
    /// The total area of the hull damage along its edge, in cm². More than
    /// 0 means it's open to space: it's losing whatever air it has, and
    /// would lose any air let into it.
    pub hole_cm2: u64,
    /// Its air pressure, in pascals: the air in all its tiles shared
    /// evenly between them.
    pub pressure_pa: u64,
}

impl Space {
    /// Whether it's open to space through hull damage.
    pub fn is_venting(self) -> bool {
        self.hole_cm2 > 0
    }
}

/// Every connected open space on the map, and which one each tile is in.
/// Made by [`crate::Map::survey`]; it's a snapshot, so it goes out of date
/// as soon as the map changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Survey {
    /// The map's width, height and depth, to find a position's tile.
    size: (i32, i32, i32),
    /// For each tile (in the map's order), the number of the space it's in;
    /// `None` for solid tiles, which hold no air.
    space_of: Vec<Option<usize>>,
    spaces: Vec<Space>,
}

impl Survey {
    pub(crate) fn new(
        size: (i32, i32, i32),
        space_of: Vec<Option<usize>>,
        spaces: Vec<Space>,
    ) -> Self {
        Survey {
            size,
            space_of,
            spaces,
        }
    }

    /// The number of the space the tile at `p` is in, if it's on the map
    /// and holds air.
    fn number_at(&self, p: Pos) -> Option<usize> {
        let (width, height, depth) = self.size;
        let on_map =
            (0..width).contains(&p.x) && (0..height).contains(&p.y) && (0..depth).contains(&p.z);
        if !on_map {
            return None;
        }
        let i = usize::try_from(p.x + p.y * width + p.z * width * height).ok()?;
        self.space_of.get(i).copied().flatten()
    }

    /// The space the tile at `p` is in, if it holds air.
    pub fn at(&self, p: Pos) -> Option<Space> {
        self.number_at(p).map(|n| self.spaces[n])
    }

    /// Whether the tile at `p` is in a space that's venting to space.
    pub fn is_venting(&self, p: Pos) -> bool {
        self.at(p).is_some_and(Space::is_venting)
    }

    /// The spaces on the four sides of `p` (on its own deck), each listed
    /// once with the first side it was found on, looking north, east,
    /// south, then west. For a shut door, that's the air on each side of
    /// it.
    pub fn beside(&self, p: Pos) -> Vec<(Dir, Space)> {
        let mut seen = Vec::new();
        let mut sides = Vec::new();
        for dir in [Dir::North, Dir::East, Dir::South, Dir::West] {
            if let Some(n) = self.number_at(p.step(dir))
                && !seen.contains(&n)
            {
                seen.push(n);
                sides.push((dir, self.spaces[n]));
            }
        }
        sides
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4 × 1 strip: two tiles in space 0, a wall, then a tile in space 1.
    fn strip() -> Survey {
        let open = Space {
            tiles: 2,
            hole_cm2: 0,
            pressure_pa: 100,
        };
        let holed = Space {
            tiles: 1,
            hole_cm2: 400,
            pressure_pa: 0,
        };
        Survey::new(
            (4, 1, 1),
            vec![Some(0), Some(0), None, Some(1)],
            vec![open, holed],
        )
    }

    #[test]
    fn a_tile_knows_its_space_and_whether_it_vents() {
        let survey = strip();
        assert_eq!(survey.at(Pos::new(1, 0, 0)).map(|s| s.tiles), Some(2));
        assert_eq!(survey.at(Pos::new(2, 0, 0)), None, "the wall holds no air");
        assert_eq!(survey.at(Pos::new(9, 0, 0)), None, "off the map");
        assert_eq!(survey.at(Pos::new(0, 0, 1)), None, "no such deck");
        assert!(!survey.is_venting(Pos::new(0, 0, 0)));
        assert!(survey.is_venting(Pos::new(3, 0, 0)));
    }

    #[test]
    fn beside_a_wall_are_the_spaces_on_each_side_each_once() {
        let survey = strip();
        let sides = survey.beside(Pos::new(2, 0, 0));
        assert_eq!(sides.len(), 2);
        assert_eq!(sides[0].0, Dir::East);
        assert!(sides[0].1.is_venting());
        assert_eq!((sides[1].0, sides[1].1.tiles), (Dir::West, 2));
        // A pillar in a room: the same space on three sides, listed once.
        let room = Space {
            tiles: 5,
            hole_cm2: 0,
            pressure_pa: 100,
        };
        let pillar = Survey::new(
            (3, 2, 1),
            vec![Some(0), None, Some(0), Some(0), Some(0), Some(0)],
            vec![room],
        );
        assert_eq!(pillar.beside(Pos::new(1, 0, 0)), vec![(Dir::East, room)]);
    }
}
