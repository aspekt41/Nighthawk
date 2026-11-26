//! Finding the `data/` folder and loading content files from it.
//!
//! The sim never touches files (decision D18), so reading from disk happens
//! here in the front-end. The sim only ever sees the files' text.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use nighthawk_sim::{MapError, World, parse_map};

/// The map a new game starts on, as a file name in `data/maps/` (no `.json`).
pub const START_MAP: &str = "stasis_bay";

/// Environment variable that points at a data folder somewhere else.
const DATA_ENV: &str = "NIGHTHAWK_DATA";

/// Anything that can stop the game from loading its content.
#[derive(Debug)]
pub enum LoadError {
    NoDataDir { tried: Vec<PathBuf> },
    Read { path: PathBuf, source: io::Error },
    Map { path: PathBuf, source: MapError },
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

/// Loads `data/maps/<map_id>.json` and starts a new game on it, with all
/// randomness coming from `seed`.
pub fn load_world(data_dir: &Path, map_id: &str, seed: u64) -> Result<World, LoadError> {
    let path = data_dir.join("maps").join(format!("{map_id}.json"));
    let text = fs::read_to_string(&path).map_err(|source| LoadError::Read {
        path: path.clone(),
        source,
    })?;
    let loaded = parse_map(&text).map_err(|source| LoadError::Map { path, source })?;
    Ok(World::new(loaded, seed))
}

/// Finds the data folder and loads the starting map.
pub fn load_start_world(seed: u64) -> Result<World, LoadError> {
    let candidates = data_dir_candidates();
    let dir = find_data_dir(&candidates).ok_or_else(|| LoadError::NoDataDir {
        tried: candidates.clone(),
    })?;
    load_world(dir, START_MAP, seed)
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
        let world = load_start_world(1).unwrap();
        assert_eq!(world.area_name(), "Stasis bay");
    }

    #[test]
    fn loads_the_real_starting_map() {
        let world = load_world(&repo_data_dir(), START_MAP, 1).unwrap();
        assert_eq!(world.area_name(), "Stasis bay");
        assert_eq!(world.turn(), 0);
    }

    #[test]
    fn missing_map_file_is_a_read_error() {
        let err = load_world(&repo_data_dir(), "no_such_map", 1).unwrap_err();
        assert!(matches!(err, LoadError::Read { .. }));
        assert!(err.to_string().contains("no_such_map.json"));
    }

    #[test]
    fn broken_map_file_names_the_file_in_the_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("maps")).unwrap();
        fs::write(dir.path().join("maps/bad.json"), r#"{ "id": "bad" }"#).unwrap();

        let err = load_world(dir.path(), "bad", 1).unwrap_err();

        assert!(matches!(err, LoadError::Map { .. }));
        assert!(err.to_string().contains("bad.json"));
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
