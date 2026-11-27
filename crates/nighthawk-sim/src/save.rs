//! Turning a [`World`] into save-file text and back (decision D23).
//!
//! A save file is JSON. It's readable on purpose, so you can open one and see
//! exactly what was saved:
//!
//! ```json
//! {
//!   "format": "nighthawk-save",
//!   "version": 12,
//!   "world": { "deck_names": ["Stasis bay"], "seconds": 36, ... }
//! }
//! ```
//!
//! Like the rest of the sim, this module does no file I/O: it works on text.
//! Writing the text to disk is the front-end's job.
//!
//! ## Versions
//!
//! `version` is bumped whenever the shape of the saved world changes. Loading
//! reads the header first, so a save from another version gets a clear
//! "unsupported version" error instead of a confusing parse error. Before
//! 1.0 we don't migrate old saves; once players have saves worth keeping,
//! migrations go here.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::world::World;

/// The `format` tag every save file starts with.
pub const SAVE_FORMAT: &str = "nighthawk-save";

/// The save layout this build reads and writes. Bump it when `World` (or
/// anything inside it) changes shape, and update the save-format test.
pub const SAVE_VERSION: u32 = 12;

/// Everything that can be wrong with a save file.
#[derive(Debug)]
pub enum SaveError {
    /// Not valid JSON, or valid JSON in the wrong shape.
    Json(serde_json::Error),
    /// Valid JSON, but not a Nighthawk save (wrong or missing `format`).
    NotASave,
    /// A save from a different version of the game.
    UnsupportedVersion { found: u32 },
    /// Parsed fine, but breaks a rule the game relies on, for example a
    /// player standing inside a wall. Usually means the file was edited.
    Inconsistent(String),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Json(e) => write!(f, "save file is damaged: {e}"),
            SaveError::NotASave => write!(f, "this is not a Nighthawk save file"),
            SaveError::UnsupportedVersion { found } => write!(
                f,
                "save file is version {found}, but this build only reads version {SAVE_VERSION}"
            ),
            SaveError::Inconsistent(why) => write!(f, "save file is inconsistent: {why}"),
        }
    }
}

impl std::error::Error for SaveError {}

/// Just the header, read first. It doesn't use `deny_unknown_fields`,
/// because its whole job is to look at two fields and ignore the rest.
#[derive(Deserialize)]
struct Header {
    format: Option<String>,
    version: Option<u32>,
}

/// The whole file, for writing. It borrows the world (`&'a World`) so saving
/// doesn't need to copy it.
#[derive(Serialize)]
struct SaveFileOut<'a> {
    format: &'static str,
    version: u32,
    world: &'a World,
}

/// The whole file, for reading. `format` and `version` were already checked
/// by [`check_header`]; they're listed here only so `deny_unknown_fields`
/// accepts them. A leading `_` tells Rust "deliberately unused", and
/// `rename` keeps the JSON names without the underscore.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveFileIn {
    #[serde(rename = "format")]
    _format: String,
    #[serde(rename = "version")]
    _version: u32,
    world: World,
}

/// The save-file text for `world`.
pub fn save_to_string(world: &World) -> String {
    let file = SaveFileOut {
        format: SAVE_FORMAT,
        version: SAVE_VERSION,
        world,
    };
    // Serialising our own plain data types can't fail: there are no maps with
    // non-string keys and no custom serialisers that return errors.
    serde_json::to_string_pretty(&file).expect("a World always serialises")
}

/// Reads a save file's text back into a world.
pub fn load_from_str(text: &str) -> Result<World, SaveError> {
    check_header(text)?;
    let file: SaveFileIn = serde_json::from_str(text).map_err(SaveError::Json)?;
    file.world
        .check_consistent()
        .map_err(SaveError::Inconsistent)?;
    Ok(file.world)
}

