//! Hand tools and components (decision D60): the real content's crafting
//! chain, played out with commands. Rags twist into cordage, cordage and
//! scrap make a hammer, the hammer beats scrap into plate, and plate
//! becomes a pry bar (for jammed doors), a wrench (for bolted furniture)
//! or, with wire and a rag, a hull patch kit (for breaches).

mod common;

use nighthawk_sim::{Blocked, Command, Event, Pos, World, blocked};

/// A workshop drawn by `rows` on the real content: you stand at `@`
/// wearing a jumpsuit, on a steel scrap, beside a locker `L` stocked with
/// twelve aluminium scraps, two of polyester, and a shiv. `J` is a jammed
/// door, `X` a hull breach, `#` a bulkhead and `H` hull.
fn workshop(rows: &[&str]) -> World {
    let json = format!(
        r##"{{ "id": "workshop", "name": "Workshop", "player_wears": [ {{ "item": "jumpsuit" }} ],
             "legend": {{ "H": "hull_plating", "#": "bulkhead", ".": "deck_plating",
                 "J": "jammed_door", "X": "hull_breach",
                 "@": {{ "structure": "deck_plating", "items": [ {{ "item": "scrap" }} ] }},
                 "L": {{ "structure": "deck_plating", "furniture": "locker", "contents": [
                     {{ "item": "scrap", "material": "aluminium_alloy", "count": 12 }},
                     {{ "item": "scrap", "material": "polyester", "count": 2 }},
                     {{ "item": "shiv" }} ] }} }},
             "decks": [ {{ "name": "Workshop", "rows": {rows:?} }} ] }}"##
    );
    World::new(common::parse(&json), 1)
}

/// A jammed door east of you: `HL@J.H`.
const BY_A_JAMMED_DOOR: [&str; 3] = ["HHHHHH", "HL@J.H", "HHHHHH"];
const JAMMED: Pos = Pos::new(3, 1, 0);
const LOCKER: Pos = Pos::new(1, 1, 0);

fn action(world: &World, id: &str) -> u16 {
    world.content().action_index(id).unwrap()
}

fn craft_number(world: &World, id: &str) -> u16 {
    let i = world.content().crafts().iter().position(|c| c.id == id);
    u16::try_from(i.unwrap_or_else(|| panic!("no craft {id:?}"))).unwrap()
}

/// Makes the craft with id `id`, and says what it made.
fn craft(world: &mut World, id: &str) -> Vec<String> {
    let number = craft_number(world, id);
    assert_eq!(world.craft_blocked(number), None, "{id}");
    let events = world.apply(Command::Craft(number));
    let types = world.item_types();
    events
        .iter()
        .filter_map(|e| match e {
            Event::Crafted { item, .. } => Some(types[usize::from(item.kind)].id.clone()),
            _ => None,
        })
        .collect()
}

/// How many of the item `id` you carry.
fn carrying(world: &World, id: &str) -> u32 {
    world.carried_count(world.item_type_index(id).unwrap())
}

/// Picks up the scrap underfoot, cuts a polyester scrap into rags with the
/// shiv in the locker, twists them into cordage, and lashes two scraps to
/// it into a hammer.
fn make_a_hammer(world: &mut World) {
    let here = world.player();
    world.apply(Command::Interact {
        at: here,
        action: action(world, "pick_up"),
    });
    assert_eq!(craft(world, "rags"), ["rag", "rag"]);
    assert_eq!(craft(world, "cordage"), ["cordage"]);
    assert_eq!(craft(world, "makeshift_hammer"), ["makeshift_hammer"]);
    assert_eq!(world.tool_level("HAMMER"), 1);
    assert_eq!(carrying(world, "scrap"), 0, "the carried scrap went first");
}

#[test]
fn the_crew_deck_cabin_door_is_jammed_and_must_be_pried_or_cut() {
    let world = common::start_world(1);
    let door = Pos::new(11, 4, 1);
    assert_eq!(world.structure_at(door).unwrap().id, "jammed_door");
    assert!(world.recipe_for("open", door).is_none(), "it won't open");
    let pry = world.recipe_for("pry", door).unwrap();
    assert_eq!(pry.needs.get("PRY"), Some(&1));
    assert_eq!(pry.to.as_deref(), Some("door_open"));
    let cut = world.recipe_for("cut", door).unwrap();
    assert_eq!(cut.to.as_deref(), Some("damaged_door"));
}

