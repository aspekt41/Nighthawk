//! Tool qualities (decision D9), as content in `data/qualities/` (D43).
//!
//! A quality is something a tool can do, at a level from 1 to
//! [`MAX_QUALITY_LEVEL`]: `LASER 1` is a small laser cutter. Actions ask
//! for a quality at a level, never for a particular tool, so there can be
//! many ways to get one. Items list the levels they have; materials list
//! the levels it takes to cut them. Both are [`Levels`], checked here.

use std::collections::BTreeMap;
use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::content::{ContentFile, FileError, first_duplicate, read_all};
use crate::hash::Fnv64;

/// The highest level a quality can have. Levels run from 1 to this.
pub const MAX_QUALITY_LEVEL: u8 = 10;

/// One quality a tool can have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Quality {
    /// How data files refer to it, and how the game shows it next to a
    /// level, like `"LASER"` in "LASER 2".
    pub id: String,
    /// What it is in words, like "laser cutting".
    pub name: String,
    /// A sentence or two about it.
    pub description: String,
}

/// Quality levels by quality id, like `{ "LASER": 2 }`: what a tool can
/// do, or what it takes to cut a material.
///
/// A `BTreeMap` rather than a `HashMap`, because it always lists its keys
/// in the same (sorted) order, which the state hash relies on; a
/// `HashMap`'s order changes from run to run.
pub type Levels = BTreeMap<String, u8>;

/// Every quality from the files in `data/qualities/`, saved as just the
/// list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Qualities {
    list: Vec<Quality>,
}

impl Qualities {
    pub fn list(&self) -> &[Quality] {
        &self.list
    }

    /// The number of the quality with this `id`, if there is one.
    pub fn index_of(&self, id: &str) -> Option<u16> {
        let i = self.list.iter().position(|q| q.id == id)?;
        Some(u16::try_from(i).expect("quality count was checked on load"))
    }

    /// Checks a list of levels: every quality is one of these, and every
    /// level is from 1 to [`MAX_QUALITY_LEVEL`].
    pub(crate) fn check(&self, levels: &Levels) -> Result<(), LevelError> {
        for (quality, &level) in levels {
            if self.index_of(quality).is_none() {
                return Err(LevelError::UnknownQuality {
                    quality: quality.clone(),
                });
            }
            if !(1..=MAX_QUALITY_LEVEL).contains(&level) {
                return Err(LevelError::OutOfRange {
                    quality: quality.clone(),
                    level,
                });
            }
        }
        Ok(())
    }
}

/// Feeds a list of levels into a state hash.
pub(crate) fn hash_levels(levels: &Levels, h: &mut Fnv64) {
    h.u64(levels.len() as u64);
    for (quality, &level) in levels {
        h.str(quality).u8(level);
    }
}

/// What can be wrong with one list of levels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelError {
    /// It names a quality that isn't in `data/qualities/`.
    UnknownQuality { quality: String },
    /// A level outside 1 to [`MAX_QUALITY_LEVEL`].
    OutOfRange { quality: String, level: u8 },
}

impl fmt::Display for LevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LevelError::UnknownQuality { quality } => {
                write!(f, "{quality:?} isn't a quality")
            }
            LevelError::OutOfRange { quality, level } => write!(
                f,
                "{quality} {level} is out of range; levels go from 1 to {MAX_QUALITY_LEVEL}"
            ),
        }
    }
}

/// Everything that can be wrong with the qualities files.
#[derive(Debug)]
pub enum QualityError {
    File(FileError),
    /// More qualities than an action's reason can refer to.
    TooMany {
        count: usize,
    },
    DuplicateId {
        id: String,
    },
}

impl fmt::Display for QualityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QualityError::File(e) => write!(f, "invalid qualities file {e}"),
            QualityError::TooMany { count } => {
                write!(f, "{count} qualities is too many; the most is {}", u16::MAX)
            }
            QualityError::DuplicateId { id } => write!(f, "quality {id:?} is defined twice"),
        }
    }
}

impl std::error::Error for QualityError {}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct QualitiesFile {
    /// Optional: the JSON Schema this file follows, so editors can check
    /// it as you type (`"$schema": "../schema/qualities.schema.json"`).
    #[serde(rename = "$schema", default)]
    _schema: Option<String>,
    qualities: Vec<Quality>,
}

