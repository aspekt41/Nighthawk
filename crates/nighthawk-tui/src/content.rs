//! Finding the `data/` folder and loading content files from it.
//!
//! The sim never touches files (decision D18), so reading from disk happens
//! here in the front-end. The sim only ever sees the files' text.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use nighthawk_sim::{LoadedMap, MapError, MaterialError, Materials, parse_map, parse_materials};

/// The map a new game starts on, as a file name in `data/maps/` (no `.json`).
pub const START_MAP: &str = "aft_ring";

/// Environment variable that points at a data folder somewhere else.
const DATA_ENV: &str = "NIGHTHAWK_DATA";

/// Anything that can stop the game from loading its content.
#[derive(Debug)]
pub enum LoadError {
    NoDataDir {
        tried: Vec<PathBuf>,
    },
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Map {
        path: PathBuf,
        source: MapError,
    },
    Materials {
        path: PathBuf,
        source: MaterialError,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::NoDataDir { tried } => {
                write!(f, "could not find the data folder; looked in:")?;
                for path in tried {
                    write!(f, "\n  {}", path.display())?;
                }
                write!(f, "\nSet {DATA_ENV} to the folder that holds maps/.")
            }
            LoadError::Read { path, source } => {
                write!(f, "could not read {}: {source}", path.display())
            }
            LoadError::Map { path, source } => write!(f, "{}: {source}", path.display()),
            LoadError::Materials { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for LoadError {}

/// Where to look for the data folder, most specific first:
/// 1. `$NIGHTHAWK_DATA`, if set
/// 2. `./data`, for running from the repository root
/// 3. the repository's `data/` as it was at build time, so `cargo run` works
///    from any directory during development
pub fn data_dir_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::var_os(DATA_ENV) {
        candidates.push(PathBuf::from(dir));
    }
    candidates.push(PathBuf::from("data"));
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"));
    candidates
}

/// The first candidate that is an existing directory.
pub fn find_data_dir(candidates: &[PathBuf]) -> Option<&Path> {
    candidates.iter().map(PathBuf::as_path).find(|p| p.is_dir())
}

/// Reads a file's text, with an error that names the file.
fn read(path: &Path) -> Result<String, LoadError> {
    fs::read_to_string(path).map_err(|source| LoadError::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// Loads and checks `data/materials.json`.
pub fn load_materials(data_dir: &Path) -> Result<Materials, LoadError> {
    let path = data_dir.join("materials.json");
    let text = read(&path)?;
    parse_materials(&text).map_err(|source| LoadError::Materials { path, source })
}

/// Loads and checks `data/maps/<map_id>.json`, along with the materials
/// its tiles are made of.
pub fn load_map(data_dir: &Path, map_id: &str) -> Result<LoadedMap, LoadError> {
    let materials = load_materials(data_dir)?;
    let path = data_dir.join("maps").join(format!("{map_id}.json"));
    let text = read(&path)?;
    parse_map(&text, &materials).map_err(|source| LoadError::Map { path, source })
}

/// Finds the data folder and loads the map every new game starts on.
pub fn load_start_map() -> Result<LoadedMap, LoadError> {
    let candidates = data_dir_candidates();
    let dir = find_data_dir(&candidates).ok_or_else(|| LoadError::NoDataDir {
        tried: candidates.clone(),
    })?;
    load_map(dir, START_MAP)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_data_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    #[test]
    fn first_existing_directory_wins() {
        let real = repo_data_dir();
        let candidates = vec![
            PathBuf::from("/definitely/not/here"),
            real.clone(),
            std::env::temp_dir(),
        ];
        assert_eq!(find_data_dir(&candidates), Some(real.as_path()));
    }

    #[test]
    fn no_existing_directory_gives_none() {
        let candidates = vec![PathBuf::from("/definitely/not/here")];
        assert_eq!(find_data_dir(&candidates), None);
    }

    #[test]
    fn files_are_not_data_directories() {
        let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert_eq!(find_data_dir(&[file]), None);
    }

    #[test]
    fn candidates_end_with_local_then_build_time_data_dirs() {
        let candidates = data_dir_candidates();
        let n = candidates.len();
        assert!(n >= 2);
        assert_eq!(candidates[n - 2], PathBuf::from("data"));
        assert_eq!(candidates[n - 1], repo_data_dir());
    }

    #[test]
    fn the_start_world_loads_from_the_default_locations() {
        let map = load_start_map().unwrap();
        assert_eq!(map.name, "Aft habitation ring");
        assert_eq!(map.deck_names[0], "Stasis bay");
    }

    #[test]
    fn loads_the_real_starting_map() {
        let world = nighthawk_sim::World::new(load_map(&repo_data_dir(), START_MAP).unwrap(), 1);
        assert_eq!(world.area_name(), "Stasis bay");
        assert_eq!(world.seconds(), 0);
    }

    #[test]
    fn missing_map_file_is_a_read_error() {
        let err = load_map(&repo_data_dir(), "no_such_map").unwrap_err();
        assert!(matches!(err, LoadError::Read { .. }));
        assert!(err.to_string().contains("no_such_map.json"));
    }

    /// A temporary data folder with the real materials file in it.
    fn data_dir_with_materials() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::copy(
            repo_data_dir().join("materials.json"),
            dir.path().join("materials.json"),
        )
        .unwrap();
        dir
    }

    #[test]
    fn broken_map_file_names_the_file_in_the_error() {
        let dir = data_dir_with_materials();
        fs::create_dir(dir.path().join("maps")).unwrap();
        fs::write(dir.path().join("maps/bad.json"), r#"{ "id": "bad" }"#).unwrap();

        let err = load_map(dir.path(), "bad").unwrap_err();

        assert!(matches!(err, LoadError::Map { .. }));
        assert!(err.to_string().contains("bad.json"));
    }

    #[test]
    fn missing_or_broken_materials_name_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = load_map(dir.path(), "aft_ring").unwrap_err();
        assert!(matches!(missing, LoadError::Read { .. }));
        assert!(missing.to_string().contains("materials.json"));

        fs::write(dir.path().join("materials.json"), "{}").unwrap();
        let broken = load_map(dir.path(), "aft_ring").unwrap_err();
        assert!(matches!(broken, LoadError::Materials { .. }));
        assert!(
            broken
                .to_string()
                .contains("materials.json: invalid materials JSON")
        );
    }

    #[test]
    fn the_real_materials_load() {
        let materials = load_materials(&repo_data_dir()).unwrap();
        assert!(materials.index_of("nanocarbon_alloy").is_some());
    }

    #[test]
    fn missing_data_dir_lists_where_it_looked() {
        let err = LoadError::NoDataDir {
            tried: vec![PathBuf::from("/a"), PathBuf::from("/b")],
        };
        let msg = err.to_string();
        assert!(msg.contains("/a") && msg.contains("/b") && msg.contains(DATA_ENV));
    }
}
