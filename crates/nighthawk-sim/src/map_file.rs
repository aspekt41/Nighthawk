//! Loading maps from JSON (decision D17: content is JSON).
//!
//! A map file draws each deck as rows of characters, and a legend says which
//! tile each character means. `@` is special: it marks where the player
//! starts, on a floor tile. For example:
//!
//! ```json
//! {
//!   "id": "closet",
//!   "name": "Supply closet",
//!   "legend": { "#": "wall", ".": "floor" },
//!   "decks": [ { "rows": ["#####", "#.@.#", "#####"] } ]
//! }
//! ```
//!
//! Decks are listed bottom to top: the first deck is `z = 0`. Every deck must
//! be the same size, because the [`Map`] is one 3D box.
//!
//! This module does no file I/O. It turns JSON *text* into a map, so the sim
//! stays pure and the tests don't need files. Reading the file from disk is
//! the front-end's job.

use std::collections::HashMap;
use std::fmt;

use serde::Deserialize;

use crate::air::Mix;
use crate::heat::{MAX_CELSIUS, MIN_CELSIUS, celsius_to_mk};
use crate::map::{Map, StairsError, Tile};
use crate::material::{Material, Materials};
use crate::pos::Pos;
use crate::structure::{Behaviour, Structure, Structures};

/// The glyph that marks the player's starting position.
const PLAYER_START: char = '@';

/// A map ready to play: the tiles plus everything else the file said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedMap {
    pub id: String,
    pub name: String,
    /// One name per deck, bottom to top, like "Stasis bay".
    pub deck_names: Vec<String>,
    pub map: Map,
    pub player_start: Pos,
    /// Every material, in the order the map's tiles refer to them (the
    /// world keeps its own copy, so a save never depends on data files).
    pub materials: Vec<Material>,
    /// Every structure, in the order the map's tiles refer to them.
    pub structures: Vec<Structure>,
}

/// A mistake in a map's legend. Kept apart from [`MapError`] so each list
/// of messages stays short.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegendError {
    /// A key that isn't exactly one character.
    BadKey { key: String },
    /// An entry naming a structure that isn't defined.
    UnknownStructure { glyph: char, structure: String },
    /// An entry naming a material that isn't defined.
    UnknownMaterial { glyph: char, material: String },
    /// `"open": true` on something that isn't a door.
    NotADoor { glyph: char },
    /// `@` standing on something you can't stand on.
    StartInsideSomething { structure: String },
}

impl fmt::Display for LegendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LegendError::BadKey { key } => {
                write!(f, "legend key {key:?} must be exactly one character")
            }
            LegendError::UnknownStructure { glyph, structure } => write!(
                f,
                "legend entry {glyph:?} is {structure:?}, which isn't a structure"
            ),
            LegendError::UnknownMaterial { glyph, material } => write!(
                f,
                "legend entry {glyph:?} is made of {material:?}, which isn't a material"
            ),
            LegendError::NotADoor { glyph } => {
                write!(f, "legend entry {glyph:?} is open, but only doors can be")
            }
            LegendError::StartInsideSomething { structure } => write!(
                f,
                "'{PLAYER_START}' marks where the player starts, so it must stand for something walkable, not {structure:?}"
            ),
        }
    }
}