#[test]
fn rags_become_cordage_and_cordage_and_scrap_a_hammer() {
    let mut world = workshop(&BY_A_JAMMED_DOOR);
    assert!(world.known_crafts().is_empty());
    make_a_hammer(&mut world);
    let hammer = world.item_type_index("makeshift_hammer").unwrap();
    let (_, made) = world
        .carried()
        .into_iter()
        .find(|(_, item)| item.kind == hammer)
        .unwrap();
    let steel = world.content().material_index("steel").unwrap();
    assert_eq!(made.material, steel, "of the first scrap used, yours");
    let known: Vec<&str> = world
        .known_crafts()
        .into_iter()
        .map(|n| world.content().crafts()[usize::from(n)].id.as_str())
        .collect();
    assert_eq!(
        known,
        [
            "shiv",
            "rags",
            "cordage",
            "wire",
            "metal_plate",
            "makeshift_hammer",
            "sling_pack"
        ],
        "plate-made tools come to mind once you've held a plate"
    );
}

#[test]
fn a_pry_bar_hammered_from_plate_levers_a_jammed_door_open() {
    let mut world = workshop(&BY_A_JAMMED_DOOR);
    let pry = action(&world, "pry");
    let quality = world.content().quality_index("PRY").unwrap();
    assert_eq!(
        blocked(&world, JAMMED, pry),
        Some(Blocked::Needs { quality, level: 1 })
    );
    assert_eq!(
        blocked(&world, JAMMED, action(&world, "open")),
        Some(Blocked::NotPossible),
        "it won't slide open"
    );
    make_a_hammer(&mut world);
    assert_eq!(craft(&mut world, "metal_plate"), ["metal_plate"]);
    assert_eq!(craft(&mut world, "pry_bar"), ["pry_bar"]);
    assert_eq!(carrying(&world, "metal_plate"), 0);
    let before = world.seconds();
    world.apply(Command::Interact {
        at: JAMMED,
        action: pry,
    });
    assert_eq!(world.seconds() - before, 30);
    assert_eq!(world.structure_at(JAMMED).unwrap().id, "door_open");
}

#[test]
fn a_wrench_unbolts_a_locker_from_the_deck() {
    let mut world = workshop(&BY_A_JAMMED_DOOR);
    let dismantle = action(&world, "dismantle");
    make_a_hammer(&mut world);
    let quality = world.content().quality_index("WRENCH").unwrap();
    assert_eq!(
        blocked(&world, LOCKER, dismantle),
        Some(Blocked::Needs { quality, level: 1 })
    );
    craft(&mut world, "metal_plate");
    assert_eq!(craft(&mut world, "wrench"), ["wrench"]);
    world.apply(Command::Interact {
        at: LOCKER,
        action: dismantle,
    });
    assert!(world.furniture_at(LOCKER).is_none());
    assert!(
        world.items_at(LOCKER).count() > 5,
        "its steel, and what was still inside"
    );
}

#[test]
fn a_hull_patch_kit_of_plate_wire_and_rag_seals_a_breach() {
    // The breach is in the hull above and to your right, walled off from
    // your room, so the air stays while you work.
    let mut world = workshop(&["HHHXHH", "HL@#.H", "HHHHHH"]);
    let breach = Pos::new(3, 0, 0);
    let repair = action(&world, "repair");
    make_a_hammer(&mut world);
    let kit = world.item_type_index("hull_patch_kit").unwrap();
    assert_eq!(
        blocked(&world, breach, repair),
        Some(Blocked::Missing {
            item: kit,
            need: 1,
            have: 0,
            tag: None
        }),
        "scrap won't do any more"
    );
    craft(&mut world, "metal_plate");
    assert_eq!(craft(&mut world, "wire"), ["wire", "wire"]);
    assert_eq!(craft(&mut world, "rags"), ["rag", "rag"]);
    assert_eq!(craft(&mut world, "hull_patch_kit"), ["hull_patch_kit"]);
    world.apply(Command::Interact {
        at: breach,
        action: repair,
    });
    assert_eq!(world.structure_at(breach).unwrap().id, "hull_plating");
    assert_eq!(
        world.material_at(breach).unwrap().id,
        "aluminium_alloy",
        "the patch is the plate's metal"
    );
    assert_eq!(carrying(&world, "hull_patch_kit"), 0);
}
