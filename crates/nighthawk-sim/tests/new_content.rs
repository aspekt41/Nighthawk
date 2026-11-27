//! The proof that content is all data (decision D43): a whole new mechanic,
//! welding, added without a line of Rust that knows about it.
//!
//! The shipped `data/` is loaded with one extra file per folder: a `WELD`
//! quality, a welding torch with `WELD 1`, a Weld action, and a recipe
//! that welds a hull crack shut with one scrap. Then a tiny map is played:
//! the crack can't be welded until the torch is picked up, and then it can.
//! The last test checks that no Rust outside tests mentions welding.

mod common;

use std::fs;

use nighthawk_sim::{
    Blocked, Command, Content, ContentError, ContentFile, Dir, Event, Pos, Vertical, World,
    options_at, parse_map,
};

/// The extra content, per folder: what a modder would write.
fn welding(folder: &str) -> Option<&'static str> {
    Some(match folder {
        "qualities" => {
            r#"{ "qualities": [ { "id": "WELD", "name": "welding",
                 "description": "Joining metal with heat." } ] }"#
        }
        "items" => {
            r#"{ "items": [ { "id": "welding_torch", "name": "welding torch",
                 "description": "A small gas torch.", "glyph": "!", "material": "steel",
                 "volume_ml": 300, "solid_percent": 50, "qualities": { "WELD": 1 } } ] }"#
        }
        "actions" => {
            r#"{ "actions": [ { "id": "weld", "name": "Weld", "key": "w", "reach": 1,
                 "clear_of_items": true, "done": "You weld the {was} shut." } ] }"#
        }
        "recipes" => {
            r#"{ "recipes": [ { "id": "weld_hull_crack", "action": "weld",
                 "from": "hull_crack", "to": "hull_plating", "seconds": 90,
                 "needs": { "WELD": 1 }, "uses": [ { "item": "scrap", "count": 1 } ],
                 "material": "from_used" } ] }"#
        }
        _ => return None,
    })
}

/// The shipped content, plus the welding files.
fn content_with_welding() -> Content {
    Content::load(|folder| {
        let mut files = common::read_folder(folder);
        files.extend(welding(folder).map(|text| ContentFile::new("zz_welding.json", text)));
        Ok::<_, ContentError>(files)
    })
    .unwrap_or_else(|e| panic!("data/{}: {e}", e.folder()))
}

/// A one-room ship made of shipped structures: a hull crack `x` north of
/// the player, and a welding torch and a scrap lying just east.
///
/// ```text
/// HHxHH
/// H.@tH
/// HHHHH
/// ```
fn welding_world() -> World {
    let rows = ["HHxHH", "H.@tH", "HHHHH"];
    let json = format!(
        r#"{{ "id": "workshop", "name": "Workshop", "player_wears": [ {{ "item": "jumpsuit" }} ],
             "legend": {{ "H": "hull_plating", ".": "deck_plating", "@": "deck_plating",
                         "x": "hull_crack",
                         "t": {{ "structure": "deck_plating",
                                 "items": [ {{ "item": "welding_torch" }}, {{ "item": "scrap" }} ] }} }},
             "decks": [ {{ "name": "Workshop", "rows": {rows:?} }} ] }}"#
    );
    World::new(parse_map(&json, &content_with_welding()).unwrap(), 1)
}

const CRACK: Pos = Pos::new(2, 0, 0);
const PILE: Pos = Pos::new(3, 1, 0);

fn action(world: &World, id: &str) -> u16 {
    world
        .content()
        .action_index(id)
        .unwrap_or_else(|| panic!("no action {id:?}"))
}

#[test]
fn the_crack_offers_welding_but_needs_the_torch() {
    let world = welding_world();
    let weld = action(&world, "weld");
    let option = options_at(&world, CRACK)
        .into_iter()
        .find(|o| o.action == weld)
        .expect("the menu offers Weld on the crack");
    let quality = world.content().quality_index("WELD").unwrap();
    assert_eq!(option.blocked, Some(Blocked::Needs { quality, level: 1 }));
}

#[test]
fn with_the_torch_and_a_scrap_the_crack_is_welded_shut() {
    let mut world = welding_world();
    world.apply(Command::Interact {
        at: PILE,
        action: action(&world, "pick_up"),
    });
    let weld = action(&world, "weld");
    let before = world.seconds();
    let events = world.apply(Command::Interact {
        at: CRACK,
        action: weld,
    });

    let crack = world.content().structure_index("hull_crack").unwrap();
    let welded = Event::Transformed {
        at: CRACK,
        action: weld,
        was: crack,
    };
    assert_eq!(events.last(), Some(&welded));
    assert_eq!(world.seconds() - before, 90);
    assert_eq!(world.structure_at(CRACK).unwrap().id, "hull_plating");
    assert_eq!(
        world.material_at(CRACK).unwrap().id,
        "steel",
        "from the scrap"
    );
    assert_eq!(
        world.map().get(CRACK).unwrap().hole_cm2(),
        0,
        "no more leak"
    );
    let scrap = world.item_type_index("scrap").unwrap();
    assert_eq!(world.carried_count(scrap), 0, "the scrap was used");
    let line = world
        .content()
        .action("weld")
        .unwrap()
        .done_text("cracked hull", "");
    assert_eq!(line, "You weld the cracked hull shut.");
}

#[test]
fn the_shipped_ship_still_loads_with_welding_added() {
    let text = fs::read_to_string(common::data_dir().join("maps/aft_ring.json")).unwrap();
    let mut world = World::new(parse_map(&text, &content_with_welding()).unwrap(), 1);
    assert_eq!(world.area_name(), common::start_world(1).area_name());
    // Its real crack can be welded now too, with no change to the map:
    // walk up to the crew deck and into the room with the crack in its
    // hull, so it's in sight.
    let route = [(Dir::SouthWest, 1), (Dir::North, 6), (Dir::East, 24)]
        .into_iter()
        .chain([(Dir::South, 3), (Dir::West, 11), (Dir::North, 3)])
        .chain([(Dir::NorthEast, 1), (Dir::North, 1)]);
    for (i, (dir, steps)) in route.enumerate() {
        if i == 3 {
            world.apply(Command::Climb(Vertical::Up));
        }
        for _ in 0..steps {
            world.apply(Command::Move(dir));
        }
    }
    assert_eq!(world.player(), Pos::new(23, 1, 1));
    let weld = action(&world, "weld");
    let real_crack = Pos::new(23, 0, 1);
    assert!(
        options_at(&world, real_crack)
            .iter()
            .any(|o| o.action == weld)
    );
}

#[test]
fn no_rust_outside_tests_knows_about_welding() {
    // Every source file, up to its test module, with comments left out
    // (a doc comment may use welding as an example).
    let crates = common::data_dir().join("../crates");
    let mut checked = 0;
    for krate in ["nighthawk-sim", "nighthawk-tui"] {
        for entry in fs::read_dir(crates.join(krate).join("src")).unwrap() {
            let path = entry.unwrap().path();
            let text = fs::read_to_string(&path).unwrap();
            let code = text.split("#[cfg(test)]").next().unwrap_or_default();
            let mentions: Vec<&str> = code
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .filter(|l| l.to_lowercase().contains("weld"))
                .collect();
            assert!(mentions.is_empty(), "{}: {mentions:?}", path.display());
            checked += 1;
        }
    }
    assert!(checked > 20, "only {checked} files checked");
}