/// Everything that can be wrong with a map file. Each variant says exactly
/// where the problem is, so a content author can fix it without guessing.
#[derive(Debug)]
pub enum MapError {
    /// Not valid JSON, or the wrong shape: a missing field, a misspelled
    /// field name, or an unknown tile name in the legend.
    Json(serde_json::Error),
    /// A mistake in the legend.
    Legend(LegendError),
    NoDecks,
    EmptyDeck {
        deck: usize,
    },
    /// A row with a different length from the deck's first row.
    RaggedRow {
        deck: usize,
        row: usize,
        expected: usize,
        found: usize,
    },
    /// A deck with a different number of rows from deck 0.
    DeckHeightMismatch {
        deck: usize,
        expected: usize,
        found: usize,
    },
    /// A character that isn't in the legend.
    UnknownGlyph {
        glyph: char,
        deck: usize,
        row: usize,
        col: usize,
    },
    /// The map must contain exactly one `@`.
    PlayerStartCount {
        found: usize,
    },
    /// Stairs without a matching pair on the next deck.
    Stairs(StairsError),
    /// A deck's starting `pressure` is above the allowed maximum.
    BadPressure {
        deck: usize,
        percent: u32,
    },
    /// A deck's starting `temperature` is colder than deep space or too hot.
    BadTemperature {
        deck: usize,
        celsius: i32,
    },
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapError::Json(e) => write!(f, "invalid map JSON: {e}"),
            MapError::Legend(e) => write!(f, "{e}"),
            // Problems with the decks' size and shape get their own function,
            // so this `match` stays under the complexity limit.
            MapError::NoDecks
            | MapError::EmptyDeck { .. }
            | MapError::RaggedRow { .. }
            | MapError::DeckHeightMismatch { .. } => self.fmt_layout(f),
            MapError::UnknownGlyph {
                glyph,
                deck,
                row,
                col,
            } => write!(
                f,
                "deck {deck}, row {row}, column {col}: {glyph:?} is not in the legend"
            ),
            MapError::Stairs(e) => write!(f, "{e}"),
            MapError::BadPressure { deck, percent } => write!(
                f,
                "deck {deck} has pressure {percent}%; it must be 0 to {MAX_PRESSURE_PERCENT}"
            ),
            MapError::BadTemperature { deck, celsius } => write!(
                f,
                "deck {deck} has temperature {celsius} °C; it must be {MIN_CELSIUS} to {MAX_CELSIUS}"
            ),
            MapError::PlayerStartCount { found } => write!(
                f,
                "map must have exactly one '{PLAYER_START}' (player start); found {found}"
            ),
        }
    }
}

impl MapError {
    /// Messages for decks of the wrong size or shape. `fmt` only sends
    /// those variants here, so the last arm is the one left: no decks.
    fn fmt_layout(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapError::EmptyDeck { deck } => write!(f, "deck {deck} has no rows"),
            MapError::RaggedRow {
                deck,
                row,
                expected,
                found,
            } => write!(
                f,
                "deck {deck}, row {row} is {found} characters wide; expected {expected}"
            ),
            MapError::DeckHeightMismatch {
                deck,
                expected,
                found,
            } => {
                write!(f, "deck {deck} has {found} rows; expected {expected}")
            }
            _ => write!(f, "map has no decks"),
        }
    }
}

impl std::error::Error for MapError {}

/// The JSON shape, exactly as written in the file. `deny_unknown_fields`
/// makes a misspelled field (`"legnd"`) an error instead of silently ignored.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MapFile {
    id: String,
    name: String,
    legend: HashMap<String, LegendEntry>,
    decks: Vec<DeckFile>,
}

