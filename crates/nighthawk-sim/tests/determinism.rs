//! The determinism check (decisions D22 and D23).
//!
//! Plays a fixed script of commands on the real starting map with a fixed
//! seed, then compares the world's fingerprint against a stored value. It
//! also saves and reloads partway through, which must change nothing. CI
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

mod common;

use nighthawk_sim::{Command, Dir, Event, Vertical, World, load_from_str, save_to_string};

const SEED: u64 = 0x4e49_4748_5448_4157; // "NIGHTHAW" in ASCII
const TURNS: usize = 500;

/// The stored fingerprints. See "If this test fails" above.
const EXPECTED_STATE_HASH: u64 = 0x2a4a_aa8b_efa2_319d;
const EXPECTED_AMBIENT_COUNT: usize = 74;

fn start_world() -> World {
    common::start_world(SEED)
}

/// One step of the script: a command, or closing the door next to the
/// player in a direction, the way the `c` key does. That needs the world:
/// which tile is "that way" depends on where the player is by then.
#[derive(Clone, Copy)]
enum Step {
    Do(Command),
    Close(Dir),
}

/// Plays one step.
fn apply(world: &mut World, step: Step) -> Vec<Event> {
    let command = match step {
        Step::Do(command) => command,
        Step::Close(dir) => Command::Interact {
            at: world.player().step(dir),
            action: world
                .content()
                .action_index("close")
                .expect("a close action"),
        },
    };
    world.apply(command)
}

/// A fixed script: walk to the ladder, then loop around it forever,
/// walking, waiting, bumping walls, climbing both ways, and opening and
/// closing a door. It isn't random:
/// the same list every run.
fn script() -> impl Iterator<Item = Step> {
    use Command::{Climb, Move, Wait};
    // From the start (10, 7) to the ladder at (33, 2), then up.
    let to_the_ladder = std::iter::repeat_n(Move(Dir::North), 5)
        .chain(std::iter::repeat_n(Move(Dir::East), 23))
        .chain([Climb(Vertical::Up)])
        .map(Step::Do);
    // A small loop that starts and ends on the ladder, on either deck.
    let around = [
        Move(Dir::East),
        Move(Dir::East),
        Move(Dir::South),
        Wait,
        Move(Dir::West),
        Move(Dir::West),
        Move(Dir::North),
    ]
    .map(Step::Do);
    // North into the wall twice (no time passes), then back to the ladder.
    let bump_and_back = [
        Move(Dir::North),
        Move(Dir::North),
        Move(Dir::North),
        Move(Dir::South),
    ]
    .map(Step::Do);
    // On the crew deck: down the corridor to the door at (33, 7), open it,
    // walk through and back, close it behind you, and return to the ladder.
    let door_trip = std::iter::repeat_n(Move(Dir::South), 4)
        .chain([
            Move(Dir::South), // opens the door
            Move(Dir::South), // into the doorway
            Move(Dir::South), // into the room beyond
            Move(Dir::North),
            Move(Dir::North),
        ])
        .map(Step::Do)
        .chain([Step::Close(Dir::South)])
        .chain(std::iter::repeat_n(Step::Do(Move(Dir::North)), 4));
    let pattern = door_trip
        .chain(around)
        .chain([Step::Do(Climb(Vertical::Down))])
        .chain(bump_and_back)
        .chain(around)
        .chain([Climb(Vertical::Up), Climb(Vertical::Up)].map(Step::Do)); // the second can't
    let pattern: Vec<Step> = pattern.collect();
    to_the_ladder.chain(pattern.into_iter().cycle()).take(TURNS)
}

/// Plays the script and returns the final world and every event it produced.
fn play() -> (World, Vec<Event>) {
    let mut world = start_world();
    let events = script().flat_map(|s| apply(&mut world, s)).collect();
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
        "the scripted game changed: state hash {:#018x}, {ambient} ambient sounds, {} seconds",
        world.state_hash(),
        world.seconds(),
    );
}

#[test]
fn playing_twice_gives_identical_games() {
    let (a, a_events) = play();
    let (b, b_events) = play();
    assert_eq!(a, b);
    assert_eq!(a_events, b_events);
}

#[test]
fn saving_and_loading_halfway_changes_nothing() {
    // Play half the script, save, load the save into a brand-new world, and
    // play the other half. If anything about the game isn't saved, such as
    // the random generator's position, this ends in a different state.
    let mut world = start_world();
    let mut events: Vec<Event> = script()
        .take(TURNS / 2)
        .flat_map(|s| apply(&mut world, s))
        .collect();

    let mut resumed = load_from_str(&save_to_string(&world)).expect("save loads");
    events.extend(
        script()
            .skip(TURNS / 2)
            .flat_map(|s| apply(&mut resumed, s)),
    );

    let (uninterrupted, uninterrupted_events) = play();
    assert_eq!(resumed.state_hash(), EXPECTED_STATE_HASH);
    assert_eq!(resumed, uninterrupted);
    assert_eq!(events, uninterrupted_events);
}

