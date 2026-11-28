//! Content validation: every map file shipped in `data/maps` must load.
//!
//! This is an integration test (it lives in `tests/`, outside `src/`), so it
//! uses the crate only through its public API, the same way a front-end does.
//! It's also allowed to read files, unlike the sim itself.

mod common;

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;

use nighthawk_sim::{
    Command, Dir, Event, LoadedMap, MapError, Piece, Place, Pos, Tile, Vertical, World, parse_map,
};

/// The number of the shipped action with this id, like `"cut"`.
fn action(world: &nighthawk_sim::World, id: &str) -> u16 {
    world
        .content()
        .action_index(id)
        .unwrap_or_else(|| panic!("no action {id:?}"))
}

fn maps_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR is this crate's folder, crates/nighthawk-sim.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/maps")
}

/// Parses a map's text against the shipped materials and structures.
fn load(text: &str) -> Result<LoadedMap, MapError> {
    parse_map(text, &common::content())
}

#[test]
fn every_shipped_map_loads() {
    let mut checked = 0;
    for entry in fs::read_dir(maps_dir()).expect("data/maps exists") {
        let path = entry.expect("readable directory entry").path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let text = fs::read_to_string(&path).expect("readable map file");
            if let Err(e) = load(&text) {
                panic!("{} failed to load: {e}", path.display());
            }
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "no map files found in {}",
        maps_dir().display()
    );
}

#[test]
fn every_shipped_map_id_matches_its_file_name() {
    for entry in fs::read_dir(maps_dir()).expect("data/maps exists") {
        let path = entry.expect("readable directory entry").path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let loaded = load(&fs::read_to_string(&path).unwrap()).unwrap();
            let stem = path.file_stem().unwrap().to_string_lossy();
            assert_eq!(
                loaded.id,
                stem,
                "{}: id must match the file name",
                path.display()
            );
        }
    }
}

#[test]
fn the_starting_map_has_no_way_off_its_edges() {
    // The player must not be able to walk off the map. Edge tiles are hull:
    // plain bulkhead, or damaged hull (cracks and breaches leak air, but
    // they're solid).
    let text = fs::read_to_string(maps_dir().join("aft_ring.json")).unwrap();
    let loaded = load(&text).unwrap();
    let map = &loaded.map;
    let on_edge =
        |p: &Pos| p.x == 0 || p.y == 0 || p.x == map.width() - 1 || p.y == map.height() - 1;
    for p in map.positions().filter(on_edge) {
        let structure = map.structure_index(p).unwrap();
        let name = &loaded.content.structures()[usize::from(structure)].name;
        assert!(!passable(&loaded, p), "{p:?} is a {name}");
    }
}

/// Whether the player can get through the tile at `p`: no furniture is in
/// the way, and it's walkable, or some recipe makes it walkable. A shut
/// door opens when you walk into it (its `on_bump` action, open, makes an
/// open door); a jammed door takes a pry bar you can make, or a cut.
fn passable(loaded: &LoadedMap, p: Pos) -> bool {
    let (map, content) = (&loaded.map, &loaded.content);
    if furniture_in_the_way(loaded, p) {
        return false;
    }
    if map.get(p).is_some_and(Tile::is_walkable) {
        return true;
    }
    let Some(here) = map.structure_index(p) else {
        return false;
    };
    let id = &content.structures()[usize::from(here)].id;
    let walkable = |s: &str| {
        let to = content
            .structure_index(s)
            .expect("recipes name real structures");
        content.structures()[usize::from(to)].walkable
    };
    content
        .recipes()
        .iter()
        .any(|r| r.from.as_ref() == Some(id) && r.to.as_deref().is_some_and(walkable))
}

/// Whether a piece of furniture you can't walk through covers `p`.
fn furniture_in_the_way(loaded: &LoadedMap, p: Pos) -> bool {
    loaded.furniture.iter().any(|piece| {
        let kind = &loaded.content.furniture()[usize::from(piece.kind)];
        !kind.walkable && piece.tiles(kind).any(|t| t == p)
    })
}