/// What a legend character stands for: just a structure's id
/// (`"bulkhead"`), made of the structure's own material; or a structure
/// with a different material, or a door placed open
/// (`{ "structure": "door", "open": true, "material": "steel" }`).
///
/// `untagged` means serde tries each form in turn and takes the first that
/// fits, so the JSON needs no extra label saying which form it is.
#[derive(Deserialize)]
#[serde(untagged)]
enum LegendEntry {
    Id(String),
    Placed(Placed),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Placed {
    structure: String,
    #[serde(default)]
    material: Option<String>,
    #[serde(default)]
    open: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeckFile {
    /// Shown as the map panel's title while you're on this deck. Optional:
    /// without it the deck is called "Deck 0", "Deck 1" and so on.
    #[serde(default)]
    name: Option<String>,
    /// The deck's starting air pressure as a percentage of normal ship air
    /// (101.3 kPa), from 0 (vacuum) to 200. Optional: 100 if left out. The
    /// mix is always normal ship air, just thinner or denser.
    #[serde(default)]
    pressure: Option<u32>,
    /// The deck's starting air temperature in whole °C, from −270 (deep
    /// space) to 200. Optional: 20 °C if left out.
    #[serde(default)]
    temperature: Option<i32>,
    rows: Vec<String>,
}

/// The highest starting pressure a deck may ask for, in percent.
const MAX_PRESSURE_PERCENT: u32 = 200;

/// The structures and materials a map's legend can name.
#[derive(Clone, Copy)]
pub struct Catalogue<'a> {
    pub materials: &'a Materials,
    pub structures: &'a Structures,
}

/// Parses and validates a map file's text. Its legend names structures from
/// `catalogue`, made of its materials.
pub fn parse_map(json: &str, catalogue: Catalogue) -> Result<LoadedMap, MapError> {
    let file: MapFile = serde_json::from_str(json).map_err(MapError::Json)?;
    let legend = glyph_legend(&file.legend, catalogue)?;
    let (width, height) = dimensions(&file.decks)?;

    let depth = file.decks.len();
    let mut map = Map::filled(to_i32(width), to_i32(height), to_i32(depth), Tile::Floor);
    let mut starts = Vec::new();

    for (z, y, x, glyph) in cells(&file.decks) {
        let pos = Pos::new(to_i32(x), to_i32(y), to_i32(z));
        let (tile, structure, material) = tile_for(glyph, &legend, (z, y, x))?;
        map.set(pos, tile);
        map.set_structure(pos, structure);
        map.set_material(pos, material);
        if glyph == PLAYER_START {
            starts.push(pos);
        }
    }

    map.check_stairs().map_err(MapError::Stairs)?;
    fill_air(&mut map, &file.decks)?;
    let deck_names = deck_names(&file.decks);

    match starts[..] {
        [player_start] => Ok(LoadedMap {
            id: file.id,
            name: file.name,
            deck_names,
            map,
            player_start,
            materials: catalogue.materials.list().to_vec(),
            structures: catalogue.structures.list().to_vec(),
        }),
        _ => Err(MapError::PlayerStartCount {
            found: starts.len(),
        }),
    }
}

/// Every glyph in every deck, as `(deck, row, column, glyph)`, in reading
/// order. Flattening the three nested loops into one iterator keeps
/// `parse_map` flat and easy to follow.
fn cells(decks: &[DeckFile]) -> impl Iterator<Item = (usize, usize, usize, char)> + '_ {
    decks.iter().enumerate().flat_map(|(z, deck)| {
        deck.rows.iter().enumerate().flat_map(move |(y, row)| {
            row.chars()
                .enumerate()
                .map(move |(x, glyph)| (z, y, x, glyph))
        })
    })
}

/// Gives every deck its starting air: normal ship air scaled to the
/// deck's `pressure` (100% unless the file says otherwise), at the deck's
/// `temperature` (20 °C unless it says otherwise).
fn fill_air(map: &mut Map, decks: &[DeckFile]) -> Result<(), MapError> {
    for (z, deck) in decks.iter().enumerate() {
        let percent = deck.pressure.unwrap_or(100);
        if percent > MAX_PRESSURE_PERCENT {
            return Err(MapError::BadPressure { deck: z, percent });
        }
        let celsius = deck.temperature.unwrap_or(DEFAULT_CELSIUS);
        if !(MIN_CELSIUS..=MAX_CELSIUS).contains(&celsius) {
            return Err(MapError::BadTemperature { deck: z, celsius });
        }
        map.fill_deck_air(to_i32(z), Mix::STANDARD.scaled(percent));
        map.set_deck_temperature(to_i32(z), celsius_to_mk(celsius));
    }
    Ok(())
}

/// The temperature a deck starts at if its map doesn't say: 20 °C.
const DEFAULT_CELSIUS: i32 = 20;

/// Each deck's name, or "Deck N" for decks without one.
fn deck_names(decks: &[DeckFile]) -> Vec<String> {
    decks
        .iter()
        .enumerate()
        .map(|(z, deck)| deck.name.clone().unwrap_or_else(|| format!("Deck {z}")))
        .collect()
}

/// What a legend character places: a tile, and the indexes of its
/// structure and material.
type Placement = (Tile, u16, u16);

/// Turns the legend's one-character string keys into `char`s, and each
/// entry into what it places.
fn glyph_legend(
    legend: &HashMap<String, LegendEntry>,
    catalogue: Catalogue,
) -> Result<HashMap<char, Placement>, MapError> {
    legend
        .iter()
        .map(|(key, entry)| {
            let mut chars = key.chars();
            let glyph = match (chars.next(), chars.next()) {
                (Some(c), None) => c,
                _ => return Err(LegendError::BadKey { key: key.clone() }),
            };
            Ok((glyph, place(glyph, entry, catalogue)?))
        })
        .collect::<Result<_, LegendError>>()
        .map_err(MapError::Legend)
}

