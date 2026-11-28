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
use crate::power;
use crate::quality::Levels;
use crate::recipe::Use;
use crate::world::World;

/// Why an action can't be done right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    /// You can't see the tile (it's out of sight, or later, in the dark),
    /// so you can't act on it, or even tell what's there.
    CantSee,
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
    /// type number `item`, and you have `have`. If only items of a material
    /// with a tag count, `tag` is its number in the world's list of
    /// material tags (`Content::material_tags`).
    Missing {
        item: u16,
        need: u32,
        have: u32,
        tag: Option<u16>,
    },
    /// Items are lying on it, and would end up inside it.
    Cluttered,
    /// For a craft: the player hasn't yet carried everything it uses, so
    /// they don't know how to make it.
    NotKnown,
    /// For putting something away: there's no room left inside.
    Full,
    /// Every tool good enough runs on a cell, and none holds enough
    /// charge for the work: item type number `item` is the best charged,
    /// which needs `need_kj` kJ (rounded up) and has `have_kj` (rounded
    /// down) (decision D59).
    LowCharge {
        item: u16,
        need_kj: u32,
        have_kj: u32,
    },
    /// For loading a cell: you carry none it takes that's fuller than the
    /// one already in it.
    NoFullerCell,
    /// For taking a cell out, or switching on: there's no cell in it.
    NoCell,
    /// For switching on: its cell is flat.
    Flat,
    /// For putting something on: you're already wearing item type number
    /// `item` in its slot (decision D61).
    Wearing { item: u16 },
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
/// floor, and for tiles you can't see (the list would give away what's
/// there).
pub fn options_at(world: &World, at: Pos) -> Vec<ActionOption> {
    if !world.is_visible(at) {
        return Vec::new();
    }
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
/// `at` right now, or `None` if nothing does. A tile you can't see comes
/// first: you can't tell what else is wrong with it.
pub fn blocked(world: &World, at: Pos, action: u16) -> Option<Blocked> {
    if !world.is_visible(at) {
        return Some(Blocked::CantSee);
    }
    match world.content().actions().get(usize::from(action)) {
        Some(action) if applies(world, at, action) => rules(world, at, action),
        _ => Some(Blocked::NotPossible),
    }
}

/// Whether `action` makes sense at `at` at all: there's a recipe for it
/// (opening a door is one), or there are items to pick up. Off the map
/// there's neither (and it's never in sight, which is checked first).
fn applies(world: &World, at: Pos, action: &Action) -> bool {
    match action.effect {
        Effect::Recipe => world.recipe_for(&action.id, at).is_some(),
        Effect::PickUp => world.items_at(at).next().is_some(),
        Effect::TakeOut => world
            .furniture_at(at)
            .is_some_and(|piece| !piece.contents.is_empty()),
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
/// there or furniture stands there.
fn cluttered(world: &World, at: Pos, action: &Action) -> Option<Blocked> {
    let lying = world.items_at(at).next().is_some();
    let standing = world.furniture_at(at).is_some();
    (action.clear_of_items && (lying || standing)).then_some(Blocked::Cluttered)
}

/// `Needs` for the first tool level the player is short of: what the
/// material asks for this action (any one of its levels is enough), then
/// what the recipe itself needs (all of them).
fn tool_needed(world: &World, at: Pos, action: &Action) -> Option<Blocked> {
    material_tool(world, at, action).or_else(|| recipe_tools(world, at, action))
}

/// How long the recipe for `action` at `at` takes, which is how long its
/// tools must work: 0 if there's no recipe.
fn work_seconds(world: &World, at: Pos, action: &Action) -> u32 {
    world.recipe_for(&action.id, at).map_or(0, |r| r.seconds)
}

/// What the material asks: met if any one of its levels for this action
/// is; if none is, why not for the first.
fn material_tool(world: &World, at: Pos, action: &Action) -> Option<Blocked> {
    let levels = material_levels(world, at, action)?;
    let seconds = work_seconds(world, at, action);
    let mut reasons = levels
        .iter()
        .map(|(q, &level)| tool_blocked(world, q, level, seconds));
    // The first level's reason (`None` if it's met), unless a later level
    // is met.
    let first = reasons.next()?;
    if reasons.any(|reason| reason.is_none()) {
        return None;
    }
    first
}

/// The tool levels the material at `at` lists for `action` (any one is
/// enough), if the action asks the material.
fn material_levels<'a>(world: &'a World, at: Pos, action: &Action) -> Option<&'a Levels> {
    if !action.tool_from_material {
        return None;
    }
    world.material_at(at)?.worked_by.get(&action.id)
}

/// What the recipe asks: every level it lists.
fn recipe_tools(world: &World, at: Pos, action: &Action) -> Option<Blocked> {
    let recipe = world.recipe_for(&action.id, at)?;
    short_of(world, &recipe.needs, recipe.seconds)
}

/// Why the first of `levels` (tool levels, all needed, for work lasting
/// `seconds`) that the tools at hand fall short of isn't met. Recipes and
/// crafts share it.
pub(crate) fn short_of(world: &World, levels: &Levels, seconds: u32) -> Option<Blocked> {
    levels
        .iter()
        .find_map(|(quality, &level)| tool_blocked(world, quality, level, seconds))
}

/// Why no tool at hand can do `level` of `quality` for `seconds`, if none
/// can: `Needs` with nothing that good, or `LowCharge` when everything
/// that good runs on a cell that can't cover the work (decision D59).
pub(crate) fn tool_blocked(
    world: &World,
    quality: &str,
    level: u8,
    seconds: u32,
) -> Option<Blocked> {
    let types = world.item_types();
    let tools = world.tools(quality, level);
    if tools.iter().any(|(_, t)| power::can_run(t, types, seconds)) {
        return None;
    }
    // The best charged, if anything that good is at hand at all.
    let Some((_, best)) = tools
        .iter()
        .max_by_key(|(_, t)| power::charge(t, types).map_or(0, |(j, _)| j))
    else {
        return Some(needs(world, quality, level));
    };
    let have = power::charge(best, types).map_or(0, |(j, _)| u64::from(j));
    let need = power::needed_j(best, types, seconds);
    Some(Blocked::LowCharge {
        item: best.kind,
        need_kj: u32::try_from(need.div_ceil(1000)).unwrap_or(u32::MAX),
        have_kj: u32::try_from(power::kj(have)).unwrap_or(u32::MAX),
    })
}

/// The tool levels following the recipe for `action` at `at` puts to
/// work, so their cells can pay for it: the first of the material's that
/// a tool at hand can do, and every one the recipe itself needs.
pub(crate) fn tools_used(world: &World, at: Pos, action: &Action) -> Vec<(String, u8)> {
    let seconds = work_seconds(world, at, action);
    let material = material_levels(world, at, action)
        .into_iter()
        .flatten()
        .find(|&(q, &level)| tool_blocked(world, q, level, seconds).is_none());
    let recipe = world.recipe_for(&action.id, at).map(|r| &r.needs);
    material
        .into_iter()
        .chain(recipe.into_iter().flatten())
        .map(|(q, &level)| (q.clone(), level))
        .collect()
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
    missing_items(world, &recipe.uses)
}

/// `Missing` for the first of `uses` that the player doesn't have enough
/// of at hand, in their pockets or in furniture within reach (counting
/// only items of the tagged material, for a use with a tag). Recipes and
/// crafts share it.
pub(crate) fn missing_items(world: &World, uses: &[Use]) -> Option<Blocked> {
    let tags = world.content().material_tags();
    uses.iter().find_map(|used| {
        let item = world.item_type_index(&used.item)?;
        let tag = used.tag.as_deref();
        let have = world.count_at_hand(item, tag);
        let tag = tag.and_then(|t| tags.iter().position(|&known| known == t));
        (have < used.count).then_some(Blocked::Missing {
            item,
            need: used.count,
            have,
            tag: tag.and_then(|i| u16::try_from(i).ok()),
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
    /// wall `H` at (5, 1); a two-tile pillar `##` at (3, 2) and (4, 2);
    /// lamps `*` in the walls above and below it.
    ///
    /// ```text
    /// ###*###
    /// #@'..H#
    /// #..##.#
    /// #+....#
    /// ###*###
    /// ```
    fn world() -> World {
        let rows = ["###*###", "#@'..H#", "#..##.#", "#+....#", "###*###"];
        let json = format!(
            r##"{{ "id": "t", "name": "T",
                 "legend": {{ "#": "wall", ".": "floor", "+": "door", "*": "lamp",
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
            Some(Blocked::CantSee),
            "hidden right behind it, so first of all you can't see it"
        );
        assert!(
            options_at(&w, Pos::new(4, 2, 0)).is_empty(),
            "nothing given away"
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
    fn a_wall_seen_past_a_corner_can_still_be_in_the_way_of_the_laser() {
        // From (1, 1), the wall at (3, 2) is seen past the corner of the
        // wall at (2, 2), but the laser's straight line runs through it.
        //
        //     ##*##
        //     #@..#
        //     #.###
        //     #####
        let rows = ["##*##", "#@..#", "#.###", "#####"];
        let json = format!(
            r##"{{ "id": "t", "name": "T", "legend": {{ "#": "wall", ".": "floor", "*": "lamp" }},
                 "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        let w = with_laser(World::new(parse_map(&json).unwrap(), 1), 2);
        let target = Pos::new(3, 2, 0);
        assert!(w.is_visible(target));
        assert_eq!(
            blocked(&w, target, CUT),
            Some(Blocked::InTheWay {
                at: Pos::new(2, 2, 0)
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
            Some(Blocked::CantSee),
            "off the map there's nothing to see"
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
        json["content"]["item_types"][1]["wear"] =
            serde_json::json!({ "slot": "hand", "pockets": [] });
        json["worn"] = serde_json::json!([ { "kind": 1, "material": 0, "pockets": [] } ]);
        let mut world: World = serde_json::from_value(json).unwrap();
        world.finish_loading().unwrap(); // saves don't keep tiles: rebuild them
        world
    }
}
