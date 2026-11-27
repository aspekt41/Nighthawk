//! Content: the JSON in `data/` (decisions D17 and D43).
//!
//! Each kind of content (qualities, actions, materials, structures, items,
//! recipes) has a
//! folder, and every `.json` file in it is read, so content can be grouped
//! into as many files as reads best (`metals.json`, `fabrics.json`…). The
//! sim never touches the disk (decision D18): the front-end reads the files
//! and passes in each one's name and text, and the name is used to say
//! which file a mistake is in.
//!
//! All of it is loaded in one go, by [`Content::load`], into one
//! [`Content`]. A front-end only says how to read a folder; which folders
//! there are, in what order, and how each is checked is decided here. So a
//! new kind of content is added in this file (and its own module), and no
//! front-end or test helper has to change.

use std::collections::HashSet;
use std::fmt;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::action::{self, Action, ActionError, Actions, parse_actions};
use crate::hash::Fnv64;
use crate::item::{self, ItemError, ItemType, ItemTypes, parse_items};
use crate::material::{Material, MaterialError, Materials, parse_materials};
use crate::quality::{self, Qualities, Quality, QualityError, parse_qualities};
use crate::recipe::{self, Recipe, RecipeError, RecipeRefs, Recipes, parse_recipes};
use crate::structure::{Structure, StructureError, Structures, parse_structures};

/// The content folders, in the order they're loaded. A kind may refer to
/// the kinds before it (materials name actions and qualities; structures
/// name materials; items name materials and qualities; recipes name all of
/// those), which is why the order matters.
pub const CONTENT_FOLDERS: [&str; 6] = [
    "qualities",
    "actions",
    "materials",
    "structures",
    "items",
    "recipes",
];

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

/// Everything a game is made of: every tool quality, action, material,
/// structure, item type and recipe, checked against each other.
///
/// The world keeps its own copy, saved with the game, so editing the data
/// files never changes a game already in progress (decision D35). Tiles,
/// items and recipes refer to entries by their place in these lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Content {
    pub(crate) qualities: Qualities,
    pub(crate) actions: Actions,
    pub(crate) materials: Materials,
    pub(crate) structures: Structures,
    pub(crate) item_types: ItemTypes,
    pub(crate) recipes: Recipes,
}

impl Content {
    /// Loads and checks every kind of content. `read_folder` is how the
    /// front-end reads one folder of [`CONTENT_FOLDERS`]: given its name, it
    /// returns the files in it, or the front-end's own error.
    ///
    /// The `E: From<ContentError>` bound means "any error type that a
    /// [`ContentError`] can be turned into". It lets the `?` operator turn a
    /// mistake in the content into the caller's error type, so callers get
    /// one kind of error back whether a file couldn't be read or was wrong.
    pub fn load<E: From<ContentError>>(
        mut read_folder: impl FnMut(&str) -> Result<Vec<ContentFile>, E>,
    ) -> Result<Self, E> {
        let mut files = Vec::with_capacity(CONTENT_FOLDERS.len());
        for folder in CONTENT_FOLDERS {
            files.push(read_folder(folder)?);
        }
        let files = files.try_into().expect("one list of files per folder");
        Ok(Self::parse(files)?)
    }

    /// Parses and checks the files of every folder, given in
    /// [`CONTENT_FOLDERS`] order.
    fn parse(files: [Vec<ContentFile>; CONTENT_FOLDERS.len()]) -> Result<Self, ContentError> {
        // Taking the array apart like this ties each step to its place in
        // CONTENT_FOLDERS: a sixth folder there won't compile until it's
        // parsed here too.
        let [qualities, actions, materials, structures, items, recipes] = files;
        let qualities = parse_qualities(&qualities).map_err(ContentError::Qualities)?;
        let actions = parse_actions(&actions).map_err(ContentError::Actions)?;
        let materials =
            parse_materials(&materials, &qualities, &actions).map_err(ContentError::Materials)?;
        let structures =
            parse_structures(&structures, &materials).map_err(ContentError::Structures)?;
        let item_types =
            parse_items(&items, &materials, &qualities).map_err(ContentError::Items)?;
        let refs = RecipeRefs {
            actions: &actions,
            qualities: &qualities,
            structures: &structures,
            items: &item_types,
        };
        let recipes = parse_recipes(&recipes, &refs).map_err(ContentError::Recipes)?;
        Ok(Self {
            qualities,
            actions,
            materials,
            structures,
            item_types,
            recipes,
        })
    }