/// What a legend entry places.
fn place(glyph: char, entry: &LegendEntry, catalogue: Catalogue) -> Result<Placement, LegendError> {
    let (id, material, open) = match entry {
        LegendEntry::Id(id) => (id, None, false),
        LegendEntry::Placed(p) => (&p.structure, p.material.as_ref(), p.open),
    };
    let structure =
        catalogue
            .structures
            .index_of(id)
            .ok_or_else(|| LegendError::UnknownStructure {
                glyph,
                structure: id.clone(),
            })?;
    let made = &catalogue.structures.list()[usize::from(structure)];
    if open && made.behaviour != Behaviour::Door {
        return Err(LegendError::NotADoor { glyph });
    }
    let tile = made.behaviour.tile(open);
    if glyph == PLAYER_START && !tile.is_walkable() {
        return Err(LegendError::StartInsideSomething {
            structure: id.clone(),
        });
    }
    let material_id = material.unwrap_or(&made.material);
    let material =
        catalogue
            .materials
            .index_of(material_id)
            .ok_or_else(|| LegendError::UnknownMaterial {
                glyph,
                material: material_id.clone(),
            })?;
    Ok((tile, structure, material))
}

/// Checks every deck is the same rectangle and returns its `(width, height)`.
/// Width counts characters, not bytes, so non-ASCII glyphs like `═` work.
fn dimensions(decks: &[DeckFile]) -> Result<(usize, usize), MapError> {
    let first = decks.first().ok_or(MapError::NoDecks)?;
    let height = first.rows.len();
    let width = first
        .rows
        .first()
        .ok_or(MapError::EmptyDeck { deck: 0 })?
        .chars()
        .count();

    for (deck, d) in decks.iter().enumerate() {
        if d.rows.is_empty() {
            return Err(MapError::EmptyDeck { deck });
        }
        if d.rows.len() != height {
            return Err(MapError::DeckHeightMismatch {
                deck,
                expected: height,
                found: d.rows.len(),
            });
        }
        for (row, r) in d.rows.iter().enumerate() {
            let found = r.chars().count();
            if found != width {
                return Err(MapError::RaggedRow {
                    deck,
                    row,
                    expected: width,
                    found,
                });
            }
        }
    }
    Ok((width, height))
}

/// The tile a glyph stands for. `@` is the player start, on a floor tile.
fn tile_for(
    glyph: char,
    legend: &HashMap<char, Placement>,
    (deck, row, col): (usize, usize, usize),
) -> Result<Placement, MapError> {
    legend.get(&glyph).copied().ok_or(MapError::UnknownGlyph {
        glyph,
        deck,
        row,
        col,
    })
}

/// Map files are small, so their sizes always fit in an `i32`. If one ever
/// doesn't, that's a bug worth crashing on, not a recoverable error.
fn to_i32(n: usize) -> i32 {
    i32::try_from(n).expect("map dimension fits in i32")
}

/// [`parse_map`] with a small set of test structures (named after their
/// behaviours: `"wall"`, `"floor"`, `"door"`…, all steel), so tests can
/// build maps from a few lines of JSON. If the legend doesn't say what `@`
/// stands on, it's `"floor"`. Other modules' tests import it *as*
/// `parse_map`.
#[cfg(test)]
pub(crate) fn parse_map_for_tests(json: &str) -> Result<LoadedMap, MapError> {
    let materials = crate::material::tests::test_materials();
    let structures = crate::structure::tests::test_structures();
    let catalogue = Catalogue {
        materials: &materials,
        structures: &structures,
    };
    // Add `"@": "floor"` to the legend, if the JSON is valid enough to
    // have one and it doesn't say. Otherwise parse it as it is, so tests of
    // broken JSON see the real error.
    let mut value: serde_json::Value = match serde_json::from_str(json) {
        Ok(value) => value,
        Err(_) => return parse_map(json, catalogue),
    };
    if let Some(legend) = value.get_mut("legend").and_then(|l| l.as_object_mut()) {
        legend.entry("@").or_insert_with(|| "floor".into());
    }
    parse_map(&value.to_string(), catalogue)
}

#[cfg(test)]
mod tests {
    use super::*;
    // This import beats the `parse_map` that `super::*` brings in: names
    // imported by name win over names imported with `*`.
    use super::parse_map_for_tests as parse_map;
    use crate::air::Mix;

