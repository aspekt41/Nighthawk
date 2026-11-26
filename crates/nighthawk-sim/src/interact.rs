//! What the player can do to a tile, and why not (decision D36).
//!
//! The interact menu points a cursor at a tile and lists the actions that
//! make sense there. Each action has a *reach*: opening or closing a door
//! needs you right beside it, while a laser cutter works from 2 tiles away
//! along a clear line (you, the gap, the wall). An action that can't be done
//! is still listed, with the reason ([`Blocked`]), so the player can see
//! what stands in the way.
//!
//! These are rules, so they live in the sim: the front-end asks
//! [`options_at`] what to show, and [`crate::World::apply`] checks the same
//! rules again before doing anything.

use crate::map::Tile;
use crate::material::Quality;
use crate::pos::Pos;
use crate::recipe::RecipeAction;
use crate::world::World;

/// How far you can reach to open or close a door: the tiles right around
/// you, diagonals included.
pub const HAND_REACH: u32 = 1;

/// How far a laser cutter reaches: 2 tiles, so standing at (1, 1) it can
/// cut a wall at (1, 3) across the open tile at (1, 2).
pub const LASER_REACH: u32 = 2;

/// Something the player can do to a tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Open,
    Close,
    /// Cut through it with a tool.
    Cut,
    /// Pick up the items lying there.
    PickUp,
}

impl Action {
    /// Every action, in the order the menu lists them.
    pub const ALL: [Action; 4] = [Action::Open, Action::Close, Action::Cut, Action::PickUp];

    /// How far away the action works, in steps (diagonals count as one).
    pub fn reach(self) -> u32 {
        match self {
            Action::Open | Action::Close | Action::PickUp => HAND_REACH,
            Action::Cut => LASER_REACH,
        }
    }

    /// Whether the action makes sense at `at` at all: doors open and
    /// close; things with a cut recipe can be cut (decision D40); items
    /// lying there can be picked up.
    fn applies_at(self, world: &World, at: Pos) -> bool {
        let Some(tile) = world.map().get(at) else {
            return false;
        };
        match self {
            Action::Open => tile == Tile::DoorClosed,
            Action::Close => tile == Tile::DoorOpen,
            // Something solid, with a recipe saying what it becomes.
            Action::Cut => !tile.holds_air() && world.recipe_for(RecipeAction::Cut, at).is_some(),
            Action::PickUp => world.items_at(at).next().is_some(),
        }
    }
}

/// Why an action can't be done right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    /// The tile is further than the action's reach, or on another deck.
    TooFar { reach: u32 },
    /// Something solid is between you and the tile.
    InTheWay { at: Pos },
    /// You can't close a door you're standing in.
    StandingInIt,
    /// It needs a tool quality you don't have.
    Needs { quality: Quality, level: u8 },
    /// Nothing can cut this material.
    Uncuttable,
    /// The action doesn't apply to this tile (opening a wall, say).
    NotPossible,
}

/// One line of the interact menu: an action, and why it can't be done, if
/// it can't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionOption {
    pub action: Action,
    pub blocked: Option<Blocked>,
}

/// The actions that make sense on the tile at `at`, each with what blocks
/// it. Empty for tiles with nothing to do, like open floor.
pub fn options_at(world: &World, at: Pos) -> Vec<ActionOption> {
    Action::ALL
        .into_iter()
        .filter(|action| action.applies_at(world, at))
        .map(|action| ActionOption {
            action,
            blocked: blocked(world, at, action),
        })
        .collect()
}

/// What stops the player doing `action` to the tile at `at` right now, or
/// `None` if nothing does.
pub fn blocked(world: &World, at: Pos, action: Action) -> Option<Blocked> {
    if !action.applies_at(world, at) {
        return Some(Blocked::NotPossible);
    }
    match action {
        Action::Cut => cut_blocked(world, at),
        Action::Open | Action::Close => hand_blocked(world, at),
        // Picking up from the tile you stand on is fine.
        Action::PickUp => too_far(world, at, HAND_REACH),
    }
}

/// Doors: you must be beside the door, and not standing in it.
fn hand_blocked(world: &World, at: Pos) -> Option<Blocked> {
    if at == world.player() {
        return Some(Blocked::StandingInIt);
    }
    too_far(world, at, HAND_REACH)
}

