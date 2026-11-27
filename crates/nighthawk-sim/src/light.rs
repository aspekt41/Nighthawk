//! Light: which tiles are lit, and so can be seen from afar (decision D50).
//!
//! A light is a structure built to give light (an emergency light set into
//! a bulkhead) or an item that gives it (a hand lamp), lying on the floor
//! or carried. Each one lights the tiles it could "see", using the same
//! field of view as the player's, so walls and pillars cast shadows; and
//! only within its reach, measured as the crow flies: a tile `dx` across
//! and `dy` down from the light is lit if dx² + dy² ≤ reach². That makes a
//! circle of light rather than a square, still in whole numbers only
//! (decision D22).
//!
//! Lit tiles can be seen from anywhere in view; in the dark you only see
//! the tiles right around you ([`crate::sight`]).

use crate::fov::field_of_view;
use crate::map::{Map, Tile};
use crate::pos::Pos;

/// The furthest any light reaches, in tiles. Content is checked against
/// it, so a typo can't make a light that floods the ship (and costs a
/// field of view across all of it after every command).
pub const MAX_LIGHT: u8 = 20;

/// A light: where it is, and how many tiles its light reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Light {
    pub(crate) at: Pos,
    pub(crate) reach: u8,
}

/// Which tiles any of `lights` lights up: one flag per tile, in the map's
/// own order (see [`Map::index_of`]).
pub(crate) fn lit_tiles(map: &Map, lights: &[Light]) -> Vec<bool> {
    let mut lit = vec![false; map.tile_count()];
    let blocks = |p: Pos| map.get(p).is_none_or(Tile::blocks_sight);
    for light in lights {
        let reach = i32::from(light.reach);
        field_of_view(light.at, reach, &blocks, &mut |p| {
            let (dx, dy) = (p.x - light.at.x, p.y - light.at.y);
            if dx * dx + dy * dy <= reach * reach
                && let Some(i) = map.index_of(p)
            {
                lit[i] = true;
            }
        });
    }
    lit
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An 11 × 5 room of floor with walls all round, and a wall across it
    /// at x = 7 (rows 1 to 3).
    fn room() -> Map {
        let mut map = Map::filled(11, 5, 1, Tile::WALL);
        let floor =
            (1..4).flat_map(|y| (1..10).filter(|&x| x != 7).map(move |x| Pos::new(x, y, 0)));
        for p in floor {
            map.place(p, 1, Tile::FLOOR);
        }
        map
    }

    fn is_lit(map: &Map, lit: &[bool], x: i32, y: i32) -> bool {
        map.index_of(Pos::new(x, y, 0)).is_some_and(|i| lit[i])
    }

    #[test]
    fn a_light_lights_a_circle_around_it() {
        let map = room();
        let lit = lit_tiles(
            &map,
            &[Light {
                at: Pos::new(3, 2, 0),
                reach: 2,
            }],
        );
        assert!(is_lit(&map, &lit, 3, 2), "where it is");
        assert!(
            is_lit(&map, &lit, 5, 2) && is_lit(&map, &lit, 1, 2),
            "2 across"
        );
        assert!(is_lit(&map, &lit, 4, 3), "1 across and 1 down: 2 ≤ 4");
        assert!(!is_lit(&map, &lit, 5, 3), "2 across and 1 down: 5 > 4");
        assert!(!is_lit(&map, &lit, 6, 2), "beyond its reach");
    }

    #[test]
    fn walls_cast_shadows_and_are_lit_on_the_side_facing_the_light() {
        let map = room();
        let lit = lit_tiles(
            &map,
            &[Light {
                at: Pos::new(5, 2, 0),
                reach: 9,
            }],
        );
        assert!(is_lit(&map, &lit, 7, 2), "the wall itself");
        assert!(!is_lit(&map, &lit, 8, 2) && !is_lit(&map, &lit, 9, 1));
    }

    #[test]
    fn several_lights_add_up_and_no_lights_is_dark() {
        let map = room();
        assert!(lit_tiles(&map, &[]).iter().all(|&l| !l), "dark");
        let both = [
            Light {
                at: Pos::new(1, 2, 0),
                reach: 1,
            },
            Light {
                at: Pos::new(9, 2, 0),
                reach: 1,
            },
        ];
        let lit = lit_tiles(&map, &both);
        assert!(is_lit(&map, &lit, 2, 2) && is_lit(&map, &lit, 8, 2));
        assert!(!is_lit(&map, &lit, 5, 2), "dark in between");
    }

    #[test]
    fn a_light_set_into_a_wall_shines_out_of_it() {
        let map = room();
        // In the dividing wall: it lights both sides.
        let lit = lit_tiles(
            &map,
            &[Light {
                at: Pos::new(7, 2, 0),
                reach: 2,
            }],
        );
        assert!(is_lit(&map, &lit, 6, 2) && is_lit(&map, &lit, 8, 2));
    }
}
