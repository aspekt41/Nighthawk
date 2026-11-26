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
mod hash;
mod map;
mod map_file;
mod pos;
mod rng;
mod save;
mod world;

pub use air::{Gas, Mix};
pub use map::{Map, StairsError, Tile};
pub use map_file::{LoadedMap, MapError, parse_map};
pub use pos::{Dir, Pos, Vertical};
pub use rng::{GameRng, Stream};
pub use save::{SAVE_FORMAT, SAVE_VERSION, SaveError, load_from_str, save_to_string};
pub use world::{
    CLIMB_SECONDS, Command, DIAGONAL_STEP_SECONDS, DOOR_SECONDS, Event, STEP_SECONDS, Sound,
    WAIT_SECONDS, World,
};