/// Cutting: the material must be cuttable, within the laser's reach along a
/// clear line, and you need the tool.
fn cut_blocked(world: &World, at: Pos) -> Option<Blocked> {
    let material = world.material_at(at)?;
    let Some((&quality, &level)) = material.cut_by.iter().next() else {
        return Some(Blocked::Uncuttable);
    };
    let have = world.tool_level(quality);
    too_far(world, at, LASER_REACH)
        .or_else(|| in_the_way(world, at))
        .or_else(|| (have < level).then_some(Blocked::Needs { quality, level }))
}

/// `TooFar` if `at` is more than `reach` steps from the player, or on
/// another deck.
fn too_far(world: &World, at: Pos, reach: u32) -> Option<Blocked> {
    let near = world.player().steps_to(at).is_some_and(|d| d <= reach);
    (!near).then_some(Blocked::TooFar { reach })
}

/// `InTheWay` naming the first solid tile on the straight line from the
/// player to `at`, if there is one.
fn in_the_way(world: &World, at: Pos) -> Option<Blocked> {
    let map = world.map();
    world
        .player()
        .line_to(at)
        .into_iter()
        .find(|&p| !map.get(p).is_some_and(Tile::holds_air))
        .map(|p| Blocked::InTheWay { at: p })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map_file::parse_map_for_tests as parse_map;

    /// A test room. `@` at (1, 1); a closed door `+` at (1, 3) two tiles
    /// south; an open door `'` at (2, 1) beside the player; a hard-alloy
    /// wall `H` at (5, 1); a two-tile pillar `##` at (3, 2) and (4, 2).
    ///
    /// ```text
    /// #######
    /// #@'..H#
    /// #..##.#
    /// #+....#
    /// #######
    /// ```
    fn world() -> World {
        let rows = ["#######", "#@'..H#", "#..##.#", "#+....#", "#######"];
        let json = format!(
            r##"{{ "id": "t", "name": "T",
                 "legend": {{ "#": "wall", ".": "floor", "+": "door",
                             "'": {{ "structure": "door", "open": true }},
                             "H": {{ "structure": "wall", "material": "hard_alloy" }} }},
                 "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        World::new(parse_map(&json).unwrap(), 1)
    }

    fn only(world: &World, at: Pos) -> Vec<ActionOption> {
        options_at(world, at)
    }

    #[test]
    fn open_floor_has_nothing_to_do_and_off_the_map_has_nothing_either() {
        let w = world();
        assert_eq!(only(&w, Pos::new(2, 2, 0)), vec![]);
        assert_eq!(only(&w, Pos::new(-1, 0, 0)), vec![]);
    }

    #[test]
    fn a_closed_door_can_be_opened_from_beside_it_and_cut_with_a_tool() {
        let w = world();
        // The door at (1, 3) is two steps away: too far to open by hand,
        // close enough for a laser across the clear tile at (1, 2).
        assert_eq!(
            only(&w, Pos::new(1, 3, 0)),
            vec![
                ActionOption {
                    action: Action::Open,
                    blocked: Some(Blocked::TooFar { reach: 1 })
                },
                ActionOption {
                    action: Action::Cut,
                    blocked: Some(Blocked::Needs {
                        quality: Quality::Laser,
                        level: 2
                    })
                },
            ]
        );
    }

    #[test]
    fn an_open_door_beside_you_can_be_closed_but_not_while_you_stand_in_it() {
        let mut w = world();
        let door = Pos::new(2, 1, 0);
        assert_eq!(
            only(&w, door),
            vec![ActionOption {
                action: Action::Close,
                blocked: None
            }]
        );
        w.apply(crate::Command::Move(crate::Dir::East));
        assert_eq!(
            blocked(&w, door, Action::Close),
            Some(Blocked::StandingInIt)
        );
    }

    #[test]
    fn diagonal_neighbours_are_within_reach() {
        let mut w = world();
        w.apply(crate::Command::Move(crate::Dir::South)); // to (1, 2)
        // The open door at (2, 1) is diagonal from (1, 2).
        assert_eq!(blocked(&w, Pos::new(2, 1, 0), Action::Close), None);
    }

    #[test]
    fn walls_list_only_cutting_with_the_level_their_material_needs() {
        let w = world();
        let hard = only(&w, Pos::new(5, 1, 0));
        assert_eq!(hard.len(), 1);
        assert_eq!(hard[0].action, Action::Cut);
        assert_eq!(hard[0].blocked, Some(Blocked::TooFar { reach: 2 }));
    }

    #[test]
    fn something_solid_on_the_line_blocks_the_laser() {
        // From (2, 2), the far half of the pillar at (4, 2) is two steps
        // east, but the near half at (3, 2) is in the way.
        let mut w = world();
        w.apply(crate::Command::Move(crate::Dir::South));
        w.apply(crate::Command::Move(crate::Dir::East));
        assert_eq!(w.player(), Pos::new(2, 2, 0));
        assert_eq!(
            blocked(&w, Pos::new(4, 2, 0), Action::Cut),
            Some(Blocked::InTheWay {
                at: Pos::new(3, 2, 0)
            })
        );
        // The near half itself is right beside you: only the tool is
        // missing.
        assert_eq!(
            blocked(&w, Pos::new(3, 2, 0), Action::Cut),
            Some(Blocked::Needs {
                quality: Quality::Laser,
                level: 2
            })
        );
    }

    #[test]
    fn a_material_nothing_cuts_says_so() {
        let mut w = world();
        let wall = Pos::new(0, 1, 0);
        w.materials_mut()[0].cut_by.clear();
        assert_eq!(blocked(&w, wall, Action::Cut), Some(Blocked::Uncuttable));
    }

    #[test]
    fn another_deck_is_always_too_far() {
        let w = world();
        let elsewhere = Pos::new(1, 2, 1);
        assert_eq!(
            too_far(&w, elsewhere, 99),
            Some(Blocked::TooFar { reach: 99 })
        );
    }

    #[test]
    fn actions_that_do_not_apply_are_not_possible() {
        let w = world();
        assert_eq!(
            blocked(&w, Pos::new(0, 0, 0), Action::Open),
            Some(Blocked::NotPossible)
        );
        assert_eq!(
            blocked(&w, Pos::new(9, 9, 9), Action::Cut),
            Some(Blocked::NotPossible)
        );
    }

    #[test]
    fn reaches_are_the_agreed_ones() {
        assert_eq!(Action::Open.reach(), 1);
        assert_eq!(Action::Close.reach(), 1);
        assert_eq!(Action::Cut.reach(), 2);
    }

    #[test]
    fn items_on_a_tile_within_reach_can_be_picked_up() {
        let rows = ["####", "#@s#", "#..#", "####"];
        let json = format!(
            r##"{{ "id": "t", "name": "T",
                 "legend": {{ "#": "wall", ".": "floor",
                    "s": {{ "structure": "floor", "items": [ {{ "item": "scrap" }} ] }} }},
                 "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        let w = World::new(parse_map(&json).unwrap(), 1);
        assert_eq!(
            only(&w, Pos::new(2, 1, 0)),
            vec![ActionOption {
                action: Action::PickUp,
                blocked: None
            }]
        );
        assert_eq!(only(&w, Pos::new(1, 2, 0)), vec![], "no items there");
        assert_eq!(Action::PickUp.reach(), 1);
    }

    #[test]
    fn only_solid_things_with_a_cut_recipe_offer_cutting() {
        let w = world();
        // The test recipes cut doors and walls; the open door at (2, 1)
        // isn't solid, and floor has no recipe.
        let cut_offered = |at| options_at(&w, at).iter().any(|o| o.action == Action::Cut);
        assert!(cut_offered(Pos::new(1, 3, 0)), "a shut door");
        assert!(cut_offered(Pos::new(0, 1, 0)), "a wall");
        assert!(!cut_offered(Pos::new(2, 1, 0)), "an open door");
        assert!(!cut_offered(Pos::new(2, 2, 0)), "floor");
    }

    #[test]
    fn with_the_right_tool_in_reach_nothing_blocks_the_cut() {
        // The steel door two tiles south needs LASER 2.
        let w = with_laser(world(), 2);
        assert_eq!(blocked(&w, Pos::new(1, 3, 0), Action::Cut), None);
    }

    /// The world, wearing a tool with `LASER level`. The tool is set up by
    /// editing the world's saved form, since its fields are private to the
    /// `world` module.
    fn with_laser(world: World, level: u8) -> World {
        let mut json: serde_json::Value = serde_json::to_value(&world).unwrap();
        json["item_types"][1]["qualities"]["LASER"] = level.into();
        json["item_types"][1]["wear"] = serde_json::json!({ "pockets": [] });
        json["worn"] = serde_json::json!([ { "kind": 1, "material": 0, "pockets": [] } ]);
        serde_json::from_value(json).unwrap()
    }
}
