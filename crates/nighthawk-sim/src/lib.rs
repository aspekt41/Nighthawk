//! The Nighthawk simulation core.
//!
//! This crate is the game itself, with no terminal, no I/O and no clock. A
//! front-end (the `nighthawk-tui` crate today, maybe a web client later) sends
//! [`Command`]s in and gets [`Event`]s back, then reads the [`World`] to draw
//! it. Keeping the sim this pure is decision D18: it makes the game testable
//! and lets more than one front-end share it.
//!
//! Two rules from the decision log are visible in the code from day one:
//! - **D7, time only moves when you act.** The only thing that advances
//!   `World::turn` is [`World::apply`]. Nothing runs in the background.
//! - **D8, the world is 3D.** Every [`Pos`] has a `z` (the deck), even while
//!   there is only one deck.

mod map;
mod pos;
mod world;

pub use map::{Map, Tile};
pub use pos::{Dir, Pos};
pub use world::{Command, Event, World};
