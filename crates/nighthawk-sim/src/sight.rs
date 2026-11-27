//! What the player can see right now, and what they remember (decision
//! D49).
//!
//! After every command the world works out which tiles the player can see
//! and remembers each one as it is now: which structure it is, and the
//! kind of the top item lying there. A tile can be seen if it's in the
//! player's field of view ([`crate::fov::field_of_view`]) and either lit
//! ([`crate::light`], decision D50) or right beside the player: in the
//! dark you can still make out what's within arm's reach.
//! Tiles out of sight keep what was last seen, like CDDA's map memory: a
//! door you saw open stays open in your memory until you look again.
//!
//! What's remembered is game state (saved, and part of the state hash);
//! what's visible right now is worked out again from the map, so it isn't.

use serde::{Deserialize, Serialize};

use crate::fov::field_of_view;
use crate::hash::Fnv64;
use crate::light::{Light, lit_tiles};
use crate::map::{Map, Tile};
use crate::pos::Pos;

/// Marks "nothing" in the memory lists: a tile never seen, or no item
/// seen lying there. `u16::MAX` is never a real index (content lists are
/// checked to be shorter).
const NOTHING: u16 = u16::MAX;

/// What the player remembers about a tile they've seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Remembered {
    /// The structure it was, by number in the world's content.
    pub structure: u16,
    /// The kind of the top item lying there, by number, if there was one.
    pub item: Option<u16>,
}

/// What the player has seen, and can see.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct View {
    /// For each tile (in map order), the structure last seen there, or
    /// [`NOTHING`] if never seen.
    seen_structure: Vec<u16>,
    /// For each tile, the kind of the top item last seen lying there, or
    /// [`NOTHING`].
    seen_item: Vec<u16>,
    /// For each tile, whether it can be seen right now. Not saved: worked
    /// out again by [`View::refresh`] after loading.
    #[serde(skip)]
    visible: Vec<bool>,
    /// For each tile, whether it's lit right now (on the player's deck).
    /// Not saved either.
    #[serde(skip)]
    lit: Vec<bool>,
}

impl View {
    /// A view of a map of `tiles` tiles, with nothing seen yet.
    pub(crate) fn new(tiles: usize) -> View {
        View {
            seen_structure: vec![NOTHING; tiles],
            seen_item: vec![NOTHING; tiles],
            visible: vec![false; tiles],
            lit: vec![false; tiles],
        }
    }

    /// Works out what can be seen from `eye` on `map` by `lights`, and
    /// remembers it as it is now. `top_item` gives the kind of the first
    /// item lying at a position, if any.
    pub(crate) fn refresh(
        &mut self,
        map: &Map,
        eye: Pos,
        lights: &[Light],
        top_item: &dyn Fn(Pos) -> Option<u16>,
    ) {
        let mut in_view = Vec::new();
        let range = map.width().max(map.height());
        let blocks = |p: Pos| map.get(p).is_none_or(Tile::blocks_sight);
        field_of_view(eye, range, &blocks, &mut |p| in_view.push(p));
        self.lit = lit_tiles(map, lights);
        self.visible = vec![false; self.seen_structure.len()];
        for p in in_view {
            let (Some(i), Some(structure)) = (map.index_of(p), map.structure_index(p)) else {
                continue; // off the map: nothing there to see
            };
            let beside = eye.steps_to(p).is_some_and(|steps| steps <= 1);
            if !self.lit[i] && !beside {
                continue; // too dark to see from here
            }
            self.visible[i] = true;
            self.seen_structure[i] = structure;
            self.seen_item[i] = top_item(p).unwrap_or(NOTHING);
        }
    }

    /// Whether the tile at `p` can be seen right now.
    pub(crate) fn is_visible(&self, map: &Map, p: Pos) -> bool {
        map.index_of(p)
            .and_then(|i| self.visible.get(i))
            .copied()
            .unwrap_or(false)
    }

