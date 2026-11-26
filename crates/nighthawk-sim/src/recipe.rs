//! Recipes: how one thing becomes another (decisions D38 and D40).
//!
//! Every change from one structure (or later, item) to another is a recipe
//! in `data/recipes/*.json`: an action, what it acts on, how long it takes,
//! and what it becomes. Cutting a door with a laser is a recipe; patching
//! the hull with scrap will be one; so will building and crafting. Keeping
//! them all as data means the interact menu just looks up which recipes
//! apply to the tile you point at.
//!
//! What a recipe *needs* depends on its action. For cutting, the tool level
//! comes from the material being cut (decision D35): an aluminium door
//! needs `LASER 1`, a steel one `LASER 2`.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::content::{ContentFile, FileError, first_duplicate, read_all};
use crate::hash::Fnv64;
use crate::structure::Structures;

/// What a recipe does. Each action has its own rules in the code; recipes
/// say what it turns what into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeAction {
    /// Cut through a structure with a tool that can cut its material.
    Cut,
}

impl RecipeAction {
    /// A fixed number per action, for the state hash.
    fn hash_id(self) -> u8 {
        match self {
            RecipeAction::Cut => 0,
        }
    }
}

/// One recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub id: String,
    pub action: RecipeAction,
    /// The id of the structure it acts on.
    pub from: String,
    /// The id of the structure it becomes. It keeps its material.
    pub to: String,
    /// How long it takes, in seconds.
    pub seconds: u32,
}

impl Recipe {
    pub(crate) fn hash_into(&self, h: &mut Fnv64) {
        h.str(&self.id)
            .u8(self.action.hash_id())
            .str(&self.from)
            .str(&self.to)
            .u64(u64::from(self.seconds));
    }
}

/// Every recipe from the files in `data/recipes/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipes {
    list: Vec<Recipe>,
}

impl Recipes {
    pub fn list(&self) -> &[Recipe] {
        &self.list
    }
}

/// Everything that can be wrong with the recipe files.
#[derive(Debug)]
pub enum RecipeError {
    File(FileError),
    DuplicateId {
        id: String,
    },
    /// A recipe naming a structure that isn't defined.
    UnknownStructure {
        recipe: String,
        structure: String,
    },
    /// A recipe that takes no time.
    NoTime {
        recipe: String,
    },
    /// Two recipes doing the same action to the same structure: which one
    /// the menu offered would be a guess.
    Ambiguous {
        recipe: String,
    },
}

impl fmt::Display for RecipeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecipeError::File(e) => write!(f, "invalid recipes file {e}"),
            RecipeError::DuplicateId { id } => write!(f, "recipe {id:?} is defined twice"),
            RecipeError::UnknownStructure { recipe, structure } => write!(
                f,
                "recipe {recipe:?} names {structure:?}, which isn't a structure"
            ),
            RecipeError::NoTime { recipe } => {
                write!(f, "recipe {recipe:?} must take at least 1 second")
            }
            RecipeError::Ambiguous { recipe } => write!(
                f,
                "recipe {recipe:?} does the same action to the same structure as an earlier one"
            ),
        }
    }
}

impl std::error::Error for RecipeError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipesFile {
    recipes: Vec<Recipe>,
}

/// Parses and checks every recipes file. Each recipe's structures must be
/// in `structures`.
pub fn parse_recipes(
    files: &[ContentFile],
    structures: &Structures,
) -> Result<Recipes, RecipeError> {
    let list = read_all(files, |f: RecipesFile| f.recipes).map_err(RecipeError::File)?;
    if let Some(id) = first_duplicate(list.iter().map(|r| r.id.as_str())) {
        return Err(RecipeError::DuplicateId { id: id.into() });
    }
    for (i, recipe) in list.iter().enumerate() {
        check_one(recipe, structures)?;
        let earlier = &list[..i];
        if earlier
            .iter()
            .any(|r| r.action == recipe.action && r.from == recipe.from)
        {
            return Err(RecipeError::Ambiguous {
                recipe: recipe.id.clone(),
            });
        }
    }
    Ok(Recipes { list })
}

