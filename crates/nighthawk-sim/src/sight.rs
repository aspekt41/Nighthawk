//! What the player can see right now, and what they remember (decision
//! D49).
//!
//! After every command the world works out which tiles the player can see
//! and remembers each one as it is now: which structure it is, the kind of
//! furniture standing there, and the kind of the top item lying there. A tile can be seen if it's in the
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
use crate::map::Map;
use crate::pos::Pos;

/// Marks "nothing" in the memory lists: a tile never seen, or no
/// furniture or item seen there. `u16::MAX` is never a real index (content lists are
/// checked to be shorter).
const NOTHING: u16 = u16::MAX;

/// What the player remembers about a tile they've seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Remembered {
    /// The structure it was, by number in the world's content.
    pub structure: u16,
    /// The kind of furniture standing there, by number, if there was any.
    pub furniture: Option<u16>,
    /// The kind of the top item lying there, by number, if there was one.
    pub item: Option<u16>,
}

/// What there is to see, for [`View::refresh`].
pub(crate) struct Scene<'a> {
    pub(crate) map: &'a Map,
    /// Whether something at a position stops sight (and light): its
    /// structure, or furniture standing there.
    pub(crate) blocks: &'a dyn Fn(Pos) -> bool,
    pub(crate) lights: &'a [Light],
    /// The kind of furniture standing at a position, if any.
    pub(crate) furniture: &'a dyn Fn(Pos) -> Option<u16>,
    /// The kind of the top item lying at a position, if any.
    pub(crate) top_item: &'a dyn Fn(Pos) -> Option<u16>,
}

