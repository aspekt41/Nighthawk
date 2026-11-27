//! The JSON Schemas in `data/schema/` are kept up to date (decision D48).
//!
//! They're generated from the Rust types that load each kind of file
//! (`nighthawk_sim::data_schemas`). If this test fails because one is out
//! of date, regenerate them all, then read the diff:
//!
//! ```sh
//! NIGHTHAWK_WRITE_SCHEMAS=1 cargo test -p nighthawk-sim --test schemas
//! ```

mod common;

use std::fs;
use std::path::PathBuf;

use nighthawk_sim::{CONTENT_FOLDERS, data_schemas};

fn schema_dir() -> PathBuf {
    common::data_dir().join("schema")
}

#[test]
fn the_schemas_in_data_schema_are_up_to_date() {
    // Reading an environment variable is fine; only setting one needs
    // `unsafe`. When it's set, write the schemas instead of checking.
    let write = std::env::var_os("NIGHTHAWK_WRITE_SCHEMAS").is_some();
    for (name, text) in data_schemas() {
        let path = schema_dir().join(&name);
        if write {
            fs::create_dir_all(schema_dir()).unwrap();
            fs::write(&path, &text).unwrap();
        }
        let on_disk = fs::read_to_string(&path).unwrap_or_default();
        assert!(
            on_disk == text,
            "data/schema/{name} is out of date; regenerate with\n  \
             NIGHTHAWK_WRITE_SCHEMAS=1 cargo test -p nighthawk-sim --test schemas"
        );
    }
}

#[test]
fn no_schema_is_left_over_from_content_that_has_gone() {
    let expected: Vec<String> = data_schemas().into_iter().map(|(n, _)| n).collect();
    for entry in fs::read_dir(schema_dir()).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        assert!(
            expected.contains(&name),
            "data/schema/{name} isn't generated any more"
        );
    }
}

#[test]
fn every_data_file_names_its_schema() {
    // `"$schema"` is how editors find a file's schema.
    let folders = CONTENT_FOLDERS
        .iter()
        .map(|f| (*f, *f))
        .chain([("maps", "map")]);
    for (folder, schema) in folders {
        let expected = format!("../schema/{schema}.schema.json");
        for entry in fs::read_dir(common::data_dir().join(folder)).unwrap() {
            let path = entry.unwrap().path();
            let text = fs::read_to_string(&path).unwrap();
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(
                value["$schema"].as_str(),
                Some(expected.as_str()),
                "{}",
                path.display()
            );
        }
    }
    // And the shipped content still loads with the "$schema" lines in it.
    assert_eq!(common::start_world(1).area_name(), "Stasis bay");
}