#[test]
fn the_starting_map_has_hull_damage_to_vent_through() {
    let text = fs::read_to_string(maps_dir().join("aft_ring.json")).unwrap();
    let map = load(&text).unwrap().map;
    let holes: u64 = map
        .positions()
        .map(|p| map.get(p).unwrap().hole_cm2())
        .sum();
    assert_eq!(holes, 404, "one breach (400 cm²) and one crack (4 cm²)");
}

/// Every map file in `data/maps`, loaded.
fn shipped_maps() -> Vec<(PathBuf, LoadedMap)> {
    let mut maps = Vec::new();
    for entry in fs::read_dir(maps_dir()).expect("data/maps exists") {
        let path = entry.expect("readable directory entry").path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let loaded = load(&fs::read_to_string(&path).unwrap()).unwrap();
            maps.push((path, loaded));
        }
    }
    maps
}

/// Where the player can get to from `at`: the eight neighbours on the same
/// deck, plus the deck above or below when standing on stairs. Closed doors
/// count, because the player can open them.
fn neighbours(loaded: &LoadedMap, at: Pos) -> Vec<Pos> {
    let mut next: Vec<Pos> = Dir::ALL.iter().map(|&d| at.step(d)).collect();
    if let Some(way) = loaded.map.get(at).and_then(|t| t.climb) {
        next.push(at.climb(way));
    }
    next.retain(|&p| passable(loaded, p));
    next
}

/// Every tile the player can get to from `start`, by a breadth-first flood
/// fill: visit a tile, queue its unvisited neighbours, repeat until done.
fn reachable_from(loaded: &LoadedMap, start: Pos) -> HashSet<Pos> {
    let mut seen = HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(at) = queue.pop_front() {
        let new: Vec<Pos> = neighbours(loaded, at)
            .into_iter()
            .filter(|&p| seen.insert(p))
            .collect();
        queue.extend(new);
    }
    seen
}

#[test]
fn every_open_tile_can_be_reached_from_the_start() {
    // A flood fill from the player's start, following stairs between decks.
    // Anything it doesn't reach is a room the player could never enter.
    for (path, loaded) in shipped_maps() {
        let seen = reachable_from(&loaded, loaded.player_start);
        let unreachable: Vec<Pos> = loaded
            .map
            .positions()
            .filter(|&p| passable(&loaded, p) && !seen.contains(&p))
            .collect();
        assert!(
            unreachable.is_empty(),
            "{}: {} tiles can't be reached, e.g. {:?}",
            path.display(),
            unreachable.len(),
            unreachable.first()
        );
    }
}

#[test]
fn the_starting_map_has_more_than_one_deck() {
    let text = fs::read_to_string(maps_dir().join("aft_ring.json")).unwrap();
    let loaded = load(&text).unwrap();
    assert!(loaded.map.depth() >= 2);
    assert_eq!(loaded.deck_names[0], "Stasis bay");
}

#[test]
fn the_hull_is_nanocarbon_hull_structures_and_the_inside_is_neither() {
    // Every tile on the map's border is a hull structure (plating, a crack
    // or a breach), made of Nanocarbon Alloy; nothing inside the ship is.
    let world = common::start_world(1);
    let map = world.map();
    for p in map.positions() {
        let on_edge = p.x == 0 || p.y == 0 || p.x == map.width() - 1 || p.y == map.height() - 1;
        let structure = world.structure_at(p).unwrap().id.as_str();
        let material = world.material_at(p).unwrap().id.as_str();
        assert_eq!(
            on_edge,
            structure.starts_with("hull_"),
            "{p:?} is {structure}"
        );
        assert_eq!(
            on_edge,
            material == "nanocarbon_alloy",
            "{p:?} is {material}"
        );
    }
}

