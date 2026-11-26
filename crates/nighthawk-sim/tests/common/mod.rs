//! Helpers shared by the integration tests: loading the real content in
//! `data/`. Integration tests may read files; the sim itself never does.
//!
//! Each file in `tests/` is compiled as its own crate, and a file in a
//! sub-folder like this one is the Rust convention for code they share
//! (`mod common;` in each test file brings it in). Every test file calls
//! `start_world`, which uses all the rest, so nothing here goes unused.

use std::fs;
use std::path::{Path, PathBuf};

use nighthawk_sim::{
    Catalogue, ContentFile, LoadedMap, Materials, Structures, World, parse_map, parse_materials,
    parse_structures,
};

/// The repository's `data/` folder.
pub fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

/// Every `.json` file in `data/<folder>/`, sorted by name.
pub fn read_folder(folder: &str) -> Vec<ContentFile> {
    let dir = data_dir().join(folder);
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| ContentFile::new(file_name(p), fs::read_to_string(p).unwrap()))
        .collect()
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

/// The shipped materials and structures.
pub fn catalogue() -> (Materials, Structures) {
    let materials = parse_materials(&read_folder("materials"))
        .unwrap_or_else(|e| panic!("data/materials: {e}"));
    let structures = parse_structures(&read_folder("structures"), &materials)
        .unwrap_or_else(|e| panic!("data/structures: {e}"));
    (materials, structures)
}

/// Parses a map's text against the shipped content.
pub fn parse(text: &str) -> LoadedMap {
    let (materials, structures) = catalogue();
    let catalogue = Catalogue {
        materials: &materials,
        structures: &structures,
    };
    parse_map(text, catalogue).unwrap_or_else(|e| panic!("map: {e}"))
}

/// A new game on the starting map.
pub fn start_world(seed: u64) -> World {
    let text = fs::read_to_string(data_dir().join("maps/aft_ring.json"))
        .expect("starting map is readable");
    World::new(parse(&text), seed)
}
