//! JSON Schemas for the data files (decisions D17 and D48).
//!
//! A JSON Schema describes what a JSON file may contain: which fields, of
//! what types, which are required. Editors use one to autocomplete field
//! names and flag mistakes as you type. These schemas are generated from
//! the same Rust types that load the files (`#[derive(JsonSchema)]` on each,
//! from the `schemars` crate), so they can't disagree with what the game
//! accepts: serde's attributes (`deny_unknown_fields`, `default`,
//! `rename_all`) are honoured, and each field's doc comment becomes its
//! description.
//!
//! The sim doesn't write files (decision D18): this only builds the text.
//! The copies in `data/schema/` are checked against it by a test, which can
//! also rewrite them (see `tests/schemas.rs`).

use schemars::{Schema, schema_for};

use crate::action::ActionsFile;
use crate::content::CONTENT_FOLDERS;
use crate::craft::CraftsFile;
use crate::furniture::FurnitureFile;
use crate::item::ItemsFile;
use crate::map_file::MapFile;
use crate::material::MaterialsFile;
use crate::quality::QualitiesFile;
use crate::recipe::RecipesFile;
use crate::structure::StructuresFile;

/// Each data schema: its file name in `data/schema/` (like
/// `"materials.schema.json"`) and its text. One per content folder, in
/// [`CONTENT_FOLDERS`] order, then one for maps.
pub fn data_schemas() -> Vec<(String, String)> {
    // Taking CONTENT_FOLDERS apart like this means a new content folder
    // won't compile until it has a schema here too.
    let [
        qualities,
        actions,
        materials,
        structures,
        furniture,
        items,
        recipes,
        crafts,
    ] = CONTENT_FOLDERS;
    let schemas: [(&str, Schema); 9] = [
        (qualities, schema_for!(QualitiesFile)),
        (actions, schema_for!(ActionsFile)),
        (materials, schema_for!(MaterialsFile)),
        (structures, schema_for!(StructuresFile)),
        (furniture, schema_for!(FurnitureFile)),
        (items, schema_for!(ItemsFile)),
        (recipes, schema_for!(RecipesFile)),
        (crafts, schema_for!(CraftsFile)),
        ("map", schema_for!(MapFile)),
    ];
    schemas
        .into_iter()
        .map(|(name, schema)| (format!("{name}.schema.json"), pretty(&schema)))
        .collect()
}

/// A schema as indented JSON with a final newline, as the files are kept.
fn pretty(schema: &Schema) -> String {
    let text = serde_json::to_string_pretty(schema).expect("a schema is always valid JSON");
    text + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_one_schema_per_content_folder_and_one_for_maps() {
        let names: Vec<String> = data_schemas().into_iter().map(|(n, _)| n).collect();
        let mut expected: Vec<String> = CONTENT_FOLDERS
            .iter()
            .map(|f| format!("{f}.schema.json"))
            .collect();
        expected.push("map.schema.json".into());
        assert_eq!(names, expected);
    }

    #[test]
    fn schemas_follow_serdes_rules_and_carry_the_doc_comments() {
        let schemas = data_schemas();
        let structures = &schemas[3].1;
        let value: serde_json::Value = serde_json::from_str(structures).unwrap();
        assert_eq!(value["additionalProperties"], false, "deny_unknown_fields");
        let structure = &value["$defs"]["Structure"];
        assert_eq!(structure["additionalProperties"], false);
        let required = structure["required"].as_array().unwrap();
        assert!(required.contains(&"glyph".into()), "glyph has no default");
        assert!(!required.contains(&"walkable".into()), "walkable defaults");
        let walkable = structure["properties"]["walkable"]["description"]
            .as_str()
            .unwrap();
        assert!(walkable.starts_with("You can walk on it"), "{walkable}");
        assert!(
            value["properties"]["$schema"].is_object(),
            "files may name it"
        );
        assert!(structures.ends_with("}\n"));
    }
}