    /// Every tool quality, like LASER.
    pub fn qualities(&self) -> &[Quality] {
        self.qualities.list()
    }

    /// Every action, in the order the interact menu lists them. Commands
    /// and events refer to actions by their place in this list.
    pub fn actions(&self) -> &[Action] {
        self.actions.list()
    }

    /// The action with this id, if there is one.
    pub fn action(&self, id: &str) -> Option<&Action> {
        self.actions.get(id)
    }

    /// The number of the action with this id, if there is one.
    pub fn action_index(&self, id: &str) -> Option<u16> {
        self.actions.index_of(id)
    }

    /// Every material, in the order tiles and items refer to them.
    pub fn materials(&self) -> &[Material] {
        self.materials.list()
    }

    /// Every structure, in the order tiles refer to them.
    pub fn structures(&self) -> &[Structure] {
        self.structures.list()
    }

    /// Every kind of item, in the order items refer to them.
    pub fn item_types(&self) -> &[ItemType] {
        self.item_types.list()
    }

    /// Every recipe: how one structure becomes another.
    pub fn recipes(&self) -> &[Recipe] {
        self.recipes.list()
    }

    /// The number of the quality with this id, if there is one.
    pub fn quality_index(&self, id: &str) -> Option<u16> {
        self.qualities.index_of(id)
    }

    /// The number of the material with this id, if there is one.
    pub fn material_index(&self, id: &str) -> Option<u16> {
        self.materials.index_of(id)
    }

    /// The number of the structure with this id, if there is one.
    pub fn structure_index(&self, id: &str) -> Option<u16> {
        self.structures.index_of(id)
    }

    /// The number of the item type with this id, if there is one.
    pub fn item_type_index(&self, id: &str) -> Option<u16> {
        self.item_types.index_of(id)
    }

    /// Feeds all of it into a state hash: content is game state, since a
    /// save carries its own copy (decision D35).
    pub(crate) fn hash_into(&self, h: &mut Fnv64) {
        quality::hash_qualities(self.qualities(), h);
        action::hash_actions(self.actions(), h);
        h.u64(self.materials().len() as u64);
        for material in self.materials() {
            material.hash_into(h);
        }
        h.u64(self.structures().len() as u64);
        for structure in self.structures() {
            structure.hash_into(h);
        }
        item::hash_types(self.item_types(), h);
        recipe::hash_recipes(self.recipes(), h);
    }

    /// The materials, changeable, for tests in other modules.
    #[cfg(test)]
    pub(crate) fn materials_mut(&mut self) -> &mut Vec<Material> {
        self.materials.list_mut()
    }

    /// The small set of test content every unit test's map uses (see
    /// `parse_map_for_tests`).
    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        Self {
            qualities: crate::quality::tests::test_qualities(),
            actions: crate::action::tests::test_actions(),
            materials: crate::material::tests::test_materials(),
            structures: crate::structure::tests::test_structures(),
            item_types: crate::item::tests::test_items(),
            recipes: crate::recipe::tests::test_recipes(),
        }
    }
}

/// A mistake in one of the content folders. Each error names its file.
#[derive(Debug)]
pub enum ContentError {
    Qualities(QualityError),
    Actions(ActionError),
    Materials(MaterialError),
    Structures(StructureError),
    Items(ItemError),
    Recipes(RecipeError),
}

impl ContentError {
    /// The folder the mistake is in, like `"materials"`.
    pub fn folder(&self) -> &'static str {
        let [qualities, actions, materials, structures, items, recipes] = CONTENT_FOLDERS;
        match self {
            ContentError::Qualities(_) => qualities,
            ContentError::Actions(_) => actions,
            ContentError::Materials(_) => materials,
            ContentError::Structures(_) => structures,
            ContentError::Items(_) => items,
            ContentError::Recipes(_) => recipes,
        }
    }
}