/// Parses and checks every qualities file. There may be none at all: a
/// game with no tools.
pub(crate) fn parse_qualities(files: &[ContentFile]) -> Result<Qualities, QualityError> {
    let list = read_all(files, |f: QualitiesFile| f.qualities).map_err(QualityError::File)?;
    if u16::try_from(list.len()).is_err() {
        return Err(QualityError::TooMany { count: list.len() });
    }
    if let Some(id) = first_duplicate(list.iter().map(|q| q.id.as_str())) {
        return Err(QualityError::DuplicateId { id: id.into() });
    }
    Ok(Qualities { list })
}

/// Feeds a list of qualities into a state hash.
pub(crate) fn hash_qualities(qualities: &[Quality], h: &mut Fnv64) {
    h.u64(qualities.len() as u64);
    for q in qualities {
        h.str(&q.id).str(&q.name).str(&q.description);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The test qualities: LASER, and WELD for tests that need a second.
    pub(crate) fn test_qualities() -> Qualities {
        parse_qualities(&[ContentFile::new("tools.json", TEST_QUALITIES)]).unwrap()
    }

    const TEST_QUALITIES: &str = r#"{ "qualities": [
        { "id": "LASER", "name": "laser cutting", "description": "Cuts with light." },
        { "id": "WELD", "name": "welding", "description": "Joins metal." } ] }"#;

    #[test]
    fn qualities_load_in_file_order_and_are_found_by_id() {
        let q = test_qualities();
        assert_eq!(q.list().len(), 2);
        assert_eq!(q.index_of("LASER"), Some(0));
        assert_eq!(q.index_of("WELD"), Some(1));
        assert_eq!(q.index_of("laser"), None, "ids are exact");
    }

    #[test]
    fn no_qualities_at_all_is_fine() {
        let none = parse_qualities(&[]).unwrap();
        assert!(none.list().is_empty());
    }

    #[test]
    fn mistakes_are_errors_naming_the_file_or_the_quality() {
        let twice = format!(
            "{{ \"qualities\": [{q}, {q}] }}",
            q = r#"{ "id": "LASER", "name": "l", "description": "d" }"#
        );
        let err = parse_qualities(&[ContentFile::new("a.json", twice)]).unwrap_err();
        assert_eq!(err.to_string(), r#"quality "LASER" is defined twice"#);

        let typo = r#"{ "qualities": [ { "id": "LASER", "nmae": "l", "description": "d" } ] }"#;
        let err = parse_qualities(&[ContentFile::new("tools.json", typo)]).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("invalid qualities file tools.json: "),
            "{err}"
        );
    }

    #[test]
    fn too_many_qualities_is_an_error() {
        let one = r#"{ "id": "Q", "name": "q", "description": "d" }"#;
        let many: Vec<&str> = std::iter::repeat_n(one, usize::from(u16::MAX) + 1).collect();
        let text = format!("{{ \"qualities\": [{}] }}", many.join(","));
        let err = parse_qualities(&[ContentFile::new("a.json", text)]).unwrap_err();
        assert!(matches!(err, QualityError::TooMany { count: 65_536 }));
        assert!(err.to_string().contains("the most is 65535"));
    }

    #[test]
    fn levels_must_name_real_qualities_within_range() {
        let q = test_qualities();
        let levels = |pairs: &[(&str, u8)]| -> Levels {
            pairs.iter().map(|&(k, v)| (k.to_string(), v)).collect()
        };
        assert_eq!(q.check(&levels(&[("LASER", 1), ("WELD", 10)])), Ok(()));
        let unknown = q.check(&levels(&[("LAZER", 1)])).unwrap_err();
        assert_eq!(unknown.to_string(), r#""LAZER" isn't a quality"#);
        let zero = q.check(&levels(&[("LASER", 0)])).unwrap_err();
        assert_eq!(
            zero.to_string(),
            "LASER 0 is out of range; levels go from 1 to 10"
        );
        assert!(q.check(&levels(&[("WELD", 11)])).is_err());
    }

    #[test]
    fn levels_and_qualities_change_the_hash() {
        let hash = |levels: &Levels| {
            let mut h = Fnv64::new();
            hash_levels(levels, &mut h);
            h.finish()
        };
        let one: Levels = [("LASER".to_string(), 1)].into();
        let two: Levels = [("LASER".to_string(), 2)].into();
        assert_ne!(hash(&one), hash(&two));

        let hash_all = |list: &[Quality]| {
            let mut h = Fnv64::new();
            hash_qualities(list, &mut h);
            h.finish()
        };
        let mut renamed = test_qualities().list().to_vec();
        renamed[0].name.push('!');
        assert_ne!(hash_all(test_qualities().list()), hash_all(&renamed));
    }
}