#[test]
fn every_shipped_structure_is_on_the_starting_map_or_made_by_a_recipe() {
    // A structure nothing uses is probably a typo in a map's legend or a
    // recipe.
    let world = common::start_world(1);
    let content = common::content();
    let mut used: HashSet<&str> = world
        .map()
        .positions()
        .map(|p| world.structure_at(p).unwrap().id.as_str())
        .collect();
    used.extend(content.recipes().iter().filter_map(|r| r.to.as_deref()));
    for structure in world.structures() {
        assert!(
            used.contains(structure.id.as_str()),
            "{} is unused",
            structure.id
        );
    }
}

#[test]
fn the_shipped_materials_have_the_agreed_laser_levels() {
    // Levels are set by hand (decision D35). This test is the list to
    // update on purpose when they're retuned.
    let content = common::content();
    let level = |id: &str| {
        let i = usize::from(content.material_index(id).unwrap());
        content.materials()[i]
            .worked_by
            .get("cut")
            .and_then(|levels| levels.get("LASER"))
            .copied()
    };
    assert_eq!(level("aluminium_alloy"), Some(1));
    assert_eq!(level("steel"), Some(2));
    assert_eq!(level("nanocarbon_alloy"), Some(5));
}

#[test]
fn you_start_in_a_jumpsuit_with_four_one_litre_pockets_and_deck_shoes() {
    let world = common::start_world(1);
    let worn: Vec<&str> = world
        .worn()
        .iter()
        .map(|w| world.item_types()[usize::from(w.kind)].id.as_str())
        .collect();
    assert_eq!(worn, ["jumpsuit", "deck_shoes"]);
    let jumpsuit = &world.item_types()[usize::from(world.worn()[0].kind)];
    let pockets: Vec<(&str, u32, u32)> = jumpsuit
        .pockets()
        .iter()
        .map(|p| (p.name.as_str(), p.volume_ml, p.max_mass_g))
        .collect();
    assert_eq!(
        pockets,
        vec![
            ("left hip pocket", 1000, 2500),
            ("right hip pocket", 1000, 2500),
            ("left leg pocket", 1000, 2500),
            ("right leg pocket", 1000, 2500),
        ]
    );
}

#[test]
fn four_steel_scraps_lie_near_the_start() {
    let world = common::start_world(1);
    let ctx = world.item_context();
    let near: Vec<(&str, &str, u64)> = world
        .floor_items()
        .iter()
        .filter(|f| f.at.z == 0 && world.player().steps_to(f.at).is_some_and(|d| d <= 3))
        .map(|f| {
            let kind = &world.item_types()[usize::from(f.item.kind)];
            let material = &world.materials()[usize::from(f.item.material)];
            (kind.id.as_str(), material.id.as_str(), ctx.mass_g(&f.item))
        })
        .collect();
    assert_eq!(
        near,
        vec![
            ("scrap", "steel", 785),
            ("scrap", "steel", 785),
            ("scrap", "steel", 785),
            ("scrap", "steel", 785),
        ]
    );
    let kind = |id: &str| &world.item_types()[usize::from(world.item_type_index(id).unwrap())];
    assert_eq!(kind("basic_laser_cutter").qualities.get("LASER"), Some(&1));
    assert_eq!(kind("hand_lamp").light, 4);
}

#[test]
fn you_wake_beside_your_own_empty_stasis_chamber_among_others() {
    let world = common::start_world(1);
    let chamber = world.content().furniture_index("stasis_chamber").unwrap();
    let chambers: Vec<&Piece> = world
        .furniture()
        .iter()
        .filter(|p| p.kind == chamber)
        .collect();
    assert_eq!(
        chambers.len(),
        6,
        "four on the north wall, two on the south"
    );
    let your_kind = world.content().furniture_index("your_stasis_chamber");
    let yours = world.furniture_at(Pos::new(11, 6, 0)).unwrap();
    assert_eq!((Some(yours.kind), yours.turned), (your_kind, false));
    assert!(yours.contents.is_empty());
    let port = &world.content().furniture()[usize::from(yours.kind)];
    assert_eq!(
        (port.charges_w, port.holds_ml),
        (100, 2000),
        "a charging port"
    );
    assert_eq!(
        world.player().steps_to(yours.at),
        Some(1),
        "right beside you"
    );
    let turned = chambers.iter().filter(|p| p.turned).count();
    assert_eq!(turned, 2);
    assert!(
        world.is_visible(Pos::new(7, 4, 0)),
        "the locker, in view from the start"
    );
}

