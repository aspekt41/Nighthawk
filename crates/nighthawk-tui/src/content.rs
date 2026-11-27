//! Finding the `data/` folder and loading content files from it.
//!
//! The sim never touches files (decision D18), so reading from disk happens
//! here in the front-end. The sim only ever sees the files' text.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use nighthawk_sim::{
    Catalogue, ContentFile, ItemError, ItemTypes, LoadedMap, MapError, MaterialError, Materials,
    RecipeError, Recipes, StructureError, Structures, parse_items, parse_map, parse_materials,
    parse_recipes, parse_structures,
};

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
    /// A mistake in `data/materials/` (the error names the file).
    Materials {
        path: PathBuf,
        source: MaterialError,
    },
    /// A mistake in `data/structures/` (the error names the file).
    Structures {
        path: PathBuf,
        source: StructureError,
    },
    /// A mistake in `data/items/` (the error names the file).
    Items {
        path: PathBuf,
        source: ItemError,
    },
    /// A mistake in `data/recipes/` (the error names the file).
    Recipes {
        path: PathBuf,
        source: RecipeError,
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
            LoadError::Structures { path, source } => write!(f, "{}: {source}", path.display()),
            LoadError::Items { path, source } => write!(f, "{}: {source}", path.display()),
            LoadError::Recipes { path, source } => write!(f, "{}: {source}", path.display()),
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

/// All the content a map can refer to.
pub struct Content {
    pub materials: Materials,
    pub structures: Structures,
    pub items: ItemTypes,
    pub recipes: Recipes,
}

impl Content {
    /// The content as the sim's `Catalogue`, which borrows each list.
    pub fn catalogue(&self) -> Catalogue<'_> {
        Catalogue {
            materials: &self.materials,
            structures: &self.structures,
            items: &self.items,
            recipes: &self.recipes,
        }
    }
}

/// Loads and checks `data/materials/`, `data/structures/`, `data/items/`
/// and `data/recipes/`.
pub fn load_content(data_dir: &Path) -> Result<Content, LoadError> {
    let path = data_dir.join("materials");
    let materials = parse_materials(&read_folder(&path)?)
        .map_err(|source| LoadError::Materials { path, source })?;
    let path = data_dir.join("structures");
    let structures = parse_structures(&read_folder(&path)?, &materials)
        .map_err(|source| LoadError::Structures { path, source })?;
    let path = data_dir.join("items");
    let items = parse_items(&read_folder(&path)?, &materials)
        .map_err(|source| LoadError::Items { path, source })?;
    let path = data_dir.join("recipes");
    let recipes = parse_recipes(&read_folder(&path)?, &structures, &items)
        .map_err(|source| LoadError::Recipes { path, source })?;
    Ok(Content {
        materials,
        structures,
        items,
        recipes,
    })
}

/// Loads and checks `data/maps/<map_id>.json`, along with the structures
/// and materials its tiles are.
pub fn load_map(data_dir: &Path, map_id: &str) -> Result<LoadedMap, LoadError> {
    let content = load_content(data_dir)?;
    let path = data_dir.join("maps").join(format!("{map_id}.json"));
    let text = read(&path)?;
    parse_map(&text, content.catalogue()).map_err(|source| LoadError::Map { path, source })
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
        for folder in ["materials", "structures", "items", "recipes"] {
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
        assert!(missing.to_string().contains("materials"));
    }

    #[test]
    fn broken_content_names_the_folder_and_the_file() {
        let dir = data_dir_with_content();
        fs::write(dir.path().join("materials/zz_broken.json"), "{}").unwrap();
        let err = load_map(dir.path(), "aft_ring").unwrap_err();
        assert!(matches!(err, LoadError::Materials { .. }));
        assert!(
            err.to_string()
                .contains("invalid materials file zz_broken.json"),
            "{err}"
        );

        let dir = data_dir_with_content();
        fs::write(dir.path().join("structures/zz_broken.json"), "{}").unwrap();
        let err = load_map(dir.path(), "aft_ring").unwrap_err();
        assert!(matches!(err, LoadError::Structures { .. }));
        assert!(
            err.to_string()
                .contains("invalid structures file zz_broken.json"),
            "{err}"
        );

        let dir = data_dir_with_content();
        fs::write(dir.path().join("items/zz_broken.json"), "{}").unwrap();
        let err = load_map(dir.path(), "aft_ring").unwrap_err();
        assert!(matches!(err, LoadError::Items { .. }));
        assert!(
            err.to_string()
                .contains("invalid items file zz_broken.json"),
            "{err}"
        );

        let dir = data_dir_with_content();
        fs::write(dir.path().join("recipes/zz_broken.json"), "{}").unwrap();
        let err = load_map(dir.path(), "aft_ring").unwrap_err();
        assert!(matches!(err, LoadError::Recipes { .. }));
        assert!(
            err.to_string()
                .contains("invalid recipes file zz_broken.json"),
            "{err}"
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
        assert!(content.materials.index_of("nanocarbon_alloy").is_some());
        assert!(content.structures.index_of("hull_plating").is_some());
        assert!(content.items.index_of("basic_laser_cutter").is_some());
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
