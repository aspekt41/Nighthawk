//! The determinism check (decision D22).
//!
//! Plays a fixed script of commands on the real starting map with a fixed
//! seed, then compares the world's fingerprint against a stored value. CI
//! runs this on Linux and macOS, so it also proves the game comes out the same
//! on different machines.
//!
//! ## If this test fails
//!
//! - **You didn't mean to change game behaviour:** something became
//!   non-deterministic or changed by accident. Find it; don't update the value.
//! - **You did** (a new rule, a new random roll, a map edit): update the
//!   stored value to the one in the failure message, and say why in the
//!   commit message.

use std::fs;
use std::path::PathBuf;

use nighthawk_sim::{Command, Dir, Event, World, parse_map};

const SEED: u64 = 0x4e49_4748_5448_4157; // "NIGHTHAW" in ASCII
const TURNS: usize = 500;

/// The stored fingerprints. See "If this test fails" above.
const EXPECTED_STATE_HASH: u64 = 0x6dfc_bf2b_6cf9_c682;
const EXPECTED_AMBIENT_COUNT: usize = 34;

fn start_world() -> World {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/maps/stasis_bay.json");
    let text = fs::read_to_string(path).expect("stasis bay map is readable");
    World::new(parse_map(&text).expect("stasis bay map is valid"), SEED)
}

/// A fixed, wandering walk with waits and wall bumps mixed in. It isn't
/// random: the same list every run.
fn script() -> impl Iterator<Item = Command> {
    use Command::{Move, Wait};
    let pattern = [
        Move(Dir::East),
        Move(Dir::East),
        Move(Dir::SouthEast),
        Wait,
        Move(Dir::North),
        Move(Dir::North),
        Move(Dir::North),
        Move(Dir::West),
        Move(Dir::NorthWest),
        Wait,
        Move(Dir::South),
        Move(Dir::East),
        Move(Dir::NorthEast),
    ];
    pattern.into_iter().cycle().take(TURNS)
}

/// Plays the script and returns the final world and every event it produced.
fn play() -> (World, Vec<Event>) {
    let mut world = start_world();
    let events = script().flat_map(|c| world.apply(c)).collect();
    (world, events)
}

#[test]
fn a_scripted_game_always_ends_in_the_same_state() {
    let (world, events) = play();
    let ambient = events
        .iter()
        .filter(|e| matches!(e, Event::Ambient(_)))
        .count();

    assert_eq!(
        (world.state_hash(), ambient),
        (EXPECTED_STATE_HASH, EXPECTED_AMBIENT_COUNT),
        "the scripted game changed: state hash {:#018x}, {ambient} ambient sounds, turn {}",
        world.state_hash(),
        world.turn(),
    );
}

#[test]
fn playing_twice_gives_identical_games() {
    let (a, a_events) = play();
    let (b, b_events) = play();
    assert_eq!(a, b);
    assert_eq!(a_events, b_events);
}