impl fmt::Display for ContentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContentError::Qualities(e) => write!(f, "{e}"),
            ContentError::Actions(e) => write!(f, "{e}"),
            ContentError::Materials(e) => write!(f, "{e}"),
            ContentError::Structures(e) => write!(f, "{e}"),
            ContentError::Items(e) => write!(f, "{e}"),
            ContentError::Recipes(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ContentError {}

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

    /// The smallest valid contents of each folder: one steel material, one
    /// structure, one item, no recipes.
    fn minimal(folder: &str) -> Vec<ContentFile> {
        let text = match folder {
            "qualities" => {
                r#"{ "qualities": [ { "id": "LASER", "name": "laser", "description": "L." } ] }"#
            }
            "actions" => r#"{ "actions": [] }"#,
            "materials" => {
                r#"{ "materials": [ { "id": "steel", "name": "Steel", "description": "S.",
                     "colour": "grey", "density": 7850, "specific_heat": 490,
                     "thermal_conductivity": 45000, "hardness": 150 } ] }"#
            }
            "structures" => {
                r#"{ "structures": [ { "id": "deck", "name": "deck", "description": "D.",
                     "glyph": ".", "behaviour": "floor", "material": "steel" } ] }"#
            }
            "items" => {
                r#"{ "items": [ { "id": "bolt", "name": "bolt", "description": "B.",
                     "glyph": "*", "material": "steel", "volume_ml": 1, "solid_percent": 100 } ] }"#
            }
            _ => r#"{ "recipes": [] }"#,
        };
        vec![ContentFile::new(format!("{folder}.json"), text)]
    }

    #[test]
    fn loading_reads_each_content_folder_once_in_order() {
        let mut asked = Vec::new();
        let content = Content::load(|folder| {
            asked.push(folder.to_string());
            Ok::<_, ContentError>(minimal(folder))
        })
        .unwrap();
        assert_eq!(asked, CONTENT_FOLDERS);
        assert_eq!(content.qualities()[0].id, "LASER");
        assert_eq!(content.quality_index("LASER"), Some(0));
        assert!(content.actions().is_empty());
        assert_eq!(content.materials()[0].id, "steel");
        assert_eq!(content.structures()[0].id, "deck");
        assert_eq!(content.item_types()[0].id, "bolt");
        assert!(content.recipes().is_empty());
        assert_eq!(content.material_index("steel"), Some(0));
        assert_eq!(content.structure_index("deck"), Some(0));
        assert_eq!(content.item_type_index("bolt"), Some(0));
        assert_eq!(content.item_type_index("nut"), None);
    }

    /// A front-end's own error type: it couldn't read a folder, or the
    /// content in it was wrong.
    #[derive(Debug)]
    enum FrontEndError {
        Unreadable(String),
        Wrong(ContentError),
    }

    impl From<ContentError> for FrontEndError {
        fn from(error: ContentError) -> Self {
            FrontEndError::Wrong(error)
        }
    }

    #[test]
    fn a_folder_that_cannot_be_read_stops_loading_with_the_callers_error() {
        let err = Content::load(|folder| match folder {
            "items" => Err(FrontEndError::Unreadable(folder.into())),
            _ => Ok(minimal(folder)),
        })
        .unwrap_err();
        assert!(matches!(err, FrontEndError::Unreadable(ref f) if f == "items"));
    }

    /// Loads minimal content with an empty object in place of the files
    /// of the folder `broken`.
    fn load_with_broken(broken: &str) -> Result<Content, FrontEndError> {
        let files = |folder: &str| match folder == broken {
            true => vec![ContentFile::new("zz.json", "{}")],
            false => minimal(folder),
        };
        Content::load(|folder| Ok(files(folder)))
    }

    #[test]
    fn a_mistake_in_any_folder_names_the_folder_and_the_file() {
        for broken in CONTENT_FOLDERS {
            let FrontEndError::Wrong(err) = load_with_broken(broken).unwrap_err() else {
                panic!("{broken}: expected a content error");
            };
            assert_eq!(err.folder(), broken);
            assert!(
                err.to_string()
                    .starts_with(&format!("invalid {broken} file zz.json")),
                "{err}"
            );
        }
    }
}
