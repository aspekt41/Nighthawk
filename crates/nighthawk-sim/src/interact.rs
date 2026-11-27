//! What the player can do to a tile, and why not (decisions D36 and D43).
//!
//! The interact menu points a cursor at a tile and lists the actions that
//! make sense there. Actions are content (`data/actions/`, see `action.rs`):
//! each says how far it reaches (opening a door needs you right beside it; a
//! laser cutter works from 2 tiles away along a clear line), whether the
//! tile must be clear of items, and whether a tool is needed. An action that
//! can't be done is still listed, with the reason ([`Blocked`]), so the
//! player can see what stands in the way.
//!
//! These are rules, so they live in the sim: the front-end asks
//! [`options_at`] what to show, and [`crate::World::apply`] checks the same
//! rules again before doing anything. Actions are referred to by their
//! number: their place in the world's list of actions.

use crate::action::{Action, Effect};
use crate::map::Tile;
use crate::pos::Pos;
use crate::world::World;

/// Why an action can't be done right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    /// The tile is further than the action's reach, or on another deck.
    TooFar { reach: u32 },
    /// Something solid is between you and the tile.
    InTheWay { at: Pos },
    /// It can't be done to the tile you're standing on.
    StandingInIt,
    /// It needs a tool with quality number `quality` (in the world's
    /// content) at `level`, and you have nothing that good.
    Needs { quality: u16, level: u8 },
    /// It needs a tool at the level the material lists for it, and this
    /// material lists none: nothing you could carry would do it.
    Unworkable,
    /// The action doesn't apply to this tile (opening a wall, say).
    NotPossible,
    /// You don't carry enough of an item the recipe uses: `need` of item
    /// type number `item`, and you have `have`.
    Missing { item: u16, need: u32, have: u32 },
    /// Items are lying on it, and would end up inside it.
    Cluttered,
}

/// One line of the interact menu: an action (its number), and why it can't
/// be done, if it can't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionOption {
    pub action: u16,
    pub blocked: Option<Blocked>,
}

/// The actions that make sense on the tile at `at`, in menu order, each
/// with what blocks it. Empty for tiles with nothing to do, like open
/// floor.
pub fn options_at(world: &World, at: Pos) -> Vec<ActionOption> {
    // `(0..).zip(list)` numbers the actions as it goes: 0, 1, 2…
    (0..)
        .zip(world.content().actions())
        .filter(|(_, action)| applies(world, at, action))
        .map(|(number, action)| ActionOption {
            action: number,
            blocked: rules(world, at, action),
        })
        .collect()
}

/// What stops the player doing action number `action` to the tile at
/// `at` right now, or `None` if nothing does.
pub fn blocked(world: &World, at: Pos, action: u16) -> Option<Blocked> {
    match world.content().actions().get(usize::from(action)) {
        Some(action) if applies(world, at, action) => rules(world, at, action),
        _ => Some(Blocked::NotPossible),
    }
}

/// Whether `action` makes sense at `at` at all: there's a recipe for it
/// (opening a door is one), or there are items to pick up.
fn applies(world: &World, at: Pos, action: &Action) -> bool {
    if !world.map().in_bounds(at) {
        return false;
    }
    match action.effect {
        Effect::Recipe => world.recipe_for(&action.id, at).is_some(),
        Effect::PickUp => world.items_at(at).next().is_some(),
    }
}

/// What stops an action that applies here, checked in a fixed order, so
/// the reason given is the first thing to fix: whether anything could do
/// it at all, where you stand, how far it is, what's in the way, what's
/// lying on it, then the tools and items you carry.
fn rules(world: &World, at: Pos, action: &Action) -> Option<Blocked> {
    unworkable(world, at, action)
        .or_else(|| standing_in_it(world, at, action))
        .or_else(|| too_far(world, at, action.reach))
        .or_else(|| blocked_line(world, at, action))
        .or_else(|| cluttered(world, at, action))
        .or_else(|| tool_needed(world, at, action))
        .or_else(|| missing(world, at, action))
}

/// `Unworkable` for an action that needs a tool at the material's level,
/// on a material that lists none for it.
fn unworkable(world: &World, at: Pos, action: &Action) -> Option<Blocked> {
    if !action.tool_from_material {
        return None;
    }
    let material = world.material_at(at)?;
    (!material.worked_by.contains_key(&action.id)).then_some(Blocked::Unworkable)
}

/// `StandingInIt` for an action that can't be done to your own tile.
fn standing_in_it(world: &World, at: Pos, action: &Action) -> Option<Blocked> {
    (!action.on_own_tile && at == world.player()).then_some(Blocked::StandingInIt)
}

/// `TooFar` if `at` is more than `reach` steps from the player, or on
/// another deck.
fn too_far(world: &World, at: Pos, reach: u32) -> Option<Blocked> {
    let near = world.player().steps_to(at).is_some_and(|d| d <= reach);
    (!near).then_some(Blocked::TooFar { reach })
}