    /// Whether the tile at `p` is lit right now. Only tiles on the deck
    /// the view was last worked out for are ever lit.
    pub(crate) fn is_lit(&self, map: &Map, p: Pos) -> bool {
        map.index_of(p)
            .and_then(|i| self.lit.get(i))
            .copied()
            .unwrap_or(false)
    }

    /// What the player remembers of the tile at `p`, if they've ever seen
    /// it.
    pub(crate) fn remembered(&self, map: &Map, p: Pos) -> Option<Remembered> {
        let i = map.index_of(p)?;
        let structure = *self.seen_structure.get(i)?;
        let item = self.seen_item.get(i).copied().unwrap_or(NOTHING);
        (structure != NOTHING).then_some(Remembered {
            structure,
            item: (item != NOTHING).then_some(item),
        })
    }

    /// Checks a loaded view fits its world: one entry per tile, and every
    /// structure and item kind remembered is one that exists.
    pub(crate) fn check(
        &self,
        tiles: usize,
        structures: usize,
        kinds: usize,
    ) -> Result<(), String> {
        for (name, list) in [
            ("structures", &self.seen_structure),
            ("items", &self.seen_item),
        ] {
            if list.len() != tiles {
                return Err(format!(
                    "the map has {tiles} tiles but remembered {name} for {}",
                    list.len()
                ));
            }
        }
        let unknown = |list: &[u16], count: usize| {
            list.iter()
                .any(|&n| n != NOTHING && usize::from(n) >= count)
        };
        if unknown(&self.seen_structure, structures) || unknown(&self.seen_item, kinds) {
            return Err("a remembered tile names a structure or item that doesn't exist".into());
        }
        Ok(())
    }