    /// Builds a one-deck map file from rows, with the standard legend.
    fn one_deck(rows: &[&str]) -> String {
        let rows: Vec<String> = rows.iter().map(|r| format!("{r:?}")).collect();
        format!(
            r###"{{ "id": "t", "name": "Test", "legend": {{ "#": "wall", ".": "floor" }},
                  "decks": [ {{ "rows": [{}] }} ] }}"###,
            rows.join(", ")
        )
    }

    #[test]
    fn parses_a_valid_map() {
        let loaded = parse_map(&one_deck(&["####", "#.@#", "####"])).unwrap();

        assert_eq!(loaded.id, "t");
        assert_eq!(loaded.name, "Test");
        assert_eq!(
            (loaded.map.width(), loaded.map.height(), loaded.map.depth()),
            (4, 3, 1)
        );
        assert_eq!(loaded.player_start, Pos::new(2, 1, 0));
        assert_eq!(loaded.map.get(Pos::new(0, 0, 0)), Some(Tile::Wall));
        assert_eq!(loaded.map.get(Pos::new(1, 1, 0)), Some(Tile::Floor));
    }

    #[test]
    fn player_start_is_floor() {
        let loaded = parse_map(&one_deck(&["###", "#@#", "###"])).unwrap();
        assert_eq!(loaded.map.get(loaded.player_start), Some(Tile::Floor));
    }

    #[test]
    fn decks_stack_bottom_to_top() {
        let json = r###"{ "id": "t", "name": "T", "legend": { "#": "wall", ".": "floor" },
            "decks": [ { "rows": ["##", "##"] }, { "rows": ["@.", ".."] } ] }"###;
        let loaded = parse_map(json).unwrap();

        assert_eq!(loaded.map.depth(), 2);
        assert_eq!(loaded.player_start, Pos::new(0, 0, 1));
        assert_eq!(loaded.map.get(Pos::new(1, 1, 0)), Some(Tile::Wall));
        assert_eq!(loaded.map.get(Pos::new(1, 1, 1)), Some(Tile::Floor));
    }

    #[test]
    fn width_counts_characters_not_bytes() {
        // '═' is 3 bytes in UTF-8 but one tile.
        let json = r#"{ "id": "t", "name": "T", "legend": { "═": "wall", ".": "floor" },
            "decks": [ { "rows": ["═══", ".@."] } ] }"#;
        let loaded = parse_map(json).unwrap();
        assert_eq!(loaded.map.width(), 3);
        assert_eq!(loaded.map.get(Pos::new(1, 0, 0)), Some(Tile::Wall));
    }

    #[test]
    fn misspelled_field_is_rejected() {
        let json = r###"{ "id": "t", "name": "T", "legnd": { "#": "wall" },
            "decks": [ { "rows": ["@"] } ] }"###;
        assert!(matches!(parse_map(json), Err(MapError::Json(_))));
    }

    #[test]
    fn unknown_structure_name_is_rejected() {
        let json = r###"{ "id": "t", "name": "T", "legend": { "#": "wal" },
            "decks": [ { "rows": ["@"] } ] }"###;
        assert!(matches!(
            parse_map(json),
            Err(MapError::Legend(LegendError::UnknownStructure { .. }))
        ));
    }

    #[test]
    fn glyph_missing_from_legend_reports_its_location() {
        let err = parse_map(&one_deck(&["####", "#@x#", "####"])).unwrap_err();
        assert!(matches!(
            err,
            MapError::UnknownGlyph {
                glyph: 'x',
                deck: 0,
                row: 1,
                col: 2
            }
        ));
    }

    #[test]
    fn ragged_row_is_rejected() {
        let err = parse_map(&one_deck(&["####", "#@#", "####"])).unwrap_err();
        assert!(matches!(
            err,
            MapError::RaggedRow {
                deck: 0,
                row: 1,
                expected: 4,
                found: 3
            }
        ));
    }

    #[test]
    fn decks_of_different_heights_are_rejected() {
        let json = r###"{ "id": "t", "name": "T", "legend": { ".": "floor" },
            "decks": [ { "rows": ["@.", ".."] }, { "rows": [".."] } ] }"###;
        let err = parse_map(json).unwrap_err();
        assert!(matches!(
            err,
            MapError::DeckHeightMismatch {
                deck: 1,
                expected: 2,
                found: 1
            }
        ));
    }

    #[test]
    fn missing_player_start_is_rejected() {
        let err = parse_map(&one_deck(&["###", "#.#", "###"])).unwrap_err();
        assert!(matches!(err, MapError::PlayerStartCount { found: 0 }));
    }

    #[test]
    fn two_player_starts_are_rejected() {
        let err = parse_map(&one_deck(&["####", "#@@#", "####"])).unwrap_err();
        assert!(matches!(err, MapError::PlayerStartCount { found: 2 }));
    }

    #[test]
    fn multi_character_legend_key_is_rejected() {
        let json = r###"{ "id": "t", "name": "T", "legend": { "##": "wall" },
            "decks": [ { "rows": ["@"] } ] }"###;
        let err = parse_map(json).unwrap_err();
        assert!(matches!(
            err,
            MapError::Legend(LegendError::BadKey { ref key }) if key == "##"
        ));
    }

    #[test]
    fn the_player_cannot_start_inside_a_wall() {
        let json = r#"{ "id": "t", "name": "T", "legend": { "@": "wall" },
            "decks": [ { "rows": ["@"] } ] }"#;
        assert_eq!(
            parse_map(json).unwrap_err().to_string(),
            "'@' marks where the player starts, so it must stand for something walkable, not \"wall\""
        );
    }

    #[test]
    fn map_without_decks_is_rejected() {
        let json = r#"{ "id": "t", "name": "T", "legend": {}, "decks": [] }"#;
        assert!(matches!(parse_map(json), Err(MapError::NoDecks)));
    }

    #[test]
    fn deck_without_rows_is_rejected() {
        let json = r#"{ "id": "t", "name": "T", "legend": {}, "decks": [ { "rows": [] } ] }"#;
        assert!(matches!(
            parse_map(json),
            Err(MapError::EmptyDeck { deck: 0 })
        ));
    }

    #[test]
    fn later_empty_deck_is_rejected() {
        let json = r#"{ "id": "t", "name": "T", "legend": {},
            "decks": [ { "rows": ["@"] }, { "rows": [] } ] }"#;
        assert!(matches!(
            parse_map(json),
            Err(MapError::EmptyDeck { deck: 1 })
        ));
    }

    #[test]
    fn every_error_message_says_where_the_problem_is() {
        let cases = [
            (
                MapError::Legend(LegendError::BadKey { key: "ab".into() }),
                "\"ab\"",
            ),
            (MapError::NoDecks, "no decks"),
            (MapError::EmptyDeck { deck: 3 }, "deck 3"),
            (
                MapError::RaggedRow {
                    deck: 0,
                    row: 4,
                    expected: 9,
                    found: 7,
                },
                "row 4 is 7 characters wide; expected 9",
            ),
            (
                MapError::DeckHeightMismatch {
                    deck: 2,
                    expected: 5,
                    found: 4,
                },
                "deck 2 has 4 rows; expected 5",
            ),
            (MapError::PlayerStartCount { found: 2 }, "found 2"),
        ];
        for (err, expected) in cases {
            let msg = err.to_string();
            assert!(
                msg.contains(expected),
                "{msg:?} should contain {expected:?}"
            );
        }
    }

    #[test]
    fn json_errors_say_what_went_wrong() {
        let err = parse_map("{ not json").unwrap_err();
        assert!(err.to_string().starts_with("invalid map JSON:"));
    }

    #[test]
    fn errors_have_readable_messages() {
        let err = parse_map(&one_deck(&["####", "#@x#", "####"])).unwrap_err();
        assert_eq!(
            err.to_string(),
            "deck 0, row 1, column 2: 'x' is not in the legend"
        );
    }

    #[test]
    fn decks_are_named_or_numbered() {
        let lower = ["#<#"];
        let upper = ["@>."];
        let json = format!(
            r#"{{ "id": "t", "name": "T", "legend": {{ "\u0023": "wall", ".": "floor",
                    "<": "stairs_up", ">": "stairs_down" }},
                 "decks": [ {{ "rows": {lower:?} }}, {{ "name": "Upper", "rows": {upper:?} }} ] }}"#
        );
        let loaded = parse_map(&json).unwrap();
        assert_eq!(
            loaded.deck_names,
            vec!["Deck 0".to_string(), "Upper".to_string()]
        );
        assert_eq!(loaded.map.get(Pos::new(1, 0, 0)), Some(Tile::StairsUp));
        assert_eq!(loaded.map.get(Pos::new(1, 0, 1)), Some(Tile::StairsDown));
    }

    #[test]
    fn unpaired_stairs_are_rejected_with_their_location() {
        let rows = ["#@.<#"];
        let json = format!(
            r#"{{ "id": "t", "name": "T", "legend": {{ "\u0023": "wall", ".": "floor",
                    "<": "stairs_up" }}, "decks": [ {{ "rows": {rows:?} }} ] }}"#
        );
        let err = parse_map(&json).unwrap_err();
        assert!(matches!(err, MapError::Stairs(_)));
        assert_eq!(
            err.to_string(),
            "stairs up at deck 0, row 0, column 3 need stairs down at the same spot on deck 1"
        );
    }

    #[test]
    fn decks_start_with_normal_air_unless_the_file_says_otherwise() {
        let lower = ["#<#"];
        let upper = ["@>."];
        let json = format!(
            r#"{{ "id": "t", "name": "T", "legend": {{ "\u0023": "wall", ".": "floor",
                    "<": "stairs_up", ">": "stairs_down" }},
                 "decks": [ {{ "rows": {lower:?} }},
                            {{ "pressure": 60, "rows": {upper:?} }} ] }}"#
        );
        let map = parse_map(&json).unwrap().map;
        assert_eq!(
            map.air(Pos::new(1, 0, 0)),
            Some(Mix::STANDARD),
            "default 100%"
        );
        assert_eq!(
            map.air(Pos::new(0, 0, 0)),
            Some(Mix::VACUUM),
            "walls hold none"
        );
        assert_eq!(map.air(Pos::new(2, 0, 1)), Some(Mix::STANDARD.scaled(60)));
    }

    #[test]
    fn a_vacuum_deck_is_allowed() {
        let rows = ["@."];
        let json = format!(
            r#"{{ "id": "t", "name": "T", "legend": {{ ".": "floor" }},
                 "decks": [ {{ "pressure": 0, "rows": {rows:?} }} ] }}"#
        );
        let map = parse_map(&json).unwrap().map;
        assert_eq!(map.air(Pos::new(1, 0, 0)), Some(Mix::VACUUM));
    }

    #[test]
    fn too_high_a_pressure_is_rejected() {
        let rows = ["@."];
        let json = format!(
            r#"{{ "id": "t", "name": "T", "legend": {{ ".": "floor" }},
                 "decks": [ {{ "pressure": 201, "rows": {rows:?} }} ] }}"#
        );
        let err = parse_map(&json).unwrap_err();
        assert!(matches!(
            err,
            MapError::BadPressure {
                deck: 0,
                percent: 201
            }
        ));
        assert_eq!(
            err.to_string(),
            "deck 0 has pressure 201%; it must be 0 to 200"
        );
    }
}

