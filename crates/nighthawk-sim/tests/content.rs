//! Content validation: every map file shipped in `data/maps` must load.
//!
//! This is an integration test (it lives in `tests/`, outside `src/`), so it
//! uses the crate only through its public API, the same way a front-end does.
//! It's also allowed to read files, unlike the sim itself.

mod common;

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;

use nighthawk_sim::{Dir, LoadedMap, MapError, Pos, Quality, Tile, Vertical, parse_map};

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
    let map = load(&text).unwrap().map;
    let on_edge =
        |p: &Pos| p.x == 0 || p.y == 0 || p.x == map.width() - 1 || p.y == map.height() - 1;
    for p in map.positions().filter(on_edge) {
        let tile = map.get(p).unwrap();
        assert!(!tile.is_passable(), "{p:?} is a {}", tile.name());
    }
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
fn neighbours(map: &nighthawk_sim::Map, at: Pos) -> Vec<Pos> {
    let mut next: Vec<Pos> = Dir::ALL.iter().map(|&d| at.step(d)).collect();
    match map.get(at) {
        Some(Tile::StairsUp) => next.push(at.climb(Vertical::Up)),
        Some(Tile::StairsDown) => next.push(at.climb(Vertical::Down)),
        _ => {}
    }
    next.retain(|&p| map.get(p).is_some_and(Tile::is_passable));
    next
}

/// Every tile the player can get to from `start`, by a breadth-first flood
/// fill: visit a tile, queue its unvisited neighbours, repeat until done.
fn reachable_from(map: &nighthawk_sim::Map, start: Pos) -> HashSet<Pos> {
    let mut seen = HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(at) = queue.pop_front() {
        let new: Vec<Pos> = neighbours(map, at)
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
        let map = &loaded.map;
        let seen = reachable_from(map, loaded.player_start);
        let unreachable: Vec<Pos> = map
            .positions()
            .filter(|&p| map.get(p).is_some_and(Tile::is_passable) && !seen.contains(&p))
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
    used.extend(content.recipes().iter().map(|r| r.to.as_str()));
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
        content.materials()[i].cut_by.get(&Quality::Laser).copied()
    };
    assert_eq!(level("aluminium_alloy"), Some(1));
    assert_eq!(level("steel"), Some(2));
    assert_eq!(level("nanocarbon_alloy"), Some(5));
}

#[test]
fn you_start_in_a_jumpsuit_with_four_one_litre_pockets() {
    let world = common::start_world(1);
    let worn = world.worn();
    assert_eq!(worn.len(), 1);
    let jumpsuit = &world.item_types()[usize::from(worn[0].kind)];
    assert_eq!(jumpsuit.id, "jumpsuit");
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
fn a_laser_cutter_and_four_steel_scraps_lie_near_the_start() {
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
            ("basic_laser_cutter", "aluminium_alloy", 1080),
            ("scrap", "steel", 785),
            ("scrap", "steel", 785),
            ("scrap", "steel", 785),
            ("scrap", "steel", 785),
        ]
    );
    let laser = &world.item_types()[usize::from(world.floor_items()[0].item.kind)];
    assert_eq!(laser.qualities.get(&Quality::Laser), Some(&1));
}

#[test]
fn the_basic_laser_cuts_the_side_room_door_into_a_damaged_door() {
    // Pick up the laser cutter two steps east, walk back to two tiles from
    // the side room's door at (6, 7), and cut it.
    use nighthawk_sim::{Action, Command, Event};
    let mut world = common::start_world(1);
    let laser = Pos::new(12, 7, 0);
    world.apply(Command::Move(Dir::East));
    world.apply(Command::Interact {
        at: laser,
        action: Action::PickUp,
    });
    for _ in 0..3 {
        world.apply(Command::Move(Dir::West));
    }
    assert_eq!(world.player(), Pos::new(8, 7, 0));
    let door = Pos::new(6, 7, 0);
    let before = world.seconds();
    let events = world.apply(Command::Interact {
        at: door,
        action: Action::Cut,
    });
    let last = events.last().copied();
    assert!(matches!(last, Some(Event::CutThrough { at, .. }) if at == door));
    assert_eq!(world.seconds() - before, 120, "two minutes");
    assert_eq!(world.structure_at(door).unwrap().id, "damaged_door");
    assert_eq!(world.material_at(door).unwrap().id, "aluminium_alloy");
    assert_eq!(world.map().get(door), Some(Tile::Floor), "walkable, airy");
}

#[test]
fn two_scraps_repair_the_cut_side_room_door() {
    use nighthawk_sim::{Action, Command, Event};
    let mut world = common::start_world(1);
    let door = Pos::new(6, 7, 0);
    // Pick up the laser, cut the door from two tiles away (as above), then
    // fetch the scrap at (9, 9) and come back to stand beside the door.
    world.apply(Command::Move(Dir::East));
    world.apply(Command::Interact {
        at: Pos::new(12, 7, 0),
        action: Action::PickUp,
    });
    for _ in 0..3 {
        world.apply(Command::Move(Dir::West));
    }
    world.apply(Command::Interact {
        at: door,
        action: Action::Cut,
    });
    world.apply(Command::Move(Dir::SouthEast));
    world.apply(Command::Interact {
        at: Pos::new(9, 9, 0),
        action: Action::PickUp,
    });
    world.apply(Command::Move(Dir::NorthWest));
    world.apply(Command::Move(Dir::West));
    assert_eq!(world.player(), Pos::new(7, 7, 0));
    let scrap = world.item_type_index("scrap").unwrap();
    assert_eq!(world.carried_count(scrap), 4);

    let before = world.seconds();
    let events = world.apply(Command::Interact {
        at: door,
        action: Action::Repair,
    });
    let last = events.last().copied();
    assert!(matches!(last, Some(Event::Repaired { at, .. }) if at == door));
    assert_eq!(world.seconds() - before, 180, "three minutes");
    assert_eq!(world.carried_count(scrap), 2, "two scraps used");
    assert_eq!(world.structure_at(door).unwrap().id, "door");
    assert_eq!(
        world.material_at(door).unwrap().id,
        "aluminium_alloy",
        "a repaired door keeps its own material"
    );
    assert_eq!(world.map().get(door), Some(Tile::DoorClosed));
}