#[test]
fn bones_and_torn_padding_lie_about_the_stasis_bay() {
    let world = common::start_world(1);
    let lying = |material: &str| {
        let m = world.content().material_index(material).unwrap();
        let on_deck_0 = world.floor_items().iter().filter(|f| f.at.z == 0);
        on_deck_0.filter(|f| f.item.material == m).count()
    };
    assert_eq!(lying("bone"), 4, "Scrap (Bone)");
    assert_eq!(lying("polyester"), 2, "Scrap (Polyester)");
}

#[test]
fn every_kind_of_furniture_is_somewhere_in_the_ship() {
    let world = common::start_world(1);
    for (kind, furniture) in (0..).zip(world.content().furniture()) {
        let placed = world.furniture().iter().any(|p| p.kind == kind);
        assert!(placed, "no {} anywhere", furniture.id);
    }
    let crew = world.furniture().iter().filter(|p| p.at.z == 1).count();
    assert!(crew >= 20, "the crew deck is furnished: {crew} pieces");
}

#[test]
fn emergency_lights_light_the_stasis_bay_and_one_end_of_the_crew_hall() {
    let mut world = common::start_world(1);
    let lit = |world: &World, x, y, z| world.is_lit(Pos::new(x, y, z));
    assert!(lit(&world, 10, 7, 0), "where you wake");
    assert!(
        lit(&world, 6, 7, 0),
        "the side room's door, to cut from afar"
    );
    assert!(lit(&world, 33, 2, 0), "the ladder up");
    assert!(!lit(&world, 3, 7, 0), "the side room is dark");
    // Up the ladder: the hall is lit only at its far west end, and the
    // cabins are dark.
    let route = std::iter::repeat_n(Command::Move(Dir::North), 5)
        .chain(std::iter::repeat_n(Command::Move(Dir::East), 23))
        .chain([Command::Climb(Vertical::Up)]);
    for command in route {
        world.apply(command);
    }
    assert_eq!(world.player(), Pos::new(33, 2, 1));
    assert!(
        lit(&world, 8, 5, 1) && lit(&world, 14, 6, 1),
        "the west end"
    );
    assert!(!lit(&world, 25, 5, 1), "the middle of the hall");
    assert!(!lit(&world, 33, 2, 1), "the ladder up here");
    assert!(!lit(&world, 23, 2, 1) && !lit(&world, 10, 10, 1), "cabins");
}

/// The gear locker, in view of where you wake.
const GEAR_LOCKER: Pos = Pos::new(7, 4, 0);

/// From the start, two steps north-west to the gear locker; take
/// everything out of it; then two steps south, to (8, 7), two tiles from
/// the side room's door.
fn fetch_the_gear(world: &mut World) {
    world.apply(Command::Move(Dir::NorthWest));
    world.apply(Command::Move(Dir::NorthWest));
    world.apply(Command::Interact {
        at: GEAR_LOCKER,
        action: action(world, "take_out"),
    });
    world.apply(Command::Move(Dir::South));
    world.apply(Command::Move(Dir::South));
    assert_eq!(world.player(), Pos::new(8, 7, 0));
    let cutter = world.item_type_index("basic_laser_cutter").unwrap();
    assert_eq!(world.carried_count(cutter), 1);
}

#[test]
fn the_gear_locker_in_view_of_the_start_holds_a_cutter_a_lamp_and_a_screwdriver() {
    let mut world = common::start_world(1);
    assert!(world.is_visible(GEAR_LOCKER));
    let inside: Vec<&str> = world
        .furniture_at(GEAR_LOCKER)
        .unwrap()
        .contents
        .iter()
        .map(|item| world.item_types()[usize::from(item.kind)].id.as_str())
        .collect();
    assert_eq!(inside, ["basic_laser_cutter", "hand_lamp", "screwdriver"]);
    fetch_the_gear(&mut world);
    let lamp = world.item_type_index("hand_lamp").unwrap();
    assert_eq!(world.carried_count(lamp), 1);
    assert!(world.furniture_at(GEAR_LOCKER).unwrap().contents.is_empty());
}