fn check_one(recipe: &Recipe, structures: &Structures) -> Result<(), RecipeError> {
    let unknown = [&recipe.from, &recipe.to]
        .into_iter()
        .find(|id| structures.index_of(id).is_none());
    if let Some(structure) = unknown {
        return Err(RecipeError::UnknownStructure {
            recipe: recipe.id.clone(),
            structure: structure.clone(),
        });
    }
    if recipe.seconds == 0 {
        return Err(RecipeError::NoTime {
            recipe: recipe.id.clone(),
        });
    }
    Ok(())
}

/// Feeds a list of recipes into a state hash.
pub(crate) fn hash_recipes(recipes: &[Recipe], h: &mut Fnv64) {
    h.u64(recipes.len() as u64);
    for recipe in recipes {
        recipe.hash_into(h);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::structure::tests::test_structures;

    /// Test recipes: cutting a door or a wall leaves floor, in 60 seconds.
    pub(crate) fn test_recipes_json() -> String {
        r#"{ "recipes": [
            { "id": "cut_door", "action": "cut", "from": "door", "to": "floor", "seconds": 60 },
            { "id": "cut_wall", "action": "cut", "from": "wall", "to": "floor", "seconds": 60 }
        ] }"#
            .to_string()
    }

    pub(crate) fn test_recipes() -> Recipes {
        parse_recipes(
            &[ContentFile::new("test.json", test_recipes_json())],
            &test_structures(),
        )
        .expect("test recipes are valid")
    }

    fn error_of(json: &str) -> String {
        parse_recipes(
            &[ContentFile::new("cutting.json", json)],
            &test_structures(),
        )
        .unwrap_err()
        .to_string()
    }

    #[test]
    fn a_valid_file_loads() {
        let recipes = test_recipes();
        assert_eq!(recipes.list().len(), 2);
        assert_eq!(recipes.list()[0].action, RecipeAction::Cut);
        assert_eq!(recipes.list()[0].to, "floor");
    }

    #[test]
    fn no_recipes_at_all_is_fine() {
        let recipes = parse_recipes(&[], &test_structures()).unwrap();
        assert!(recipes.list().is_empty());
    }

    #[test]
    fn mistakes_are_errors_naming_the_file_and_recipe() {
        assert!(error_of("[").starts_with("invalid recipes file cutting.json: "));
        let unknown = test_recipes_json().replace(
            "\"to\": \"floor\", \"seconds\": 60 },",
            "\"to\": \"rubble\", \"seconds\": 60 },",
        );
        assert_eq!(
            error_of(&unknown),
            r#"recipe "cut_door" names "rubble", which isn't a structure"#
        );
        let instant = test_recipes_json().replace("\"seconds\": 60 },", "\"seconds\": 0 },");
        assert_eq!(
            error_of(&instant),
            r#"recipe "cut_door" must take at least 1 second"#
        );
        let twice = test_recipes_json().replace("cut_wall", "cut_door");
        assert_eq!(error_of(&twice), r#"recipe "cut_door" is defined twice"#);
        let action = test_recipes_json().replace(
            "\"cut\", \"from\": \"door\"",
            "\"bake\", \"from\": \"door\"",
        );
        assert!(error_of(&action).contains("bake"));
    }

    #[test]
    fn two_recipes_for_the_same_action_on_the_same_thing_are_an_error() {
        let both = test_recipes_json().replace("\"from\": \"wall\"", "\"from\": \"door\"");
        assert_eq!(
            error_of(&both),
            r#"recipe "cut_wall" does the same action to the same structure as an earlier one"#
        );
    }

    #[test]
    fn everything_about_a_recipe_is_part_of_the_hash() {
        let hash = |recipes: &[Recipe]| {
            let mut h = Fnv64::new();
            hash_recipes(recipes, &mut h);
            h.finish()
        };
        let base = test_recipes().list().to_vec();
        let changes: [fn(&mut Recipe); 4] = [
            |r| r.seconds += 1,
            |r| r.to.push('!'),
            |r| r.from.push('!'),
            |r| r.id.push('!'),
        ];
        for change in changes {
            let mut changed = base.clone();
            change(&mut changed[0]);
            assert_ne!(hash(&base), hash(&changed));
        }
        assert_eq!(RecipeAction::Cut.hash_id(), 0);
    }
}