#[test]
fn saving_after_every_command_changes_nothing() {
    // The harshest version: save and reload after every single command.
    let mut world = start_world();
    for step in script() {
        apply(&mut world, step);
        world = load_from_str(&save_to_string(&world)).expect("save loads");
    }
    assert_eq!(world.state_hash(), EXPECTED_STATE_HASH);
}

#[test]
fn the_script_climbs_up_and_comes_back_down() {
    // Guards the test itself: if a map edit moved the ladder, the script
    // would silently stop covering stairs. Check it really uses them.
    let (_, events) = play();
    let climbed = |way| {
        events
            .iter()
            .any(|e| matches!(e, Event::Climbed { way: w, .. } if *w == way))
    };
    assert!(climbed(Vertical::Up), "the script never climbed up");
    assert!(climbed(Vertical::Down), "the script never climbed down");
}

#[test]
fn the_script_opens_and_closes_a_door() {
    // Guards the test itself, like the stairs check above.
    let (_, events) = play();
    assert!(
        events.iter().any(|e| matches!(e, Event::DoorOpened { .. })),
        "never opened a door"
    );
    assert!(
        events.iter().any(|e| matches!(e, Event::DoorClosed { .. })),
        "never closed a door"
    );
}

#[test]
fn air_is_never_created_or_destroyed() {
    // However much air moves around during the scripted game (doors opening
    // and closing, air rising up the ladder, two damaged bunk rooms venting
    // to space), the air on board plus the air vented must always equal the
    // air the ship started with, down to the last pascal.
    let start = start_world().map().total_air();
    let mut world = start_world();
    for step in script() {
        apply(&mut world, step);
        let (on_board, vented) = (world.map().total_air(), world.map().vented());
        let accounted = [0, 1, 2].map(|g| on_board[g] + vented[g]);
        assert_eq!(accounted, start, "after {} seconds", world.seconds());
    }
    assert!(
        world.map().vented().iter().sum::<u64>() > 0,
        "the damaged rooms really vented"
    );
}

#[test]
fn heat_is_never_created_or_destroyed() {
    // Heat moves with the air, mixes between neighbours, leaks out through
    // the hull and leaves with vented air. The heat on board plus the heat
    // lost must always equal what the ship started with, to the last unit.
    let start = start_world().map().total_heat();
    let mut world = start_world();
    for step in script() {
        apply(&mut world, step);
        let accounted = world.map().total_heat() + world.map().heat_lost();
        assert_eq!(accounted, start, "after {} seconds", world.seconds());
    }
    assert!(world.map().heat_lost() > 0, "the hull and vents lost heat");
}

#[test]
fn the_cold_crew_deck_warms_from_the_stasis_bay_below() {
    // The crew deck starts at -20 °C. Warm air from the stasis bay rushes up
    // the ladder in the first second, and then keeps mixing up the shaft.
    let mut world = start_world();
    let top_of_ladder = nighthawk_sim::Pos::new(33, 2, 1);
    let bay = nighthawk_sim::Pos::new(16, 10, 0);
    let start = world.map().temperature(top_of_ladder).unwrap();
    assert_eq!(start, nighthawk_sim::celsius_to_mk(-20));
    world.apply(Command::Wait);
    let after_rush = world.map().temperature(top_of_ladder).unwrap();
    assert!(after_rush > start, "the rush of warm air");
    for _ in 0..60 {
        world.apply(Command::Wait);
    }
    assert!(world.map().temperature(top_of_ladder).unwrap() > after_rush);
    let bay_now = world.map().temperature(bay).unwrap();
    assert!(
        bay_now > nighthawk_sim::celsius_to_mk(13),
        "the far side of the bay is barely touched: {bay_now} mK"
    );
}

#[test]
fn air_rises_from_the_stasis_bay_to_the_thin_crew_deck() {
    // The crew deck starts at 60% pressure; the ladder shaft is open.
    let mut world = start_world();
    let top_of_ladder = nighthawk_sim::Pos::new(33, 2, 1);
    let before = world.map().air(top_of_ladder).unwrap().total();
    for _ in 0..20 {
        world.apply(Command::Wait);
    }
    assert!(world.map().air(top_of_ladder).unwrap().total() > before);
}