#[test]
fn the_cutter_runs_on_its_cell_and_your_own_chamber_charges_it() {
    let mut world = common::start_world(1);
    fetch_the_gear(&mut world); // to (8, 7), two tiles from a door
    let cutter_kind = world.item_type_index("basic_laser_cutter").unwrap();
    let cutter_charge = |world: &World| {
        let (_, cutter) = world
            .carried()
            .into_iter()
            .find(|(_, item)| item.kind == cutter_kind)
            .unwrap();
        world.charge(cutter)
    };
    assert_eq!(cutter_charge(&world), Some((150_000, 300_000)), "half full");
    let door = Pos::new(6, 7, 0);
    let cut = action(&world, "cut");
    world.apply(Command::Interact {
        at: door,
        action: cut,
    });
    assert_eq!(world.structure_at(door).unwrap().id, "damaged_door");
    assert_eq!(
        cutter_charge(&world),
        Some((30_000, 300_000)),
        "a kilowatt for two minutes"
    );
    // Back east, beside your own chamber; leave the cutter in it a minute.
    world.apply(Command::Move(Dir::East));
    world.apply(Command::Move(Dir::East));
    let yours = Pos::new(11, 6, 0);
    let (path, _) = world
        .carried()
        .into_iter()
        .find(|(_, item)| item.kind == cutter_kind)
        .unwrap();
    world.apply(Command::Store {
        item: path,
        into: Place::Furniture(yours),
    });
    for _ in 0..12 {
        world.apply(Command::Wait);
    }
    let inside = &world.furniture_at(yours).unwrap().contents[0];
    assert_eq!(
        world.charge(inside),
        Some((36_200, 300_000)),
        "100 W for 62 s"
    );
}

#[test]
fn beside_the_gear_locker_its_tools_are_at_hand_and_things_can_go_back_in() {
    let mut world = common::start_world(1);
    assert_eq!(world.tool_level("SCREW"), 0, "where you wake");
    world.apply(Command::Move(Dir::NorthWest));
    world.apply(Command::Move(Dir::NorthWest));
    assert_eq!(world.containers_in_reach(), [GEAR_LOCKER], "the only one");
    assert_eq!(world.tool_level("SCREW"), 1, "the screwdriver inside");
    world.apply(Command::Interact {
        at: GEAR_LOCKER,
        action: action(&world, "take_out"),
    });
    let (first, _) = world.carried()[0];
    let events = world.apply(Command::Store {
        item: first,
        into: Place::Furniture(GEAR_LOCKER),
    });
    assert!(events.iter().any(|e| matches!(e, Event::Stored { .. })));
    assert_eq!(world.furniture_at(GEAR_LOCKER).unwrap().contents.len(), 1);
}

#[test]
fn every_kind_of_furniture_can_be_smashed_or_dismantled_with_a_tool() {
    let world = common::start_world(1);
    let recipes = world.content().recipes();
    for furniture in world.content().furniture() {
        let id = Some(furniture.id.as_str());
        let recipe = |action: &str| {
            recipes
                .iter()
                .find(|r| r.action == action && r.furniture.as_deref() == id)
                .unwrap_or_else(|| panic!("no {action} recipe for {}", furniture.id))
        };
        assert!(recipe("smash").needs.is_empty(), "smashing needs no tool");
        // Bunks and lockers are bolted to the deck; the rest are screwed
        // together.
        let bolted = ["bunk", "locker"].contains(&furniture.id.as_str());
        let tool = if bolted { "WRENCH" } else { "SCREW" };
        let needs: Vec<(&str, u8)> = recipe("dismantle")
            .needs
            .iter()
            .map(|(q, &level)| (q.as_str(), level))
            .collect();
        assert_eq!(needs, [(tool, 1)], "{}", furniture.id);
        let most = |action| {
            recipe(action)
                .gives
                .iter()
                .map(|g| g.count.range().1)
                .sum::<u32>()
        };
        assert!(most("dismantle") >= most("smash"), "{}", furniture.id);
    }
}

