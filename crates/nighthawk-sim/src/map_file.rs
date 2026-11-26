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
use crate::map::{Map, StairsError, Tile};
use crate::pos::Pos;

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
}

/// Everything that can be wrong with a map file. Each variant says exactly
/// where the problem is, so a content author can fix it without guessing.
#[derive(Debug)]
pub enum MapError {
    /// Not valid JSON, or the wrong shape: a missing field, a misspelled
    /// field name, or an unknown tile name in the legend.
    Json(serde_json::Error),
    /// A legend key that isn't exactly one character, or is the reserved `@`.
    BadLegendKey {
        key: String,
    },
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
    /// A character that isn't in the legend and isn't `@`.
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
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapError::Json(e) => write!(f, "invalid map JSON: {e}"),
            MapError::BadLegendKey { key } => write!(
                f,
                "legend key {key:?} must be exactly one character and not '{PLAYER_START}'"
            ),
            MapError::NoDecks => write!(f, "map has no decks"),
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
            MapError::PlayerStartCount { found } => write!(
                f,
                "map must have exactly one '{PLAYER_START}' (player start); found {found}"
            ),
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
    legend: HashMap<String, Tile>,
    decks: Vec<DeckFile>,
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
    rows: Vec<String>,
}

/// The highest starting pressure a deck may ask for, in percent.
const MAX_PRESSURE_PERCENT: u32 = 200;

/// Parses and validates a map file's text.
pub fn parse_map(json: &str) -> Result<LoadedMap, MapError> {
    let file: MapFile = serde_json::from_str(json).map_err(MapError::Json)?;
    let legend = glyph_legend(&file.legend)?;
    let (width, height) = dimensions(&file.decks)?;

    let depth = file.decks.len();
    let mut map = Map::filled(to_i32(width), to_i32(height), to_i32(depth), Tile::Floor);
    let mut starts = Vec::new();

    for (z, y, x, glyph) in cells(&file.decks) {
        let pos = Pos::new(to_i32(x), to_i32(y), to_i32(z));
        map.set(pos, tile_for(glyph, &legend, (z, y, x))?);
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
/// deck's `pressure` (100% unless the file says otherwise).
fn fill_air(map: &mut Map, decks: &[DeckFile]) -> Result<(), MapError> {
    for (z, deck) in decks.iter().enumerate() {
        let percent = deck.pressure.unwrap_or(100);
        if percent > MAX_PRESSURE_PERCENT {
            return Err(MapError::BadPressure { deck: z, percent });
        }
        map.fill_deck_air(to_i32(z), Mix::STANDARD.scaled(percent));
    }
    Ok(())
}

/// Each deck's name, or "Deck N" for decks without one.
fn deck_names(decks: &[DeckFile]) -> Vec<String> {
    decks
        .iter()
        .enumerate()
        .map(|(z, deck)| deck.name.clone().unwrap_or_else(|| format!("Deck {z}")))
        .collect()
}

/// Turns the legend's one-character string keys into `char`s.
fn glyph_legend(legend: &HashMap<String, Tile>) -> Result<HashMap<char, Tile>, MapError> {
    legend
        .iter()
        .map(|(key, &tile)| {
            let mut chars = key.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c != PLAYER_START => Ok((c, tile)),
                _ => Err(MapError::BadLegendKey { key: key.clone() }),
            }
        })
        .collect()
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
    legend: &HashMap<char, Tile>,
    (deck, row, col): (usize, usize, usize),
) -> Result<Tile, MapError> {
    if glyph == PLAYER_START {
        return Ok(Tile::Floor);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
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
    fn unknown_tile_name_is_rejected() {
        let json = r###"{ "id": "t", "name": "T", "legend": { "#": "wal" },
            "decks": [ { "rows": ["@"] } ] }"###;
        assert!(matches!(parse_map(json), Err(MapError::Json(_))));
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
        assert!(matches!(err, MapError::BadLegendKey { ref key } if key == "##"));
    }

    #[test]
    fn legend_cannot_redefine_the_player_start() {
        let json = r#"{ "id": "t", "name": "T", "legend": { "@": "wall" },
            "decks": [ { "rows": ["@"] } ] }"#;
        assert!(matches!(
            parse_map(json),
            Err(MapError::BadLegendKey { .. })
        ));
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
            (MapError::BadLegendKey { key: "ab".into() }, "\"ab\""),
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
