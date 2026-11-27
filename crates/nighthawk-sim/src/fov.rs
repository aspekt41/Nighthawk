//! Field of view: which tiles can be seen from a point (decision D49).
//!
//! This is *symmetric shadowcasting*, after Albert Ford's write-up of it
//! (albertford.com/shadowcasting). The view is split into four quadrants
//! (north, east, south, west). Each is scanned row by row outwards from the
//! origin; a wall in a row casts a shadow over the rows behind it, which
//! narrows the slopes the next rows are scanned between. A floor tile is
//! seen only if its *centre* is inside the lit part of its row, which makes
//! the result symmetric: if A can see B, B can see A. (That matters once
//! there are creatures: you can't see something that can't see you.) Walls
//! are seen if any part of them is lit, so rooms show their edges.
//!
//! Every slope is kept as an exact fraction of whole numbers, never a
//! floating-point number, so every machine computes the same view
//! (decision D22). It works on one deck: `z` never changes.

use crate::pos::Pos;

/// Calls `see` for every tile visible from `origin` within `range` rows
/// (a square, not a circle: callers that want a circle check distance
/// themselves). `blocks` says whether a tile stops sight; positions off
/// the map should block. The origin is always seen, whether or not it
/// blocks. A tile may be seen more than once.
pub(crate) fn field_of_view(
    origin: Pos,
    range: i32,
    blocks: &dyn Fn(Pos) -> bool,
    see: &mut dyn FnMut(Pos),
) {
    see(origin);
    for quadrant in Quadrant::ALL {
        let mut caster = Caster {
            origin,
            range,
            quadrant,
            blocks,
            see: &mut *see,
        };
        caster.scan(Row::first());
    }
}

/// A slope as a fraction `num / den`, with `den` always positive. Slopes
/// say where a row's lit part starts and ends: column = depth × slope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slope {
    num: i32,
    den: i32,
}

impl Slope {
    /// The slope through the near edge of the tile at `col` in the row at
    /// `depth`: `(2·col − 1) / (2·depth)`.
    fn tile_edge(depth: i32, col: i32) -> Slope {
        Slope {
            num: 2 * col - 1,
            den: 2 * depth,
        }
    }
}

/// One row of a quadrant, `depth` tiles out from the origin, lit between
/// two slopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Row {
    depth: i32,
    start: Slope,
    end: Slope,
}

impl Row {
    /// The row right beside the origin, lit from corner to corner.
    fn first() -> Row {
        Row {
            depth: 1,
            start: Slope { num: -1, den: 1 },
            end: Slope { num: 1, den: 1 },
        }
    }

    /// The row behind this one, lit between the same slopes.
    fn next(self) -> Row {
        Row {
            depth: self.depth + 1,
            ..self
        }
    }

    /// The first column touched by the lit part: depth × start, rounded
    /// with halves going up. In whole numbers that's
    /// floor((2·depth·num + den) / (2·den)).
    fn min_col(self) -> i32 {
        let Slope { num, den } = self.start;
        (2 * self.depth * num + den).div_euclid(2 * den)
    }

    /// The last column touched: depth × end, rounded with halves going
    /// down, which is ceil((2·depth·num − den) / (2·den)). (`div_euclid`
    /// rounds down; negating before and after turns that into rounding up.)
    fn max_col(self) -> i32 {
        let Slope { num, den } = self.end;
        -(-(2 * self.depth * num - den)).div_euclid(2 * den)
    }

    /// Whether the centre of the tile at `col` is inside the lit part,
    /// edges included: start ≤ col / depth ≤ end.
    fn is_symmetric(self, col: i32) -> bool {
        col * self.start.den >= self.depth * self.start.num
            && col * self.end.den <= self.depth * self.end.num
    }
}

/// Which way a quadrant looks from the origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quadrant {
    North,
    East,
    South,
    West,
}

impl Quadrant {
    const ALL: [Quadrant; 4] = [
        Quadrant::North,
        Quadrant::East,
        Quadrant::South,
        Quadrant::West,
    ];

    /// The map position of the tile `depth` rows out and `col` columns
    /// across, in this quadrant.
    fn place(self, origin: Pos, depth: i32, col: i32) -> Pos {
        let Pos { x, y, z } = origin;
        match self {
            Quadrant::North => Pos::new(x + col, y - depth, z),
            Quadrant::South => Pos::new(x + col, y + depth, z),
            Quadrant::East => Pos::new(x + depth, y + col, z),
            Quadrant::West => Pos::new(x - depth, y + col, z),
        }
    }
}

/// Scans one quadrant. `blocks` and `see` are borrowed closures
/// (`&dyn Fn`), so this one struct works for any map and any way of
/// marking tiles seen.
struct Caster<'a> {
    origin: Pos,
    range: i32,
    quadrant: Quadrant,
    blocks: &'a dyn Fn(Pos) -> bool,
    see: &'a mut dyn FnMut(Pos),
}