#[test]
fn only_metal_patches_the_hull_or_mends_a_door() {
    // Metal scrap, or a hull patch kit, whose plate is hammered out of
    // metal scrap.
    let world = common::start_world(1);
    let made_from = |item: &str| -> Vec<(String, Option<String>)> {
        let craft = world.content().crafts().iter().find(|c| c.makes == item);
        let uses = craft
            .unwrap_or_else(|| panic!("nothing makes {item}"))
            .uses
            .iter();
        uses.map(|u| (u.item.clone(), u.tag.clone())).collect()
    };
    let metal = Some("metal".to_string());
    for recipe in world
        .content()
        .recipes()
        .iter()
        .filter(|r| r.action == "repair")
    {
        for used in &recipe.uses {
            let kit = used.item == "hull_patch_kit";
            assert!(kit || used.tag == metal, "{}", recipe.id);
        }
    }
    assert!(made_from("hull_patch_kit").contains(&("metal_plate".into(), None)));
    assert_eq!(made_from("metal_plate"), [("scrap".into(), metal)]);
    let bone = world.content().material_index("bone").unwrap();
    assert!(!world.materials()[usize::from(bone)].has_tag("metal"));
}

#[test]
fn dismantling_your_own_stasis_chamber_gives_steel_glass_padding_copper_and_its_cell() {
    let mut world = common::start_world(1);
    fetch_the_gear(&mut world); // the screwdriver too
    world.apply(Command::Move(Dir::East));
    world.apply(Command::Move(Dir::East));
    let yours = Pos::new(11, 6, 0);
    let before = world.seconds();
    world.apply(Command::Interact {
        at: yours,
        action: action(&world, "dismantle"),
    });
    assert_eq!(world.seconds() - before, 240, "four minutes");
    assert!(world.furniture_at(yours).is_none());
    assert!(
        world.furniture_at(Pos::new(12, 6, 0)).is_none(),
        "both halves"
    );
    let mut made_of: Vec<&str> = world
        .items_at(yours)
        .map(|i| world.materials()[usize::from(i.material)].id.as_str())
        .collect();
    made_of.dedup();
    assert_eq!(
        made_of,
        ["steel", "glass", "polyester", "copper", "aluminium_alloy"]
    );
    assert_eq!(world.items_at(yours).count(), 10);
    let cell = world.items_at(yours).last().unwrap();
    assert_eq!(world.item_types()[usize::from(cell.kind)].id, "power_cell");
    assert_eq!(cell.charge_j, 0, "the emergency cell, drained");
}

/// The number of the shipped craft with this id.
fn craft_number(world: &World, id: &str) -> u16 {
    let i = world.content().crafts().iter().position(|c| c.id == id);
    u16::try_from(i.unwrap_or_else(|| panic!("no craft {id:?}"))).unwrap()
}

