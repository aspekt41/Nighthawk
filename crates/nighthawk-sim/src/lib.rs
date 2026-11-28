//! The Nighthawk simulation core.
//!
//! This crate is the game itself, with no terminal, no file I/O and no clock.
//! A front-end (the `nighthawk-tui` crate today, maybe a web client later)
//! reads content files and passes their text in, sends [`Command`]s, gets
//! [`Event`]s back, and reads the [`World`] to draw it. Keeping the sim this
//! pure is decision D18: it makes the game testable and lets more than one
//! front-end share it.
//!
//! Two rules from the decision log are visible in the code from day one:
//! - **D7, time only moves when you act.** The only thing that advances
//!   [`World::seconds`] is [`World::apply`]. Nothing runs in the background.
//! - **D29, units.** A tile is 1 m and the clock counts seconds; each action
//!   takes a set number of seconds, simulated one second at a time.
//! - **D8, the world is 3D.** Every [`Pos`] has a `z` (the deck).
//! - **Determinism (D22).** All randomness comes from [`GameRng`] streams
//!   seeded from the world's seed, so the same seed and commands always
//!   produce the same game.

mod action;
mod air;
mod body;
mod calendar;
mod content;
mod craft;
mod fov;
mod furniture;
mod hash;
mod heat;
mod interact;
mod item;
mod light;
mod map;
mod map_file;
mod material;
mod pos;
mod power;
mod quality;
mod recipe;
mod rng;
mod save;
mod schema;
mod sight;
mod structure;
mod world;

pub use action::{Action, ActionError, Effect};
pub use air::{Gas, Mix, TILE_VOLUME_CM3, VENT_SPEED_CM_PER_S, vent_ppm, vented_amount};
pub use body::{
    BREATH_FULL, BreathLevel, COMFORT_MK, Cause, SAFE_O2_PA, VACUUM_FEELS_LIKE_MK, WARMTH_FULL,
    WarmthLevel,
};
pub use calendar::{DateTime, SHIP_START};
pub use content::{CONTENT_FOLDERS, Content, ContentError, ContentFile, FileError};
pub use craft::{Craft, CraftError, CraftMaterial};
pub use furniture::{FurnitureError, FurnitureType, MAX_FURNITURE_SIDE, Piece};
pub use heat::{
    HULL_LOSS_PER_BILLION, MAX_CELSIUS, MIN_CELSIUS, MIXING_PER_MILLE, ROOM_TEMPERATURE_MK,
    SPACE_TEMPERATURE_MK, ZERO_CELSIUS_MK, celsius_to_mk, mk_to_tenths_celsius,
};
pub use interact::{ActionOption, Blocked, blocked, options_at};
pub use item::{
    FloorItem, Item, ItemContext, ItemError, ItemPath, ItemRef, ItemType, PocketType, Wear,
};
pub use light::MAX_LIGHT;
pub use map::{Map, StairsError, Tile};
pub use map_file::{ItemSpecError, LegendError, LoadedMap, MapError, parse_map};
pub use material::{Colour, Material, MaterialError};
pub use pos::{Dir, Pos, Vertical};
pub use power::{CellType, MAX_CELL_KJ, Power};
pub use quality::{LevelError, Levels, MAX_QUALITY_LEVEL, Quality, QualityError};
pub use recipe::{Count, Give, Recipe, RecipeError, RecipeMaterial, Use};
pub use rng::{GameRng, Stream};
pub use save::{SAVE_FORMAT, SAVE_VERSION, SaveError, load_from_str, save_to_string};
pub use schema::data_schemas;
pub use sight::Remembered;
pub use structure::{Structure, StructureError};
pub use world::{
    CLIMB_SECONDS, Command, DIAGONAL_STEP_SECONDS, DROP_SECONDS, Dress, Event, ItemAt,
    POCKET_SECONDS, PUT_ON_SECONDS, Place, PowerAct, REACH, RELOAD_SECONDS, STEP_SECONDS,
    STORE_SECONDS, SWITCH_SECONDS, Sound, TAKE_OFF_SECONDS, UNLOAD_SECONDS, WAIT_SECONDS,
    WAKE_SECONDS, World,
};
