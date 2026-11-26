//! Content validation: every map file shipped in `data/maps` must load.
//!
//! This is an integration test (it lives in `tests/`, outside `src/`), so it
//! uses the crate only through its public API, the same way a front-end does.
//! It's also allowed to read files, unlike the sim itself.

use std::fs;
use std::path::PathBuf;

use std::collections::{HashSet, VecDeque};

use nighthawk_sim::{
    Dir, LoadedMap, Materials, Pos, Quality, Tile, Vertical, parse_map, parse_materials,
};

fn maps_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR is this crate's folder, crates/nighthawk-sim.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/maps")
}

/// The shipped `data/materials.json`, which every map is checked against.
fn materials() -> Materials {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/materials.json");
    let text = fs::read_to_string(path).expect("data/materials.json is readable");
    parse_materials(&text).unwrap_or_else(|e| panic!("data/materials.json: {e}"))
}

#[test]
fn every_shipped_map_loads() {
    let mut checked = 0;
    for entry in fs::read_dir(maps_dir()).expect("data/maps exists") {
        let path = entry.expect("readable directory entry").path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let text = fs::read_to_string(&path).expect("readable map file");
            if let Err(e) = parse_map(&text, &materials()) {
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
            let loaded = parse_map(&fs::read_to_string(&path).unwrap(), &materials()).unwrap();
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
    let map = parse_map(&text, &materials()).unwrap().map;
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
    let map = parse_map(&text, &materials()).unwrap().map;
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
            let loaded = parse_map(&fs::read_to_string(&path).unwrap(), &materials()).unwrap();
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
    let loaded = parse_map(&text, &materials()).unwrap();
    assert!(loaded.map.depth() >= 2);
    assert_eq!(loaded.deck_names[0], "Stasis bay");
}

#[test]
fn the_hull_is_nanocarbon_and_the_inside_is_not() {
    // Every solid tile on the map's border is outer hull, made of
    // Nanocarbon Alloy; nothing inside the ship is.
    let text = fs::read_to_string(maps_dir().join("aft_ring.json")).unwrap();
    let loaded = parse_map(&text, &materials()).unwrap();
    let (map, list) = (&loaded.map, &loaded.materials);
    let made_of = |p: Pos| {
        list[usize::from(map.material_index(p).unwrap())]
            .id
            .as_str()
    };
    for p in map.positions() {
        let on_edge = p.x == 0 || p.y == 0 || p.x == map.width() - 1 || p.y == map.height() - 1;
        let hull = made_of(p) == "nanocarbon_alloy";
        assert_eq!(on_edge, hull, "{p:?} is {}", made_of(p));
    }
}

#[test]
fn the_shipped_materials_have_the_agreed_laser_levels() {
    // Levels are set by hand (decision D35). This test is the list to
    // update on purpose when they're retuned.
    let materials = materials();
    let level = |id: &str| {
        let i = usize::from(materials.index_of(id).unwrap());
        materials.list()[i].cut_by.get(&Quality::Laser).copied()
    };
    assert_eq!(level("aluminium_alloy"), Some(1));
    assert_eq!(level("steel"), Some(2));
    assert_eq!(level("nanocarbon_alloy"), Some(5));
}
