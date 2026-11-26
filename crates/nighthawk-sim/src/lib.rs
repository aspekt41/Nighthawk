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

mod air;
mod body;
mod calendar;
mod hash;
mod heat;
mod interact;
mod map;
mod map_file;
mod material;
mod pos;
mod rng;
mod save;
mod world;

pub use air::{Gas, Mix, TILE_VOLUME_CM3, VENT_SPEED_CM_PER_S, vent_ppm, vented_amount};
pub use body::{
    BREATH_FULL, BreathLevel, COMFORT_MK, Cause, SAFE_O2_PA, VACUUM_FEELS_LIKE_MK, WARMTH_FULL,
    WarmthLevel,
};
pub use calendar::{DateTime, SHIP_START};
pub use heat::{
    HULL_LOSS_PER_BILLION, MAX_CELSIUS, MIN_CELSIUS, MIXING_PER_MILLE, ROOM_TEMPERATURE_MK,
    SPACE_TEMPERATURE_MK, ZERO_CELSIUS_MK, celsius_to_mk, mk_to_tenths_celsius,
};
pub use interact::{Action, ActionOption, Blocked, HAND_REACH, LASER_REACH, blocked, options_at};
pub use map::{Map, StairsError, Tile};
pub use map_file::{LoadedMap, MapError, parse_map};
pub use material::{
    Colour, MAX_QUALITY_LEVEL, Material, MaterialError, Materials, Quality, parse_materials,
};
pub use pos::{Dir, Pos, Vertical};
pub use rng::{GameRng, Stream};
pub use save::{SAVE_FORMAT, SAVE_VERSION, SaveError, load_from_str, save_to_string};
pub use world::{
    CLIMB_SECONDS, Command, DIAGONAL_STEP_SECONDS, DOOR_SECONDS, Event, STEP_SECONDS, Sound,
    WAIT_SECONDS, World,
};
