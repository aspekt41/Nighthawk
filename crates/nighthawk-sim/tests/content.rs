//! Content validation: every map file shipped in `data/maps` must load.
//!
//! This is an integration test (it lives in `tests/`, outside `src/`), so it
//! uses the crate only through its public API, the same way a front-end does.
//! It's also allowed to read files, unlike the sim itself.

use std::fs;
use std::path::PathBuf;

use std::collections::{HashSet, VecDeque};

use nighthawk_sim::{Dir, LoadedMap, Pos, Tile, Vertical, parse_map};

fn maps_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR is this crate's folder, crates/nighthawk-sim.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/maps")
}

#[test]
fn every_shipped_map_loads() {
    let mut checked = 0;
    for entry in fs::read_dir(maps_dir()).expect("data/maps exists") {
        let path = entry.expect("readable directory entry").path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let text = fs::read_to_string(&path).expect("readable map file");
            if let Err(e) = parse_map(&text) {
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
            let loaded = parse_map(&fs::read_to_string(&path).unwrap()).unwrap();
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
fn the_starting_map_is_enclosed_by_walls_on_every_deck() {
    // The starting room must not let the player walk off the map.
    let text = fs::read_to_string(maps_dir().join("aft_ring.json")).unwrap();
    let map = parse_map(&text).unwrap().map;
    for z in 0..map.depth() {
        for x in 0..map.width() {
            for y in [0, map.height() - 1] {
                assert_eq!(map.get(nighthawk_sim::Pos::new(x, y, z)), Some(Tile::Wall));
            }
        }
        for y in 0..map.height() {
            for x in [0, map.width() - 1] {
                assert_eq!(map.get(nighthawk_sim::Pos::new(x, y, z)), Some(Tile::Wall));
            }
        }
    }
}

/// Every map file in `data/maps`, loaded.
fn shipped_maps() -> Vec<(PathBuf, LoadedMap)> {
    let mut maps = Vec::new();
    for entry in fs::read_dir(maps_dir()).expect("data/maps exists") {
        let path = entry.expect("readable directory entry").path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let loaded = parse_map(&fs::read_to_string(&path).unwrap()).unwrap();
            maps.push((path, loaded));
        }
    }
    maps
}

/// Where the player can step to from `at`: the eight neighbours on the same
/// deck, plus the deck above or below when standing on stairs.
fn neighbours(map: &nighthawk_sim::Map, at: Pos) -> Vec<Pos> {
    let mut next: Vec<Pos> = Dir::ALL.iter().map(|&d| at.step(d)).collect();
    match map.get(at) {
        Some(Tile::StairsUp) => next.push(at.climb(Vertical::Up)),
        Some(Tile::StairsDown) => next.push(at.climb(Vertical::Down)),
        _ => {}
    }
    next.retain(|&p| map.get(p).is_some_and(Tile::is_walkable));
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
fn every_walkable_tile_can_be_reached_from_the_start() {
    // A flood fill from the player's start, following stairs between decks.
    // Anything it doesn't reach is a room the player could never enter.
    for (path, loaded) in shipped_maps() {
        let map = &loaded.map;
        let seen = reachable_from(map, loaded.player_start);
        let unreachable: Vec<Pos> = map
            .positions()
            .filter(|&p| map.get(p).is_some_and(Tile::is_walkable) && !seen.contains(&p))
            .collect();
        assert!(
            unreachable.is_empty(),
            "{}: {} walkable tiles can't be reached, e.g. {:?}",
            path.display(),
            unreachable.len(),
            unreachable.first()
        );
    }
}

#[test]
fn the_starting_map_has_more_than_one_deck() {
    let text = fs::read_to_string(maps_dir().join("aft_ring.json")).unwrap();
    let loaded = parse_map(&text).unwrap();
    assert!(loaded.map.depth() >= 2);
    assert_eq!(loaded.deck_names[0], "Stasis bay");
}