#[cfg(test)]
mod temperature_tests {
    use super::parse_map_for_tests as parse_map;
    use super::*;

    /// A one-row, one-deck map with `deck` as extra fields on the deck.
    fn map_with(deck: &str) -> Result<LoadedMap, MapError> {
        let rows = ["@.#"];
        let json = format!(
            r##"{{ "id": "t", "name": "T", "legend": {{ ".": "floor", "#": "wall" }},
                 "decks": [ {{ {deck} "rows": {rows:?} }} ] }}"##
        );
        parse_map(&json)
    }

    #[test]
    fn decks_start_at_20_degrees_unless_the_file_says_otherwise() {
        let warm = map_with("").unwrap().map;
        assert_eq!(warm.temperature(Pos::new(1, 0, 0)), Some(celsius_to_mk(20)));
        let cold = map_with(r#""temperature": -20,"#).unwrap().map;
        assert_eq!(
            cold.temperature(Pos::new(1, 0, 0)),
            Some(celsius_to_mk(-20))
        );
        assert_eq!(
            cold.temperature(Pos::new(2, 0, 0)),
            None,
            "walls hold no air"
        );
    }

    #[test]
    fn a_vacuum_deck_has_no_temperature() {
        let map = map_with(r#""pressure": 0, "temperature": 5,"#).unwrap().map;
        assert_eq!(map.temperature(Pos::new(1, 0, 0)), None);
        assert_eq!(map.total_heat(), 0);
    }

    #[test]
    fn the_coldest_and_hottest_allowed_temperatures_load() {
        assert!(map_with(r#""temperature": -270,"#).is_ok());
        assert!(map_with(r#""temperature": 200,"#).is_ok());
    }

    #[test]
    fn temperatures_out_of_range_are_rejected_with_the_deck_named() {
        for celsius in [-271, 201] {
            let err = map_with(&format!(r#""temperature": {celsius},"#)).unwrap_err();
            assert!(
                matches!(err, MapError::BadTemperature { deck: 0, celsius: c } if c == celsius),
                "{err:?}"
            );
        }
        let err = map_with(r#""temperature": 201,"#).unwrap_err();
        assert_eq!(
            err.to_string(),
            "deck 0 has temperature 201 °C; it must be -270 to 200"
        );
    }
}

#[cfg(test)]
mod material_tests {
    use super::parse_map_for_tests as parse_map;
    use super::*;

    /// A one-deck map whose legend has `extra` entries besides `.` floor.
    fn map_with_legend(extra: &str, row: &str) -> Result<LoadedMap, MapError> {
        let json = format!(
            r#"{{ "id": "t", "name": "T", "legend": {{ ".": "floor", {extra} }},
                 "decks": [ {{ "rows": [{row:?}] }} ] }}"#
        );
        parse_map(&json)
    }

    #[test]
    fn a_structure_is_made_of_its_own_material_unless_the_legend_says() {
        let loaded = map_with_legend(r#""W": "wall""#, "@.W").unwrap();
        let wall = Pos::new(2, 0, 0);
        assert_eq!(loaded.map.structure_index(wall), Some(1));
        assert_eq!(loaded.map.material_index(wall), Some(0));
        assert_eq!(loaded.materials[0].id, "steel");
        assert_eq!(loaded.materials.len(), 2, "the whole list comes along");
        assert_eq!(loaded.structures.len(), 7);
    }

    #[test]
    fn a_legend_entry_can_name_its_material() {
        let entry = r#""H": { "structure": "wall", "material": "hard_alloy" }"#;
        let loaded = map_with_legend(entry, "@.H").unwrap();
        assert_eq!(loaded.map.get(Pos::new(2, 0, 0)), Some(Tile::Wall));
        assert_eq!(loaded.map.material_index(Pos::new(2, 0, 0)), Some(1));
        assert_eq!(
            loaded.map.material_index(Pos::new(0, 0, 0)),
            Some(0),
            "the player starts on steel floor"
        );
    }

    #[test]
    fn a_door_can_be_placed_open() {
        let entry = r#""'": { "structure": "door", "open": true }"#;
        let loaded = map_with_legend(entry, "@.'").unwrap();
        assert_eq!(loaded.map.get(Pos::new(2, 0, 0)), Some(Tile::DoorOpen));
        assert_eq!(loaded.map.structure_index(Pos::new(2, 0, 0)), Some(4));
    }

    #[test]
    fn only_doors_can_be_placed_open() {
        let entry = r#""W": { "structure": "wall", "open": true }"#;
        let err = map_with_legend(entry, "@.W").unwrap_err();
        assert_eq!(
            err.to_string(),
            "legend entry 'W' is open, but only doors can be"
        );
    }

    #[test]
    fn unknown_structures_and_materials_are_errors_naming_the_glyph() {
        let err = map_with_legend(r#""H": "cheese_wall""#, "@.H").unwrap_err();
        assert_eq!(
            err.to_string(),
            r#"legend entry 'H' is "cheese_wall", which isn't a structure"#
        );
        let entry = r#""H": { "structure": "wall", "material": "cheese" }"#;
        let err = map_with_legend(entry, "@.H").unwrap_err();
        assert_eq!(
            err.to_string(),
            r#"legend entry 'H' is made of "cheese", which isn't a material"#
        );
    }

    #[test]
    fn a_legend_entry_with_a_misspelled_field_is_an_error() {
        let entry = r#""H": { "structure": "wall", "materal": "steel" }"#;
        let err = map_with_legend(entry, "@.H").unwrap_err();
        assert!(
            err.to_string()
                .contains("did not match any variant of untagged enum LegendEntry"),
            "{err}"
        );
    }

    #[test]
    fn the_test_helper_leaves_broken_json_for_the_real_parser() {
        assert!(matches!(parse_map("{ nope"), Err(MapError::Json(_))));
    }
}
