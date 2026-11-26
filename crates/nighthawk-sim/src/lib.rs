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
//!   `World::turn` is [`World::apply`]. Nothing runs in the background.
//! - **D8, the world is 3D.** Every [`Pos`] has a `z` (the deck).

mod map;
mod map_file;
mod pos;
mod world;

pub use map::{Map, Tile};
pub use map_file::{LoadedMap, MapError, parse_map};
pub use pos::{Dir, Pos};
pub use world::{Command, Event, World};
