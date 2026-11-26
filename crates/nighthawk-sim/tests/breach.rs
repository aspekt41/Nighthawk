//! M1's exit test: a hull breach visibly vents a room (decision D33).
//!
//! Plays the real starting map. On the crew deck, the south-west bunk room
//! has a breach in its outer wall and starts behind a closed door, so it
//! vents to vacuum on its own. Opening that door joins it to the corridor,
//! the lounge and, through the open ladder shaft, the stasis bay below, and
//! all of that air starts escaping. Closing the door stops it.

use std::fs;
use std::path::PathBuf;

use nighthawk_sim::{Command, Dir, Event, Pos, Tile, Vertical, World, parse_map};

const DOOR: Pos = Pos::new(5, 7, 1); // the breached room's door
const IN_THE_ROOM: Pos = Pos::new(5, 10, 1);
const OUTSIDE_THE_DOOR: Pos = Pos::new(5, 6, 1);

fn start_world() -> World {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/maps/aft_ring.json");
    World::new(parse_map(&fs::read_to_string(path).unwrap()).unwrap(), 1)
}

fn pressure(world: &World, at: Pos) -> u64 {
    world.map().air(at).unwrap().total()
}

/// From the start: to the ladder, up it, down the corridor and along it to
/// just outside the breached room's door. Ends standing at OUTSIDE_THE_DOOR.
fn walk_to_the_door(world: &mut World) {
    let route = std::iter::repeat_n(Command::Move(Dir::North), 5)
        .chain(std::iter::repeat_n(Command::Move(Dir::East), 23))
        .chain([Command::Climb(Vertical::Up)])
        .chain(std::iter::repeat_n(Command::Move(Dir::South), 4))
        .chain(std::iter::repeat_n(Command::Move(Dir::West), 22));
    for command in route {
        world.apply(command);
    }
    assert_eq!(world.player(), OUTSIDE_THE_DOOR);
    assert_eq!(world.map().get(DOOR), Some(Tile::DoorClosed));
}

/// Waits `n` times (5 s each) and returns every event.
fn wait(world: &mut World, n: usize) -> Vec<Event> {
    (0..n).flat_map(|_| world.apply(Command::Wait)).collect()
}

#[test]
fn the_breached_room_is_vacuum_by_the_time_you_get_there() {
    let mut world = start_world();
    walk_to_the_door(&mut world);
    assert!(
        pressure(&world, IN_THE_ROOM) < 1_000,
        "{} Pa",
        pressure(&world, IN_THE_ROOM)
    );
    // The closed door kept everything else safe: the corridor is fine.
    assert!(pressure(&world, OUTSIDE_THE_DOOR) > 80_000);
}

#[test]
fn opening_the_door_vents_the_corridor_and_says_so() {
    let mut world = start_world();
    walk_to_the_door(&mut world);
    let before = pressure(&world, OUTSIDE_THE_DOOR);

    let events = world.apply(Command::Move(Dir::South)); // opens the door
    let events: Vec<Event> = events.into_iter().chain(wait(&mut world, 6)).collect();

    assert!(events.contains(&Event::AirEscaping), "{events:?}");
    let after = pressure(&world, OUTSIDE_THE_DOOR);
    assert!(after < before, "{before} Pa -> {after} Pa");
}

#[test]
fn closing_the_door_again_stops_the_loss() {
    let mut world = start_world();
    walk_to_the_door(&mut world);
    world.apply(Command::Move(Dir::South)); // open
    wait(&mut world, 4);
    world.apply(Command::Close(Dir::South)); // shut it

    let sealed = pressure(&world, OUTSIDE_THE_DOOR);
    let events = wait(&mut world, 12);
    assert_eq!(pressure(&world, OUTSIDE_THE_DOOR), sealed, "no more loss");
    assert!(!events.contains(&Event::AirEscaping));
}

#[test]
fn leaving_it_open_eventually_blacks_you_out() {
    let mut world = start_world();
    walk_to_the_door(&mut world);
    world.apply(Command::Move(Dir::South)); // open, and stay
    let mut events = Vec::new();
    for _ in 0..1_000 {
        if !world.is_conscious() {
            break;
        }
        events.extend(world.apply(Command::Wait));
    }
    assert!(
        !world.is_conscious(),
        "still awake after {} s",
        world.seconds()
    );
    assert!(events.contains(&Event::BlackedOut));
}
