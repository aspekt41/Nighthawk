//! Where the save file lives, and reading and writing it (decision D23).
//!
//! The sim turns a `World` into text and back (`save_to_string` and
//! `load_from_str`). This module puts that text on disk:
//!
//! | Platform | Save folder |
//! |---|---|
//! | Linux | `$XDG_DATA_HOME/nighthawk/`, usually `~/.local/share/nighthawk/` |
//! | macOS | `~/Library/Application Support/nighthawk/` |
//! | Anywhere | `$NIGHTHAWK_SAVE_DIR`, if set |
//!
//! There's one save slot, `save.json`, in that folder.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use nighthawk_sim::{SaveError, World, load_from_str, save_to_string};

/// Environment variable that overrides the save folder.
pub const SAVE_ENV: &str = "NIGHTHAWK_SAVE_DIR";

const SAVE_FILE: &str = "save.json";

/// The save folder for this machine, or `None` if there's no way to tell
/// (no home directory and no override). The game still runs then, just
/// without saving.
pub fn default_save_dir() -> Option<PathBuf> {
    save_dir_from(|name| std::env::var_os(name), cfg!(target_os = "macos"))
}

/// The save-folder rules, written as a pure function of the environment so
/// they can be tested without changing real environment variables.
/// `env` looks a variable up; `macos` says which platform's rules to use.
pub fn save_dir_from(env: impl Fn(&str) -> Option<OsString>, macos: bool) -> Option<PathBuf> {
    // An empty variable counts as unset, which is what shells usually mean.
    let var = |name: &str| {
        env(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };

    if let Some(dir) = var(SAVE_ENV) {
        return Some(dir);
    }
    if macos {
        return var("HOME").map(|home| home.join("Library/Application Support/nighthawk"));
    }
    if let Some(data_home) = var("XDG_DATA_HOME") {
        return Some(data_home.join("nighthawk"));
    }
    var("HOME").map(|home| home.join(".local/share/nighthawk"))
}

/// Anything that can go wrong reading or writing the save file. Both
/// variants name the file, so the player knows exactly which one to look at.
#[derive(Debug)]
pub enum SaveFileError {
    Io { path: PathBuf, source: io::Error },
    Format { path: PathBuf, source: SaveError },
}

impl fmt::Display for SaveFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveFileError::Io { path, source } => write!(f, "{}: {source}", path.display()),
            SaveFileError::Format { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for SaveFileError {}

/// The one save slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveSlot {
    path: PathBuf,
}

impl SaveSlot {
    /// The slot inside `dir`. Nothing is touched on disk until you save.
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            path: dir.join(SAVE_FILE),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether anything has been saved here (the title menu's "Continue").
    pub fn exists(&self) -> bool {
        self.path.is_file()
    }

    /// The saved game, or `Ok(None)` if nothing has been saved yet.
    pub fn load(&self) -> Result<Option<World>, SaveFileError> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(self.io_error(source)),
        };
        load_from_str(&text)
            .map(Some)
            .map_err(|source| SaveFileError::Format {
                path: self.path.clone(),
                source,
            })
    }

    /// Writes `world` to the slot, creating the folder if needed.
    ///
    /// It writes to `save.json.tmp` first, then renames that over
    /// `save.json`. A rename is all-or-nothing, so if the game crashes or the
    /// disk fills up halfway, the old save is still intact.
    pub fn save(&self, world: &World) -> Result<(), SaveFileError> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|source| self.io_error(source))?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, save_to_string(world)).map_err(|source| self.io_error(source))?;
        fs::rename(&tmp, &self.path).map_err(|source| self.io_error(source))
    }

    fn io_error(&self, source: io::Error) -> SaveFileError {
        SaveFileError::Io {
            path: self.path.clone(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::load_start_map;
    use nighthawk_sim::Command;

    /// A fake environment: looks names up in a fixed list.
    fn env_of(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| {
            vars.iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| OsString::from(v))
        }
    }

    fn a_world() -> World {
        World::new(load_start_map().unwrap(), 5)
    }

    #[test]
    fn the_override_wins_everywhere() {
        let env = env_of(&[
            (SAVE_ENV, "/tmp/nh"),
            ("HOME", "/home/a"),
            ("XDG_DATA_HOME", "/x"),
        ]);
        assert_eq!(save_dir_from(&env, false), Some(PathBuf::from("/tmp/nh")));
        assert_eq!(save_dir_from(&env, true), Some(PathBuf::from("/tmp/nh")));
    }

    #[test]
    fn linux_uses_xdg_data_home_then_home() {
        let xdg = env_of(&[("HOME", "/home/a"), ("XDG_DATA_HOME", "/data")]);
        assert_eq!(
            save_dir_from(xdg, false),
            Some(PathBuf::from("/data/nighthawk"))
        );
        let home = env_of(&[("HOME", "/home/a")]);
        assert_eq!(
            save_dir_from(home, false),
            Some(PathBuf::from("/home/a/.local/share/nighthawk"))
        );
    }

    #[test]
    fn macos_uses_application_support() {
        let env = env_of(&[("HOME", "/Users/a"), ("XDG_DATA_HOME", "/ignored")]);
        assert_eq!(
            save_dir_from(env, true),
            Some(PathBuf::from(
                "/Users/a/Library/Application Support/nighthawk"
            ))
        );
    }

    #[test]
    fn empty_variables_count_as_unset() {
        let env = env_of(&[(SAVE_ENV, ""), ("XDG_DATA_HOME", ""), ("HOME", "/home/a")]);
        assert_eq!(
            save_dir_from(env, false),
            Some(PathBuf::from("/home/a/.local/share/nighthawk"))
        );
    }

    #[test]
    fn no_home_means_no_save_dir() {
        assert_eq!(save_dir_from(env_of(&[]), false), None);
        assert_eq!(save_dir_from(env_of(&[]), true), None);
    }

    #[test]
    fn a_normal_machine_has_a_save_dir() {
        // Linux and macOS (including CI machines) always set HOME.
        assert!(default_save_dir().is_some());
    }

    #[test]
    fn an_empty_slot_loads_as_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let slot = SaveSlot::in_dir(dir.path());
        assert!(!slot.exists());
        assert_eq!(slot.load().unwrap(), None);
    }

    #[test]
    fn a_saved_game_loads_back_identical() {
        let dir = tempfile::tempdir().unwrap();
        let slot = SaveSlot::in_dir(dir.path());
        let mut world = a_world();
        world.apply(Command::Wait);

        slot.save(&world).unwrap();

        assert!(slot.exists());
        assert_eq!(slot.load().unwrap(), Some(world));
    }

    #[test]
    fn saving_creates_missing_folders_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let slot = SaveSlot::in_dir(&dir.path().join("a/b/c"));
        slot.save(&a_world()).unwrap();
        assert!(slot.path().is_file());
        assert!(!slot.path().with_extension("json.tmp").exists());
    }

    #[test]
    fn saving_again_replaces_the_old_save() {
        let dir = tempfile::tempdir().unwrap();
        let slot = SaveSlot::in_dir(dir.path());
        let mut world = a_world();
        slot.save(&world).unwrap();
        world.apply(Command::Wait);
        slot.save(&world).unwrap();
        assert_eq!(
            slot.load().unwrap().map(|w| w.seconds()),
            Some(nighthawk_sim::WAIT_SECONDS)
        );
    }

    #[test]
    fn a_damaged_save_is_an_error_naming_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let slot = SaveSlot::in_dir(dir.path());
        fs::write(slot.path(), "{ broken").unwrap();

        let err = slot.load().unwrap_err();

        assert!(matches!(err, SaveFileError::Format { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("save.json") && msg.contains("damaged"),
            "{msg}"
        );
    }

    #[test]
    fn an_unwritable_folder_is_an_io_error_naming_the_file() {
        // A *file* where the save folder should be: creating the folder fails.
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("not_a_dir");
        fs::write(&blocker, "").unwrap();
        let slot = SaveSlot::in_dir(&blocker);

        let err = slot.save(&a_world()).unwrap_err();

        assert!(matches!(err, SaveFileError::Io { .. }));
        assert!(err.to_string().contains("save.json"));
    }

    #[test]
    fn an_unreadable_save_is_an_io_error() {
        // A *folder* named save.json: it exists but can't be read as a file.
        let dir = tempfile::tempdir().unwrap();
        let slot = SaveSlot::in_dir(dir.path());
        fs::create_dir(slot.path()).unwrap();
        assert!(matches!(slot.load(), Err(SaveFileError::Io { .. })));
    }
}
