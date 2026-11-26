//! Content files: the JSON in `data/` (decision D17).
//!
//! Each kind of content (materials, structures, later items and recipes)
//! has a folder, and every `.json` file in it is read, so content can be
//! grouped into as many files as reads best (`metals.json`,
//! `fabrics.json`…). The sim never touches the disk (decision D18): the
//! front-end reads the files and passes in each one's name and text, and
//! the name is used to say which file a mistake is in.

use std::collections::HashSet;

use serde::de::DeserializeOwned;

/// One content file: its name, for error messages, and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentFile {
    pub name: String,
    pub text: String,
}

impl ContentFile {
    /// `impl Into<String>` lets callers pass a `&str` or a `String`.
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            text: text.into(),
        }
    }
}

/// A file that isn't valid JSON of the expected shape, with its name.
#[derive(Debug)]
pub struct FileError {
    pub file: String,
    pub error: serde_json::Error,
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.file, self.error)
    }
}

/// Reads every file as an `F` (the shape of one file, like
/// `{ "materials": [...] }`) and joins the lists `entries` takes out of
/// each, in file order.
///
/// It's generic, so materials, structures and later kinds of content all
/// share it: `F: DeserializeOwned` means "any type serde can build from
/// JSON", and `entries` is a function from one file to its list.
pub(crate) fn read_all<F: DeserializeOwned, T>(
    files: &[ContentFile],
    entries: impl Fn(F) -> Vec<T>,
) -> Result<Vec<T>, FileError> {
    let mut all = Vec::new();
    for file in files {
        let parsed: F = serde_json::from_str(&file.text).map_err(|error| FileError {
            file: file.name.clone(),
            error,
        })?;
        all.extend(entries(parsed));
    }
    Ok(all)
}

/// The first id that appears twice, if any.
pub(crate) fn first_duplicate<'a>(ids: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let mut seen = HashSet::new();
    ids.into_iter().find(|&id| !seen.insert(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Numbers {
        numbers: Vec<u32>,
    }

    #[test]
    fn every_file_is_read_and_their_lists_joined_in_order() {
        let files = [
            ContentFile::new("a.json", r#"{ "numbers": [1, 2] }"#),
            ContentFile::new("b.json", r#"{ "numbers": [3] }"#),
        ];
        let all = read_all(&files, |n: Numbers| n.numbers).unwrap();
        assert_eq!(all, vec![1, 2, 3]);
    }

    #[test]
    fn a_broken_file_is_named_in_the_error() {
        let files = [
            ContentFile::new("good.json", r#"{ "numbers": [] }"#),
            ContentFile::new("bad.json", r#"{ "numbers": "two" }"#),
        ];
        let err = read_all(&files, |n: Numbers| n.numbers).unwrap_err();
        assert_eq!(err.file, "bad.json");
        assert!(
            err.to_string().starts_with("bad.json: invalid type"),
            "{err}"
        );
    }

    #[test]
    fn duplicates_are_found() {
        assert_eq!(first_duplicate(["a", "b", "a"].into_iter()), Some("a"));
        assert_eq!(first_duplicate(["a", "b"].into_iter()), None);
    }
}