impl Caster<'_> {
    /// Scans `row`, then the rows behind it, splitting the scan wherever a
    /// wall casts a shadow. Each recursion is one row further out, so it
    /// goes at most `range` deep.
    fn scan(&mut self, mut row: Row) {
        if row.depth > self.range {
            return;
        }
        // Whether the tile before this one in the row was a wall; `None`
        // before the first tile.
        let mut previous: Option<bool> = None;
        for col in row.min_col()..=row.max_col() {
            let wall = self.is_wall(row.depth, col);
            if wall || row.is_symmetric(col) {
                (self.see)(self.quadrant.place(self.origin, row.depth, col));
            }
            self.after_tile(&mut row, col, previous, wall);
            previous = Some(wall);
        }
        if previous == Some(false) {
            self.scan(row.next());
        }
    }

    /// What a change from wall to floor (or floor to wall) does to the
    /// scan: leaving a wall narrows where the lit part starts; reaching
    /// one ends a lit part, which is scanned on into the next row.
    fn after_tile(&mut self, row: &mut Row, col: i32, previous: Option<bool>, wall: bool) {
        match (previous, wall) {
            (Some(true), false) => row.start = Slope::tile_edge(row.depth, col),
            (Some(false), true) => {
                let mut next = row.next();
                next.end = Slope::tile_edge(row.depth, col);
                self.scan(next);
            }
            _ => {}
        }
    }

    fn is_wall(&self, depth: i32, col: i32) -> bool {
        (self.blocks)(self.quadrant.place(self.origin, depth, col))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A deck from rows of text: `#` blocks sight, anything else doesn't,
    /// and off the edges blocks.
    struct Grid {
        rows: Vec<Vec<char>>,
    }

    impl Grid {
        fn new(rows: &[&str]) -> Grid {
            Grid {
                rows: rows.iter().map(|r| r.chars().collect()).collect(),
            }
        }

        fn blocks(&self, p: Pos) -> bool {
            let cell = usize::try_from(p.y)
                .ok()
                .and_then(|y| self.rows.get(y))
                .and_then(|row| usize::try_from(p.x).ok().and_then(|x| row.get(x)));
            cell.is_none_or(|&c| c == '#')
        }

        /// Every tile seen from `(x, y)`.
        fn seen(&self, x: i32, y: i32, range: i32) -> HashSet<Pos> {
            let mut seen = HashSet::new();
            field_of_view(Pos::new(x, y, 0), range, &|p| self.blocks(p), &mut |p| {
                seen.insert(p);
            });
            seen
        }
    }

    fn at(x: i32, y: i32) -> Pos {
        Pos::new(x, y, 0)
    }

    #[test]
    fn in_an_empty_room_everything_is_seen_walls_included() {
        let grid = Grid::new(&["#####", "#...#", "#...#", "#####"]);
        let seen = grid.seen(1, 1, 10);
        assert_eq!(seen.len(), 20, "all 5 × 4 tiles");
    }

    #[test]
    fn a_pillar_hides_what_is_behind_it_but_is_seen_itself() {
        let grid = Grid::new(&["#######", "#.....#", "#..#..#", "#.....#", "#######"]);
        // From (1, 2), the pillar at (3, 2) hides (4, 2) and (5, 2).
        let seen = grid.seen(1, 2, 10);
        assert!(seen.contains(&at(3, 2)), "the pillar");
        assert!(!seen.contains(&at(4, 2)) && !seen.contains(&at(5, 2)));
        assert!(
            seen.contains(&at(5, 1)) && seen.contains(&at(5, 3)),
            "past its corners"
        );
    }

    #[test]
    fn walls_keep_you_from_seeing_into_the_next_room() {
        let grid = Grid::new(&["#######", "#..#..#", "#..#..#", "#######"]);
        let seen = grid.seen(1, 1, 10);
        assert!(seen.contains(&at(3, 1)), "the dividing wall");
        assert!(!seen.contains(&at(4, 1)) && !seen.contains(&at(5, 2)));
    }

    #[test]
    fn the_origin_is_always_seen_even_inside_a_wall() {
        let grid = Grid::new(&["###", "###", "###"]);
        let seen = grid.seen(1, 1, 5);
        assert!(seen.contains(&at(1, 1)));
        assert_eq!(seen.len(), 9, "and the walls touching it");
    }

    #[test]
    fn range_limits_how_far_out_the_scan_goes() {
        let grid = Grid::new(&["#########", "#.......#", "#########"]);
        let seen = grid.seen(1, 1, 3);
        assert!(seen.contains(&at(4, 1)));
        assert!(
            !seen.contains(&at(5, 1)),
            "four tiles out is beyond range 3"
        );
    }

    #[test]
    fn sight_is_symmetric_between_any_two_floor_tiles() {
        // If A sees B, B sees A: checked for every pair of floor tiles in
        // a room cluttered with pillars and an alcove.
        let rows = [
            "##########",
            "#....#...#",
            "#.##.#.#.#",
            "#........#",
            "#.#..##..#",
            "#...#....#",
            "##########",
        ];
        let grid = Grid::new(&rows);
        let floors: Vec<Pos> = (0..7)
            .flat_map(|y| (0..10).map(move |x| at(x, y)))
            .filter(|&p| !grid.blocks(p))
            .collect();
        for &a in &floors {
            let from_a = grid.seen(a.x, a.y, 20);
            for &b in &floors {
                let b_sees_a = grid.seen(b.x, b.y, 20).contains(&a);
                assert_eq!(from_a.contains(&b), b_sees_a, "{a:?} and {b:?}");
            }
        }
    }

    #[test]
    fn rounding_to_columns_goes_halves_up_at_the_start_and_halves_down_at_the_end() {
        // depth 1, slopes ±1/2: depth × slope is ±0.5 exactly.
        let row = Row {
            depth: 1,
            start: Slope { num: -1, den: 2 },
            end: Slope { num: 1, den: 2 },
        };
        assert_eq!((row.min_col(), row.max_col()), (0, 0));
        assert!(row.is_symmetric(0) && !row.is_symmetric(1));
        assert_eq!(Slope::tile_edge(2, 1), Slope { num: 1, den: 4 });
        assert_eq!(Row::first().next().depth, 2);
    }
}