#[test]
fn scrap_gives_you_a_shiv_and_the_shiv_cuts_polyester_into_rags() {
    let mut world = common::start_world(1);
    assert!(world.known_crafts().is_empty(), "you know nothing at first");
    // Pick up the four steel scraps at (9, 9): four crafts come to mind,
    // the shiv and rags, and wire and plate (which need a hammer).
    world.apply(Command::Move(Dir::SouthWest));
    world.apply(Command::Interact {
        at: Pos::new(9, 9, 0),
        action: action(&world, "pick_up"),
    });
    let (shiv, rags) = (craft_number(&world, "shiv"), craft_number(&world, "rags"));
    let (wire, plate) = (
        craft_number(&world, "wire"),
        craft_number(&world, "metal_plate"),
    );
    assert_eq!(world.known_crafts(), [shiv, rags, wire, plate]);
    let before = world.seconds();
    world.apply(Command::Craft(shiv));
    assert_eq!(world.seconds() - before, 300, "five minutes");
    assert_eq!(world.tool_level("CUT"), 1, "the shiv cuts");
    let made = world.carried().into_iter().map(|(_, item)| item);
    let shiv_kind = world.item_type_index("shiv").unwrap();
    let steel = world.content().material_index("steel").unwrap();
    assert!(
        made.clone()
            .any(|i| (i.kind, i.material) == (shiv_kind, steel))
    );
    // Up to the torn padding at (18, 2), and cut a scrap of it into rags.
    for (dir, steps) in [(Dir::North, 6), (Dir::East, 8)] {
        for _ in 0..steps {
            world.apply(Command::Move(dir));
        }
    }
    world.apply(Command::Interact {
        at: Pos::new(18, 2, 0),
        action: action(&world, "pick_up"),
    });
    world.apply(Command::Craft(rags));
    let rag = world.item_type_index("rag").unwrap();
    let polyester = world.content().material_index("polyester").unwrap();
    let carried = world.carried();
    let rags_made = carried
        .iter()
        .filter(|(_, i)| (i.kind, i.material) == (rag, polyester));
    assert_eq!(rags_made.count(), 2);
}

#[test]
fn the_basic_laser_cuts_the_side_room_door_into_a_damaged_door() {
    // Fetch the laser cutter from the gear locker, and from two tiles
    // away, cut the side room's door at (6, 7).
    use nighthawk_sim::Event;
    let mut world = common::start_world(1);
    fetch_the_gear(&mut world);
    let door = Pos::new(6, 7, 0);
    let before = world.seconds();
    let cut = action(&world, "cut");
    let events = world.apply(Command::Interact {
        at: door,
        action: cut,
    });
    let last = events.last().copied();
    assert!(
        matches!(last, Some(Event::Transformed { at, action, .. }) if at == door && action == cut)
    );
    assert_eq!(world.seconds() - before, 120, "two minutes");
    assert_eq!(world.structure_at(door).unwrap().id, "damaged_door");
    assert_eq!(world.material_at(door).unwrap().id, "aluminium_alloy");
    assert!(
        world.map().get(door).unwrap().is_walkable(),
        "walkable, airy"
    );
}

#[test]
fn two_scraps_repair_the_cut_side_room_door() {
    use nighthawk_sim::Event;
    let mut world = common::start_world(1);
    let door = Pos::new(6, 7, 0);
    let (pick_up, cut, repair) = (
        action(&world, "pick_up"),
        action(&world, "cut"),
        action(&world, "repair"),
    );
    // Fetch the laser, cut the door from two tiles away (as above), then
    // fetch the scrap at (9, 9) and come back to stand beside the door.
    fetch_the_gear(&mut world);
    world.apply(Command::Interact {
        at: door,
        action: cut,
    });
    world.apply(Command::Move(Dir::SouthEast));
    world.apply(Command::Interact {
        at: Pos::new(9, 9, 0),
        action: pick_up,
    });
    world.apply(Command::Move(Dir::NorthWest));
    world.apply(Command::Move(Dir::West));
    assert_eq!(world.player(), Pos::new(7, 7, 0));
    let scrap = world.item_type_index("scrap").unwrap();
    assert_eq!(world.carried_count(scrap), 4);

    let before = world.seconds();
    let events = world.apply(Command::Interact {
        at: door,
        action: repair,
    });
    let last = events.last().copied();
    assert!(
        matches!(last, Some(Event::Transformed { at, action, .. }) if at == door && action == repair)
    );
    assert_eq!(world.seconds() - before, 180, "three minutes");
    assert_eq!(world.carried_count(scrap), 2, "two scraps used");
    assert_eq!(world.structure_at(door).unwrap().id, "door");
    assert_eq!(
        world.material_at(door).unwrap().id,
        "aluminium_alloy",
        "a repaired door keeps its own material"
    );
    assert!(!world.map().get(door).unwrap().is_walkable(), "shut");
}