    /// Feeds what's remembered into a state hash.
    pub(crate) fn hash_into(&self, h: &mut Fnv64) {
        h.u64(self.seen_structure.len() as u64);
        for (&structure, &item) in self.seen_structure.iter().zip(&self.seen_item) {
            h.u64(u64::from(structure)).u64(u64::from(item));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 7 × 3 room split by a wall at x = 3: `#.@#..#`, with walls all
    /// round.
    fn room() -> Map {
        let mut map = Map::filled(7, 3, 1, Tile::WALL);
        for x in [1, 2, 4, 5] {
            map.place(Pos::new(x, 1, 0), 1, Tile::FLOOR);
        }
        map
    }

    fn at(x: i32, y: i32) -> Pos {
        Pos::new(x, y, 0)
    }

    /// A light where the player stands at (2, 1), bright enough for any
    /// room here.
    const BRIGHT: [Light; 1] = [Light {
        at: Pos::new(2, 1, 0),
        reach: 20,
    }];

    #[test]
    fn what_can_be_seen_is_remembered_and_the_rest_is_not() {
        let map = room();
        let mut view = View::new(21);
        view.refresh(&map, at(2, 1), &BRIGHT, &|p| (p == at(1, 1)).then_some(7));
        assert!(view.is_visible(&map, at(1, 1)) && view.is_visible(&map, at(3, 1)));
        assert!(!view.is_visible(&map, at(4, 1)), "behind the wall");
        assert_eq!(
            view.remembered(&map, at(1, 1)),
            Some(Remembered {
                structure: 1,
                item: Some(7)
            })
        );
        assert_eq!(
            view.remembered(&map, at(3, 1)),
            Some(Remembered {
                structure: 0,
                item: None
            })
        );
        assert_eq!(view.remembered(&map, at(4, 1)), None, "never seen");
        assert_eq!(view.remembered(&map, at(9, 9)), None, "off the map");
        assert!(!view.is_visible(&map, at(9, 9)));
    }

    #[test]
    fn in_the_dark_you_see_only_what_is_beside_you() {
        let map = room();
        let mut view = View::new(21);
        view.refresh(&map, at(2, 1), &[], &|_| None);
        assert!(view.is_visible(&map, at(2, 1)), "where you stand");
        assert!(view.is_visible(&map, at(1, 1)) && view.is_visible(&map, at(3, 0)));
        assert!(
            !view.is_visible(&map, at(0, 1)),
            "in view, but two steps off"
        );
        assert_eq!(view.remembered(&map, at(0, 1)), None);
        assert!(!view.is_lit(&map, at(2, 1)));
    }

    #[test]
    fn lit_tiles_in_view_are_seen_from_afar() {
        let map = room();
        let mut view = View::new(21);
        // A small light at (1, 1) reaches the wall at (0, 1) but not the
        // corner at (0, 0); one behind the dividing wall lights its own
        // side only.
        let lights = [
            Light {
                at: at(1, 1),
                reach: 1,
            },
            Light {
                at: at(5, 1),
                reach: 1,
            },
        ];
        view.refresh(&map, at(2, 1), &lights, &|_| None);
        assert!(view.is_lit(&map, at(0, 1)) && view.is_visible(&map, at(0, 1)));
        assert!(!view.is_lit(&map, at(0, 0)) && !view.is_visible(&map, at(0, 0)));
        assert!(view.is_lit(&map, at(4, 1)));
        assert!(!view.is_visible(&map, at(4, 1)), "lit, but behind the wall");
        assert!(!view.is_lit(&map, at(9, 9)), "off the map");
    }

    #[test]
    fn a_window_in_the_wall_shows_the_room_beyond() {
        let mut map = room();
        map.place(at(3, 1), 6, Tile::WINDOW);
        let mut view = View::new(21);
        view.refresh(&map, at(2, 1), &BRIGHT, &|_| None);
        assert!(view.is_visible(&map, at(4, 1)) && view.is_visible(&map, at(5, 1)));
        assert_eq!(view.remembered(&map, at(3, 1)).unwrap().structure, 6);
    }

    #[test]
    fn memory_keeps_what_was_seen_after_it_changes_out_of_sight() {
        let mut map = room();
        let mut view = View::new(21);
        view.refresh(&map, at(2, 1), &BRIGHT, &|_| None);
        // Seen as floor; then it changes while the player looks away.
        map.place(at(1, 1), 5, Tile::WALL);
        view.visible = vec![false; 21];
        assert_eq!(view.remembered(&map, at(1, 1)).unwrap().structure, 1);
        // Looking again updates it.
        view.refresh(&map, at(2, 1), &BRIGHT, &|_| None);
        assert_eq!(view.remembered(&map, at(1, 1)).unwrap().structure, 5);
    }

    #[test]
    fn a_loaded_view_must_fit_its_world() {
        let mut view = View::new(21);
        assert_eq!(view.check(21, 2, 3), Ok(()));
        assert_eq!(
            view.check(20, 2, 3),
            Err("the map has 20 tiles but remembered structures for 21".into())
        );
        view.seen_item.pop();
        assert!(
            view.check(21, 2, 3)
                .unwrap_err()
                .contains("remembered items for 20")
        );
        let mut view = View::new(21);
        view.seen_structure[0] = 2;
        assert!(view.check(21, 2, 3).unwrap_err().contains("doesn't exist"));
        view.seen_structure[0] = 1;
        view.seen_item[0] = 3;
        assert!(view.check(21, 2, 3).is_err());
    }

    #[test]
    fn what_is_remembered_is_part_of_the_hash() {
        let hash = |v: &View| {
            let mut h = Fnv64::new();
            v.hash_into(&mut h);
            h.finish()
        };
        let blank = View::new(21);
        let mut seen = View::new(21);
        seen.refresh(&room(), at(2, 1), &BRIGHT, &|_| None);
        assert_ne!(hash(&blank), hash(&seen));
        let mut only_visible = View::new(21);
        only_visible.visible[0] = true;
        assert_eq!(
            hash(&blank),
            hash(&only_visible),
            "what's visible isn't state"
        );
    }
}