/// `InTheWay` for an action that needs a clear line, if something solid
/// is on it.
fn blocked_line(world: &World, at: Pos, action: &Action) -> Option<Blocked> {
    action.clear_line.then(|| in_the_way(world, at)).flatten()
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

/// `Cluttered` for an action that needs the tile clear, if items lie
/// there.
fn cluttered(world: &World, at: Pos, action: &Action) -> Option<Blocked> {
    let lying = world.items_at(at).next().is_some();
    (action.clear_of_items && lying).then_some(Blocked::Cluttered)
}

/// `Needs` for the first tool level the player is short of: what the
/// material asks for this action (any one of its levels is enough), then
/// what the recipe itself needs (all of them).
fn tool_needed(world: &World, at: Pos, action: &Action) -> Option<Blocked> {
    material_tool(world, at, action).or_else(|| recipe_tools(world, at, action))
}

/// What the material asks: met if any one of its levels for this action
/// is; if none is, the first is the one asked for.
fn material_tool(world: &World, at: Pos, action: &Action) -> Option<Blocked> {
    if !action.tool_from_material {
        return None;
    }
    let levels = world.material_at(at)?.worked_by.get(&action.id)?;
    if levels
        .iter()
        .any(|(q, &level)| world.tool_level(q) >= level)
    {
        return None;
    }
    let (quality, &level) = levels.iter().next()?;
    Some(needs(world, quality, level))
}

/// What the recipe asks: every level it lists.
fn recipe_tools(world: &World, at: Pos, action: &Action) -> Option<Blocked> {
    let recipe = world.recipe_for(&action.id, at)?;
    recipe
        .needs
        .iter()
        .find(|&(quality, &level)| world.tool_level(quality) < level)
        .map(|(quality, &level)| needs(world, quality, level))
}

/// `Needs` for `quality` (an id, checked on load) at `level`.
fn needs(world: &World, quality: &str, level: u8) -> Blocked {
    let quality = world
        .content()
        .quality_index(quality)
        .expect("content names real qualities");
    Blocked::Needs { quality, level }
}

/// The first item the recipe for `action` at `at` uses that you don't
/// carry enough of.
fn missing(world: &World, at: Pos, action: &Action) -> Option<Blocked> {
    let recipe = world.recipe_for(&action.id, at)?;
    recipe.uses.iter().find_map(|used| {
        let item = world.item_type_index(&used.item)?;
        let have = world.carried_count(item);
        (have < used.count).then_some(Blocked::Missing {
            item,
            need: used.count,
            have,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::tests::{CLOSE, CUT, OPEN, PICK_UP, REPAIR, WELD};
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
                             "'": "open_door",
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
                    action: OPEN,
                    blocked: Some(Blocked::TooFar { reach: 1 })
                },
                ActionOption {
                    action: CUT,
                    blocked: Some(Blocked::Needs {
                        quality: 0, // LASER, the first test quality
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
        let close = ActionOption {
            action: CLOSE,
            blocked: None,
        };
        assert_eq!(only(&w, door)[0], close, "closing first, then cutting");
        w.apply(crate::Command::Move(crate::Dir::East));
        assert_eq!(blocked(&w, door, CLOSE), Some(Blocked::StandingInIt));
    }

    #[test]
    fn diagonal_neighbours_are_within_reach() {
        let mut w = world();
        w.apply(crate::Command::Move(crate::Dir::South)); // to (1, 2)
        // The open door at (2, 1) is diagonal from (1, 2).
        assert_eq!(blocked(&w, Pos::new(2, 1, 0), CLOSE), None);
    }

    #[test]
    fn walls_list_only_cutting_with_the_level_their_material_needs() {
        let w = world();
        let hard = only(&w, Pos::new(5, 1, 0));
        assert_eq!(hard.len(), 1);
        assert_eq!(hard[0].action, CUT);
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
            blocked(&w, Pos::new(4, 2, 0), CUT),
            Some(Blocked::InTheWay {
                at: Pos::new(3, 2, 0)
            })
        );
        // The near half itself is right beside you: only the tool is
        // missing.
        assert_eq!(
            blocked(&w, Pos::new(3, 2, 0), CUT),
            Some(Blocked::Needs {
                quality: 0, // LASER, the first test quality
                level: 2
            })
        );
    }

    #[test]
    fn a_material_nothing_cuts_says_so() {
        let mut w = world();
        let wall = Pos::new(0, 1, 0);
        w.materials_mut()[0].worked_by.clear();
        assert_eq!(blocked(&w, wall, CUT), Some(Blocked::Unworkable));
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
            blocked(&w, Pos::new(0, 0, 0), OPEN),
            Some(Blocked::NotPossible)
        );
        assert_eq!(
            blocked(&w, Pos::new(9, 9, 9), CUT),
            Some(Blocked::NotPossible)
        );
        assert_eq!(
            blocked(&w, Pos::new(1, 3, 0), 999),
            Some(Blocked::NotPossible),
            "an action number that isn't in the list"
        );
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
                action: PICK_UP,
                blocked: None
            }]
        );
        assert_eq!(only(&w, Pos::new(1, 2, 0)), vec![], "no items there");
    }

    #[test]
    fn cutting_is_offered_wherever_there_is_a_cut_recipe() {
        let w = world();
        // The test recipes cut doors and walls; floor and open doors (their
        // own structure) have none.
        let cut_offered = |at| options_at(&w, at).iter().any(|o| o.action == CUT);
        assert!(cut_offered(Pos::new(1, 3, 0)), "a shut door");
        assert!(cut_offered(Pos::new(0, 1, 0)), "a wall");
        assert!(!cut_offered(Pos::new(2, 1, 0)), "an open door");
        assert!(!cut_offered(Pos::new(2, 2, 0)), "floor");
    }

    /// A room with a hull crack `x` north of the player and a pile of
    /// scrap `s` to the east:
    ///
    /// ```text
    /// #x##
    /// #@s#
    /// ####
    /// ```
    fn crack_world() -> World {
        let rows = ["#x##", "#@s#", "####"];
        let json = format!(
            r##"{{ "id": "t", "name": "T",
                 "legend": {{ "#": "wall", "x": "hull_crack",
                    "s": {{ "structure": "floor", "items": [ {{ "item": "scrap" }} ] }} }},
                 "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        World::new(parse_map(&json).unwrap(), 1)
    }

    #[test]
    fn a_recipe_can_need_tool_levels_of_its_own() {
        // Welding the crack needs WELD 2 (the test recipe), whatever it's
        // made of.
        let crack = Pos::new(1, 0, 0);
        let weld = Blocked::Needs {
            quality: 1, // WELD, the second test quality
            level: 2,
        };
        assert_eq!(blocked(&crack_world(), crack, WELD), Some(weld));
        let welder = with_tool(crack_world(), "WELD", 2);
        assert_eq!(blocked(&welder, crack, WELD), None);
        let weak = with_tool(crack_world(), "WELD", 1);
        assert_eq!(blocked(&weak, crack, WELD), Some(weld));
    }

    #[test]
    fn any_one_level_the_material_lists_is_enough() {
        // Steel walls take LASER 2 to cut; let WELD 1 do it too.
        let mut w = with_tool(world(), "WELD", 1);
        let wall = Pos::new(0, 1, 0);
        let either = [("LASER".to_string(), 2), ("WELD".to_string(), 1)].into();
        w.materials_mut()[0].worked_by.insert("cut".into(), either);
        assert_eq!(blocked(&w, wall, CUT), None);
        let both_high = [("LASER".to_string(), 2), ("WELD".to_string(), 3)].into();
        w.materials_mut()[0]
            .worked_by
            .insert("cut".into(), both_high);
        assert_eq!(
            blocked(&w, wall, CUT),
            Some(Blocked::Needs {
                quality: 0,
                level: 2
            }),
            "with none met, the first is asked for"
        );
    }

    #[test]
    fn items_lying_on_a_tile_block_actions_that_need_it_clear() {
        // Repairing doesn't apply to floor, so drop scrap onto the crack's
        // neighbour and check the rule directly.
        let w = crack_world();
        let rules_for = |id: &str| w.content().action(id).unwrap().clone();
        let pile = Pos::new(2, 1, 0);
        assert_eq!(
            cluttered(&w, pile, &rules_for("repair")),
            Some(Blocked::Cluttered)
        );
        assert_eq!(cluttered(&w, pile, &rules_for("pick_up")), None);
        assert_eq!(cluttered(&w, Pos::new(1, 0, 0), &rules_for("repair")), None);
        assert_eq!(REPAIR, w.content().action_index("repair").unwrap());
    }

    #[test]
    fn with_the_right_tool_in_reach_nothing_blocks_the_cut() {
        // The steel door two tiles south needs LASER 2.
        let w = with_laser(world(), 2);
        assert_eq!(blocked(&w, Pos::new(1, 3, 0), CUT), None);
    }

    /// The world, wearing a tool with `LASER level`.
    fn with_laser(world: World, level: u8) -> World {
        with_tool(world, "LASER", level)
    }

    /// The world, wearing a tool with `quality` at `level`. The tool is set
    /// up by editing the world's saved form, since its fields are private
    /// to the `world` module.
    fn with_tool(world: World, quality: &str, level: u8) -> World {
        let mut json: serde_json::Value = serde_json::to_value(&world).unwrap();
        json["content"]["item_types"][1]["qualities"] = serde_json::json!({ quality: level });
        json["content"]["item_types"][1]["wear"] = serde_json::json!({ "pockets": [] });
        json["worn"] = serde_json::json!([ { "kind": 1, "material": 0, "pockets": [] } ]);
        serde_json::from_value(json).unwrap()
    }
}