/// Rejects files that aren't saves, or are saves from another version,
/// before trying to read the world out of them.
fn check_header(text: &str) -> Result<(), SaveError> {
    let header: Header = serde_json::from_str(text).map_err(SaveError::Json)?;
    if header.format.as_deref() != Some(SAVE_FORMAT) {
        return Err(SaveError::NotASave);
    }
    match header.version {
        Some(SAVE_VERSION) => Ok(()),
        Some(found) => Err(SaveError::UnsupportedVersion { found }),
        None => Err(SaveError::NotASave),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map_file::parse_map_for_tests as parse_map;
    use crate::pos::Dir;
    use crate::world::Command;

    /// A 4×3 room. Built with `format!` from rows, because raw strings can't
    /// hold `"#` safely (see AGENTS.md, Gotchas).
    fn small_world() -> World {
        let rows = ["####", "#@.#", "####"];
        let json = format!(
            r##"{{ "id": "t", "name": "Closet", "legend": {{ "#": "wall", ".": "floor" }},
                  "decks": [ {{ "rows": {rows:?} }} ] }}"##
        );
        World::new(parse_map(&json).unwrap(), 99)
    }

    /// The save text for `small_world` with one field replaced, to build
    /// broken saves.
    fn save_with(find: &str, replace: &str) -> String {
        let text = save_to_string(&small_world());
        assert!(text.contains(find), "test setup: {find:?} not in save text");
        text.replacen(find, replace, 1)
    }

    #[test]
    fn a_saved_world_loads_back_identical() {
        let mut world = small_world();
        for _ in 0..20 {
            world.apply(Command::Wait);
        }
        world.apply(Command::Move(Dir::East));

        let loaded = load_from_str(&save_to_string(&world)).unwrap();

        assert_eq!(loaded, world);
        assert_eq!(loaded.state_hash(), world.state_hash());
    }

    #[test]
    fn a_loaded_world_rolls_the_same_dice_as_the_original() {
        let mut original = small_world();
        original.apply(Command::Wait);
        let mut loaded = load_from_str(&save_to_string(&original)).unwrap();

        let a: Vec<_> = (0..200)
            .flat_map(|_| original.apply(Command::Wait))
            .collect();
        let b: Vec<_> = (0..200).flat_map(|_| loaded.apply(Command::Wait)).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn save_text_is_tagged_and_versioned() {
        let text = save_to_string(&small_world());
        assert!(text.contains(r#""format": "nighthawk-save""#));
        assert!(text.contains(r#""version": 12"#));
    }

    /// The exact text of the tiny save in `the_save_format_is_stable`.
    const EXPECTED_SAVE: &str = r#"{
  "format": "nighthawk-save",
  "version": 12,
  "world": {
    "deck_names": [
      "Deck 0"
    ],
    "map": {
      "width": 2,
      "height": 1,
      "depth": 1,
      "tiles": [
        "floor",
        "floor"
      ],
      "air": [
        {
          "o2": 21278,
          "n2": 80006,
          "co2": 41
        },
        {
          "o2": 21278,
          "n2": 80006,
          "co2": 41
        }
      ],
      "vented": [
        0,
        0,
        0
      ],
      "heat": [
        29703423750,
        29703423750
      ],
      "heat_lost": 0,
      "made_of": [
        0,
        0
      ],
      "structure": [
        0,
        0
      ]
    },
    "player": {
      "x": 0,
      "y": 0,
      "z": 0
    },
    "seconds": 0,
    "seed": 7,
    "ambient_rng": {
      "state": 8301130017339275205,
      "increment": 3
    },
    "breath": 1000,
    "warmth": 1000000,
    "content": {
      "materials": [
        {
          "id": "steel",
          "name": "Steel",
          "description": "Steel.",
          "colour": "grey",
          "density": 7850,
          "melting_point": 1450,
          "specific_heat": 490,
          "thermal_conductivity": 45000,
          "hardness": 150,
          "cut_by": {
            "LASER": 2
          }
        }
      ],
      "structures": [
        {
          "id": "deck_plating",
          "name": "deck plating",
          "description": "Plating.",
          "glyph": ".",
          "open_glyph": null,
          "behaviour": "floor",
          "material": "steel",
          "colour": null
        }
      ],
      "item_types": [
        {
          "id": "smock",
          "name": "smock",
          "description": "A smock.",
          "glyph": "[",
          "material": "steel",
          "volume_ml": 1,
          "solid_percent": 1,
          "qualities": {},
          "wear": {
            "pockets": []
          }
        }
      ],
      "recipes": []
    },
    "floor_items": [],
    "worn": [
      {
        "kind": 0,
        "material": 0,
        "pockets": []
      }
    ]
  }
}"#;

    /// A two-tile world built from one material and one structure, so its
    /// save is short enough to write out in full.
    fn tiny_world() -> World {
        use crate::content::{Content, ContentFile};
        let materials = crate::material::parse_materials(&[ContentFile::new(
            "metals.json",
            r#"{ "materials": [ { "id": "steel", "name": "Steel", "description": "Steel.",
                 "colour": "grey", "density": 7850, "melting_point": 1450,
                 "specific_heat": 490, "thermal_conductivity": 45000, "hardness": 150,
                 "cut_by": { "LASER": 2 } } ] }"#,
        )])
        .unwrap();
        let structures = crate::structure::parse_structures(
            &[ContentFile::new(
                "floors.json",
                r#"{ "structures": [ { "id": "deck_plating", "name": "deck plating",
                     "description": "Plating.", "glyph": ".", "behaviour": "floor",
                     "material": "steel" } ] }"#,
            )],
            &materials,
        )
        .unwrap();
        let items = crate::item::parse_items(
            &[ContentFile::new(
                "clothing.json",
                r#"{ "items": [ { "id": "smock", "name": "smock", "description": "A smock.",
                     "glyph": "[", "material": "steel", "volume_ml": 1, "solid_percent": 1,
                     "wear": { "pockets": [] } } ] }"#,
            )],
            &materials,
        )
        .unwrap();
        let rows = ["@."];
        let json = format!(
            r#"{{ "id": "t", "name": "Nook",
                 "legend": {{ ".": "deck_plating", "@": "deck_plating" }},
                 "player_wears": [ {{ "item": "smock" }} ],
                 "decks": [ {{ "rows": {rows:?} }} ] }}"#
        );
        let recipes = crate::recipe::parse_recipes(&[], &structures, &items).unwrap();
        let content = Content {
            materials,
            structures,
            item_types: items,
            recipes,
        };
        World::new(crate::map_file::parse_map(&json, &content).unwrap(), 7)
    }

    #[test]
    fn the_save_format_is_stable() {
        // The exact text of a tiny save. If this fails, the save layout
        // changed: bump SAVE_VERSION, then update this text on purpose.
        assert_eq!(save_to_string(&tiny_world()), EXPECTED_SAVE);
    }

    #[test]
    fn garbage_is_a_damaged_save() {
        assert!(matches!(
            load_from_str("not json at all"),
            Err(SaveError::Json(_))
        ));
    }

    #[test]
    fn other_json_is_not_a_save() {
        assert!(matches!(
            load_from_str(r#"{ "hello": 1 }"#),
            Err(SaveError::NotASave)
        ));
        let wrong_tag = save_with(r#""format": "nighthawk-save""#, r#""format": "other-game""#);
        assert!(matches!(
            load_from_str(&wrong_tag),
            Err(SaveError::NotASave)
        ));
    }

    #[test]
    fn a_save_without_a_version_is_not_a_save() {
        let text = save_with("\"version\": 12,", "");
        assert!(matches!(load_from_str(&text), Err(SaveError::NotASave)));
    }

    #[test]
    fn saves_from_other_versions_are_rejected_clearly() {
        // A save from before content was kept in one bundle: exactly what
        // an old save file on disk looks like after this build is
        // installed.
        let text = save_with(r#""version": 12"#, r#""version": 11"#);
        let err = load_from_str(&text).unwrap_err();
        assert!(matches!(err, SaveError::UnsupportedVersion { found: 11 }));
        assert_eq!(
            err.to_string(),
            "save file is version 11, but this build only reads version 12"
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let text = save_with(r#""seconds": 0"#, r#""seconds": 0, "cheats": true"#);
        assert!(matches!(load_from_str(&text), Err(SaveError::Json(_))));
    }

    #[test]
    fn a_wrong_tile_count_is_caught() {
        let text = save_with("\"depth\": 1", "\"depth\": 2");
        let err = load_from_str(&text).unwrap_err();
        assert!(matches!(err, SaveError::Inconsistent(_)));
        assert!(err.to_string().contains("should have 24 tiles but has 12"));
    }

    #[test]
    fn a_zero_sized_map_is_caught() {
        let text = save_with("\"width\": 4", "\"width\": 0");
        let err = load_from_str(&text).unwrap_err();
        assert!(err.to_string().contains("must be positive"), "{err}");
    }

    #[test]
    fn a_player_outside_the_map_is_caught() {
        let text = save_with("\"x\": 1", "\"x\": 40");
        let err = load_from_str(&text).unwrap_err();
        assert!(err.to_string().contains("outside the map"), "{err}");
    }

    #[test]
    fn a_player_inside_a_wall_is_caught() {
        let text = save_with("\"x\": 1", "\"x\": 0");
        let err = load_from_str(&text).unwrap_err();
        assert!(err.to_string().contains("inside a bulkhead"), "{err}");
    }

    #[test]
    fn error_messages_say_what_is_wrong() {
        assert!(
            SaveError::NotASave
                .to_string()
                .contains("not a Nighthawk save")
        );
        let damaged = load_from_str("{").unwrap_err();
        assert!(damaged.to_string().starts_with("save file is damaged:"));
    }

    /// Two decks joined by stairs, the player upstairs.
    fn two_deck_world() -> World {
        let lower = ["@<"];
        let upper = [".>"];
        let json = format!(
            r#"{{ "id": "t", "name": "T", "legend": {{ ".": "floor", "<": "stairs_up",
                    ">": "stairs_down" }},
                 "decks": [ {{ "name": "Lower", "rows": {lower:?} }},
                            {{ "name": "Upper", "rows": {upper:?} }} ] }}"#
        );
        let mut world = World::new(parse_map(&json).unwrap(), 3);
        world.apply(Command::Move(Dir::East));
        world.apply(Command::Climb(crate::pos::Vertical::Up));
        world
    }

    #[test]
    fn multi_deck_worlds_save_and_load_with_the_player_upstairs() {
        let world = two_deck_world();
        assert_eq!(world.player().z, 1);
        let loaded = load_from_str(&save_to_string(&world)).unwrap();
        assert_eq!(loaded, world);
        assert_eq!(loaded.area_name(), "Upper");
    }

    #[test]
    fn a_save_with_broken_stairs_is_caught() {
        let text = save_to_string(&two_deck_world()).replacen("\"stairs_up\"", "\"floor\"", 1);
        let err = load_from_str(&text).unwrap_err();
        assert!(matches!(err, SaveError::Inconsistent(_)));
        assert!(err.to_string().contains("need stairs up"), "{err}");
    }

    #[test]
    fn a_save_with_missing_deck_names_is_caught() {
        let text = save_to_string(&two_deck_world()).replacen("\"Upper\"", "", 1);
        // Removing a name leaves a dangling comma, so fix the JSON up too.
        let text = text.replacen("\"Lower\",\n      \n", "\"Lower\"\n", 1);
        let err = load_from_str(&text).unwrap_err();
        assert!(
            err.to_string().contains("1 deck names for 2 decks"),
            "{err}"
        );
    }

    #[test]
    fn doors_are_saved_open_or_closed() {
        let rows = ["@+'"];
        let json = format!(
            r#"{{ "id": "t", "name": "T", "legend": {{ "+": "door", "'": {{ "structure": "door", "open": true }} }},
                 "decks": [ {{ "rows": {rows:?} }} ] }}"#
        );
        let mut world = World::new(parse_map(&json).unwrap(), 3);
        world.apply(Command::Move(Dir::East)); // opens the closed door
        let text = save_to_string(&world);
        assert!(text.contains("\"door_open\""));
        assert_eq!(load_from_str(&text).unwrap(), world);
    }
}