/// What the player has seen, and can see.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct View {
    /// For each tile (in map order), the structure last seen there, or
    /// [`NOTHING`] if never seen.
    seen_structure: Vec<u16>,
    /// For each tile, the kind of furniture last seen there, or
    /// [`NOTHING`].
    seen_furniture: Vec<u16>,
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
            seen_furniture: vec![NOTHING; tiles],
            seen_item: vec![NOTHING; tiles],
            visible: vec![false; tiles],
            lit: vec![false; tiles],
        }
    }

    /// Works out what can be seen of `scene` from `eye`, and remembers it
    /// as it is now.
    pub(crate) fn refresh(&mut self, scene: &Scene, eye: Pos) {
        let map = scene.map;
        let mut in_view = Vec::new();
        let range = map.width().max(map.height());
        field_of_view(eye, range, scene.blocks, &mut |p| in_view.push(p));
        self.lit = lit_tiles(map, scene.blocks, scene.lights);
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
            self.seen_furniture[i] = (scene.furniture)(p).unwrap_or(NOTHING);
            self.seen_item[i] = (scene.top_item)(p).unwrap_or(NOTHING);
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
        // `NOTHING` becomes `None`; anything else, `Some` of it.
        let known = |list: &[u16]| list.get(i).copied().filter(|&n| n != NOTHING);
        (structure != NOTHING).then_some(Remembered {
            structure,
            furniture: known(&self.seen_furniture),
            item: known(&self.seen_item),
        })
    }

    /// Checks a loaded view fits its world: one entry per tile, and every
    /// structure, kind of furniture and kind of item remembered is one
    /// that exists. `counts` is how many of each there are, in that order.
    pub(crate) fn check(&self, tiles: usize, counts: [usize; 3]) -> Result<(), String> {
        let lists = [
            ("structures", &self.seen_structure),
            ("furniture", &self.seen_furniture),
            ("items", &self.seen_item),
        ];
        for ((name, list), count) in lists.into_iter().zip(counts) {
            if list.len() != tiles {
                return Err(format!(
                    "the map has {tiles} tiles but remembered {name} for {}",
                    list.len()
                ));
            }
            if list
                .iter()
                .any(|&n| n != NOTHING && usize::from(n) >= count)
            {
                return Err(format!("a remembered tile names {name} that doesn't exist"));
            }
        }
        Ok(())
    }

    /// Feeds what's remembered into a state hash.
    pub(crate) fn hash_into(&self, h: &mut Fnv64) {
        h.u64(self.seen_structure.len() as u64);
        let furniture_and_items = self.seen_furniture.iter().zip(&self.seen_item);
        for (&structure, (&furniture, &item)) in self.seen_structure.iter().zip(furniture_and_items)
        {
            h.u64(u64::from(structure))
                .u64(u64::from(furniture))
                .u64(u64::from(item));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Tile;

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

    /// Looks at `map` from (2, 1) by `lights`, with only walls in the way,
    /// no furniture, and `top_item` lying about.
    fn look(view: &mut View, map: &Map, lights: &[Light], top_item: &dyn Fn(Pos) -> Option<u16>) {
        let blocks = |p: Pos| map.get(p).is_none_or(Tile::blocks_sight);
        let scene = Scene {
            map,
            blocks: &blocks,
            lights,
            furniture: &|_| None,
            top_item,
        };
        view.refresh(&scene, at(2, 1));
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
        look(&mut view, &map, &BRIGHT, &|p| (p == at(1, 1)).then_some(7));
        assert!(view.is_visible(&map, at(1, 1)) && view.is_visible(&map, at(3, 1)));
        assert!(!view.is_visible(&map, at(4, 1)), "behind the wall");
        assert_eq!(
            view.remembered(&map, at(1, 1)),
            Some(Remembered {
                structure: 1,
                furniture: None,
                item: Some(7)
            })
        );
        assert_eq!(
            view.remembered(&map, at(3, 1)),
            Some(Remembered {
                structure: 0,
                furniture: None,
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
        look(&mut view, &map, &[], &|_| None);
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
        look(&mut view, &map, &lights, &|_| None);
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
        look(&mut view, &map, &BRIGHT, &|_| None);
        assert!(view.is_visible(&map, at(4, 1)) && view.is_visible(&map, at(5, 1)));
        assert_eq!(view.remembered(&map, at(3, 1)).unwrap().structure, 6);
    }

    #[test]
    fn memory_keeps_what_was_seen_after_it_changes_out_of_sight() {
        let mut map = room();
        let mut view = View::new(21);
        look(&mut view, &map, &BRIGHT, &|_| None);
        // Seen as floor; then it changes while the player looks away.
        map.place(at(1, 1), 5, Tile::WALL);
        view.visible = vec![false; 21];
        assert_eq!(view.remembered(&map, at(1, 1)).unwrap().structure, 1);
        // Looking again updates it.
        look(&mut view, &map, &BRIGHT, &|_| None);
        assert_eq!(view.remembered(&map, at(1, 1)).unwrap().structure, 5);
    }

    #[test]
    fn a_loaded_view_must_fit_its_world() {
        // 2 structures, 1 kind of furniture, 3 kinds of item.
        let counts = [2, 1, 3];
        let mut view = View::new(21);
        assert_eq!(view.check(21, counts), Ok(()));
        assert_eq!(
            view.check(20, counts),
            Err("the map has 20 tiles but remembered structures for 21".into())
        );
        view.seen_item.pop();
        assert_eq!(
            view.check(21, counts),
            Err("the map has 21 tiles but remembered items for 20".into())
        );
        let known = |change: fn(&mut View)| {
            let mut view = View::new(21);
            change(&mut view);
            view.check(21, counts)
        };
        assert_eq!(known(|v| v.seen_structure[0] = 1), Ok(()));
        let doesnt_exist =
            |what: &str| Err(format!("a remembered tile names {what} that doesn't exist"));
        assert_eq!(
            known(|v| v.seen_structure[0] = 2),
            doesnt_exist("structures")
        );
        assert_eq!(
            known(|v| v.seen_furniture[0] = 1),
            doesnt_exist("furniture")
        );
        assert_eq!(known(|v| v.seen_item[0] = 3), doesnt_exist("items"));
    }

    #[test]
    fn furniture_seen_is_remembered_and_can_hide_what_is_behind_it() {
        // A cabinet (kind 4) at (4, 1) in a room 7 tiles long, seen from
        // (2, 1): it's seen, and remembered; the tile behind it isn't.
        let mut map = Map::filled(7, 3, 1, Tile::WALL);
        for x in 1..6 {
            map.place(Pos::new(x, 1, 0), 1, Tile::FLOOR);
        }
        let cabinet = at(4, 1);
        let blocks = |p: Pos| p == cabinet || map.get(p).is_none_or(Tile::blocks_sight);
        let scene = Scene {
            map: &map,
            blocks: &blocks,
            lights: &BRIGHT,
            furniture: &|p| (p == cabinet).then_some(4),
            top_item: &|_| None,
        };
        let mut view = View::new(21);
        view.refresh(&scene, at(2, 1));
        assert_eq!(view.remembered(&map, cabinet).unwrap().furniture, Some(4));
        assert_eq!(view.remembered(&map, at(3, 1)).unwrap().furniture, None);
        assert!(!view.is_visible(&map, at(5, 1)), "hidden behind it");
    }

    #[test]
    fn what_is_remembered_is_part_of_the_hash() {
        let hash = |v: &View| {
            let mut h = Fnv64::new();
            v.hash_into(&mut h);
            h.finish()
        };
        let blank = View::new(21);
        let mut furnished = View::new(21);
        furnished.seen_furniture[0] = 0;
        assert_ne!(hash(&blank), hash(&furnished), "furniture remembered");
        let mut seen = View::new(21);
        look(&mut seen, &room(), &BRIGHT, &|_| None);
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
