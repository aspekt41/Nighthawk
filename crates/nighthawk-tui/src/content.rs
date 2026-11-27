//! Finding the `data/` folder and loading content files from it.
//!
//! The sim never touches files (decision D18), so reading from disk happens
//! here in the front-end. The sim only ever sees the files' text.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use nighthawk_sim::{Content, ContentError, ContentFile, LoadedMap, MapError, parse_map};

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
    /// A mistake in one of the content folders, like `data/materials/`
    /// (the error names the folder and the file).
    Content(ContentError),
    /// An action's key that clashes with one of the game's own keys.
    Keys(String),
}

/// Lets `?` turn a content mistake into a `LoadError`, which is what
/// `Content::load` needs to hand its errors back in our error type.
impl From<ContentError> for LoadError {
    fn from(error: ContentError) -> Self {
        LoadError::Content(error)
    }
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
            LoadError::Content(source) => write!(f, "data/{}: {source}", source.folder()),
            LoadError::Keys(message) => write!(f, "data/actions: {message}"),
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

/// Every `.json` file in the folder `dir`, sorted by name so the content
/// always loads in the same order.
fn read_folder(dir: &Path) -> Result<Vec<ContentFile>, LoadError> {
    let entries = fs::read_dir(dir).map_err(|source| LoadError::Read {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| {
            let name = p
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            Ok(ContentFile::new(name, read(p)?))
        })
        .collect()
}

/// Loads and checks every content folder in `data_dir` (`materials/`,
/// `structures/` and so on). The sim decides which folders there are; this
/// only says how to read one. Then checks the actions' keys fit this
/// front-end's own keys.
pub fn load_content(data_dir: &Path) -> Result<Content, LoadError> {
    let content = Content::load(|folder| read_folder(&data_dir.join(folder)))?;
    crate::keys::check_action_keys(content.actions()).map_err(LoadError::Keys)?;
    Ok(content)
}

/// Loads and checks `data/maps/<map_id>.json`, along with the structures
/// and materials its tiles are.
pub fn load_map(data_dir: &Path, map_id: &str) -> Result<LoadedMap, LoadError> {
    let content = load_content(data_dir)?;
    let path = data_dir.join("maps").join(format!("{map_id}.json"));
    let text = read(&path)?;
    parse_map(&text, &content).map_err(|source| LoadError::Map { path, source })
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

    /// A temporary data folder with copies of the real materials and
    /// structures in it.
    fn data_dir_with_content() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for folder in nighthawk_sim::CONTENT_FOLDERS {
            fs::create_dir(dir.path().join(folder)).unwrap();
            for entry in fs::read_dir(repo_data_dir().join(folder)).unwrap() {
                let from = entry.unwrap().path();
                fs::copy(
                    &from,
                    dir.path().join(folder).join(from.file_name().unwrap()),
                )
                .unwrap();
            }
        }
        dir
    }

    #[test]
    fn broken_map_file_names_the_file_in_the_error() {
        let dir = data_dir_with_content();
        fs::create_dir(dir.path().join("maps")).unwrap();
        fs::write(dir.path().join("maps/bad.json"), r#"{ "id": "bad" }"#).unwrap();

        let err = load_map(dir.path(), "bad").unwrap_err();

        assert!(matches!(err, LoadError::Map { .. }));
        assert!(err.to_string().contains("bad.json"));
    }

    #[test]
    fn a_missing_content_folder_is_a_read_error_naming_it() {
        let dir = tempfile::tempdir().unwrap();
        let missing = load_map(dir.path(), "aft_ring").unwrap_err();
        assert!(matches!(missing, LoadError::Read { .. }));
        let first = nighthawk_sim::CONTENT_FOLDERS[0];
        assert!(missing.to_string().contains(first), "{missing}");
    }

    #[test]
    fn broken_content_names_the_folder_and_the_file() {
        // Every content folder the sim asks for, one at a time.
        for folder in nighthawk_sim::CONTENT_FOLDERS {
            let dir = data_dir_with_content();
            fs::write(dir.path().join(folder).join("zz_broken.json"), "{}").unwrap();
            let err = load_map(dir.path(), "aft_ring").unwrap_err();
            assert!(matches!(err, LoadError::Content(_)), "{folder}: {err}");
            let message = err.to_string();
            assert!(
                message.starts_with(&format!(
                    "data/{folder}: invalid {folder} file zz_broken.json"
                )),
                "{message}"
            );
        }
    }

    #[test]
    fn an_action_key_that_clashes_with_a_game_key_stops_loading() {
        let dir = data_dir_with_content();
        let actions = dir.path().join("actions/basic.json");
        let text = fs::read_to_string(&actions).unwrap();
        fs::write(&actions, text.replace(r#""key": "x""#, r#""key": "h""#)).unwrap();
        let err = load_map(dir.path(), "aft_ring").unwrap_err();
        assert!(matches!(err, LoadError::Keys(_)));
        assert_eq!(
            err.to_string(),
            r#"data/actions: action "cut" uses the key 'h', which is a movement key"#
        );
    }

    #[test]
    fn only_json_files_are_content() {
        let dir = data_dir_with_content();
        fs::write(dir.path().join("materials/notes.txt"), "not json").unwrap();
        assert!(load_content(dir.path()).is_ok());
    }

    #[test]
    fn the_real_content_loads() {
        let content = load_content(&repo_data_dir()).unwrap();
        assert!(content.material_index("nanocarbon_alloy").is_some());
        assert!(content.structure_index("hull_plating").is_some());
        assert!(content.item_type_index("basic_laser_cutter").is_some());
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
