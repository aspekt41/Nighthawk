//! The generator rescue's quick, risky route (decisions D64 and D67),
//! played on the real ship from a fresh start: put on the emergency
//! rescue suit, cycle the airlock, cross the still-venting passage, fit
//! the fuse and come back, leaving the breach for later.

mod common;

use nighthawk_sim::{
    Blocked, Command, Dir, Dress, Event, ItemAt, Pos, SuitAirLevel, World, load_from_str,
    save_to_string,
};

const GEAR_LOCKER: Pos = Pos::new(7, 4, 0);
const EMERGENCY_LOCKER: Pos = Pos::new(25, 12, 0);
const INNER_DOOR: Pos = Pos::new(26, 11, 0);
const LOCK: Pos = Pos::new(28, 11, 0);
const OUTER_DOOR: Pos = Pos::new(29, 11, 0);
const BREACH: Pos = Pos::new(29, 12, 0);
const GENERATOR: Pos = Pos::new(30, 9, 0);
const AT_THE_GENERATOR: Pos = Pos::new(31, 9, 0);

fn action(world: &World, id: &str) -> u16 {
    world.content().action_index(id).unwrap()
}

fn walk(world: &mut World, dirs: &[Dir]) {
    for &dir in dirs {
        world.apply(Command::Move(dir));
    }
}

fn interact(world: &mut World, at: Pos, id: &str) -> Vec<Event> {
    world.apply(Command::Interact {
        at,
        action: action(world, id),
    })
}

/// From the start: the screwdriver and the rest from the gear locker, the
/// fuse and the rest from the emergency locker, and the rescue suit on,
/// ending beside the airlock's inner door at (25, 11).
fn suit_up(world: &mut World) {
    use Dir::{East, NorthWest, South, SouthEast};
    walk(world, &[NorthWest, NorthWest]);
    interact(world, GEAR_LOCKER, "take_out");
    walk(world, &[SouthEast; 5]);
    walk(world, &[East; 12]);
    walk(world, &[South]);
    interact(world, EMERGENCY_LOCKER, "take_out");
    let suit = world.item_type_index("rescue_suit").unwrap();
    let at = world
        .wearables_at_hand()
        .into_iter()
        .find(|(_, item)| item.kind == suit)
        .map(|(at, _)| at)
        .expect("the suit is in the locker");
    assert!(matches!(at, ItemAt::Inside { .. }));
    world.apply(Command::Dress(Dress::PutOn(at)));
    assert_eq!(world.suit_o2(), Some((8000, 8000)), "a full canister");
    assert_eq!(world.player(), Pos::new(25, 11, 0));
}

/// Into the airlock, shutting the inner door behind you, then opening the
/// outer one: the lock's air goes into the passage and out of the breach.
fn cycle_out(world: &mut World) {
    walk(world, &[Dir::East; 3]); // the first opens the inner door
    interact(world, INNER_DOOR, "close");
    walk(world, &[Dir::East]);
    assert_eq!(world.player(), LOCK);
    let events = world.apply(Command::Move(Dir::East));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Transformed { at, .. } if *at == OUTER_DOOR)),
        "the outer door opens from the sealed lock: {events:?}"
    );
}

fn walk_to_the_generator(world: &mut World) {
    use Dir::{East, North, NorthEast, NorthWest, SouthEast, West};
    walk(world, &[East, SouthEast]);
    walk(world, &[East; 5]);
    walk(world, &[NorthEast, North, NorthWest]);
    walk(world, &[West; 4]);
    assert_eq!(world.player(), AT_THE_GENERATOR);
}

fn walk_back_to_the_lock(world: &mut World) {
    use Dir::{East, NorthWest, South, SouthEast, SouthWest, West};
    walk(world, &[East; 4]);
    walk(world, &[SouthEast, South, SouthWest]);
    walk(world, &[West; 5]);
    walk(world, &[NorthWest, West]);
    assert_eq!(world.player(), LOCK);
}

/// Back in the lock: shut the outer door, step to the inner one and open
/// it, so the bay's air refills the lock.
fn cycle_in(world: &mut World) {
    interact(world, OUTER_DOOR, "close");
    walk(world, &[Dir::West, Dir::West]); // the second opens the inner door
    world.apply(Command::Wait);
}

/// The oxygen breathable where the player stands, in pascals.
fn o2_here(world: &World) -> u32 {
    world.map().air(world.player()).unwrap().o2
}

#[test]
fn in_the_suit_you_cross_the_venting_passage_fit_the_fuse_and_come_back() {
    let mut world = common::start_world(1);
    suit_up(&mut world);
    cycle_out(&mut world);
    walk_to_the_generator(&mut world);
    assert!(
        world.map().survey().is_venting(AT_THE_GENERATOR),
        "still venting"
    );
    let events = interact(&mut world, GENERATOR, "repair");
    assert!(
        events.iter().any(|e| matches!(e, Event::Powered { .. })),
        "{events:?}"
    );
    walk_back_to_the_lock(&mut world);
    cycle_in(&mut world);
    assert!(
        o2_here(&world) >= nighthawk_sim::SAFE_O2_PA,
        "breathable again"
    );
    assert_eq!(
        world.breath(),
        nighthawk_sim::BREATH_FULL,
        "never short of breath"
    );
    let (left, full) = world.suit_o2().unwrap();
    assert!(
        left * 100 >= full * 30,
        "a third of the canister to spare, for a mistake: {left} of {full} ml"
    );
    assert_eq!(
        world.structure_at(BREACH).unwrap().id,
        "hull_breach",
        "the breach is still there to come back to"
    );
    assert_eq!(world.fed_circuits(), [0]);
}

#[test]
fn the_suit_seals_as_the_lock_vents_and_warns_on_the_way_back() {
    let mut world = common::start_world(1);
    suit_up(&mut world);
    let mut events = Vec::new();
    walk(&mut world, &[Dir::East; 3]);
    interact(&mut world, INNER_DOOR, "close");
    walk(&mut world, &[Dir::East]);
    events.extend(world.apply(Command::Move(Dir::East)));
    events.extend(world.apply(Command::Move(Dir::East)));
    assert!(events.contains(&Event::SuitSealed), "{events:?}");
    // Dawdle in the passage until the canister is half gone.
    let mut warned = None;
    for _ in 0..30 {
        let more = world.apply(Command::Wait);
        warned = warned.or(more.iter().find_map(|e| match e {
            Event::SuitAir {
                level: SuitAirLevel::Half,
                seconds_left,
            } => Some(*seconds_left),
            _ => None,
        }));
    }
    let left = warned.expect("a warning at half");
    assert!(left > 90, "time to turn back: {left} s left");
}

#[test]
fn turning_back_halfway_you_reach_air_with_the_generator_still_dead() {
    let mut world = common::start_world(1);
    suit_up(&mut world);
    cycle_out(&mut world);
    walk(
        &mut world,
        &[Dir::East, Dir::SouthEast, Dir::East, Dir::East],
    );
    walk(
        &mut world,
        &[Dir::West, Dir::West, Dir::NorthWest, Dir::West],
    );
    assert_eq!(world.player(), LOCK);
    cycle_in(&mut world);
    assert!(o2_here(&world) >= nighthawk_sim::SAFE_O2_PA);
    assert!(world.is_conscious());
    assert!(world.fed_circuits().is_empty(), "nothing fixed");
    let (left, full) = world.suit_o2().unwrap();
    assert!(left > full / 2, "most of the canister left for another try");
}

#[test]
fn with_an_empty_canister_the_run_ends_in_a_blackout_short_of_the_fix() {
    let mut world = common::start_world(1);
    suit_up(&mut world);
    // Take the canister out and leave it behind.
    world.apply(Command::Power {
        item: nighthawk_sim::Gear::Worn(2),
        act: nighthawk_sim::PowerAct::Unload,
    });
    assert_eq!(world.suit_o2(), None);
    cycle_out(&mut world);
    // The same route as a full canister takes; a world that has blacked
    // out ignores the rest of it.
    use Dir::{East, North, NorthEast, NorthWest, SouthEast, West};
    walk(&mut world, &[East, SouthEast, East, East, East, East, East]);
    walk(
        &mut world,
        &[NorthEast, North, NorthWest, West, West, West, West],
    );
    interact(&mut world, GENERATOR, "repair");
    assert!(!world.is_conscious(), "out cold in the vacuum");
    assert!(
        world.map().survey().is_venting(world.player()),
        "in the passage, short of the generator: {:?}",
        world.player()
    );
    assert!(world.fed_circuits().is_empty(), "nothing fixed");
}

#[test]
fn the_outer_door_stays_shut_while_the_inner_one_is_open() {
    let mut world = common::start_world(1);
    suit_up(&mut world);
    walk(&mut world, &[Dir::East; 4]);
    assert_eq!(world.player(), LOCK);
    let events = world.apply(Command::Move(Dir::East));
    assert!(
        matches!(
            events[..],
            [Event::Unable {
                blocked: Blocked::Interlocked,
                ..
            }]
        ),
        "{events:?}"
    );
    assert!(!world.map().survey().is_venting(LOCK), "the bay is safe");
}

#[test]
fn saving_and_loading_mid_run_changes_nothing() {
    let mut straight = common::start_world(1);
    suit_up(&mut straight);
    cycle_out(&mut straight);
    walk(&mut straight, &[Dir::East, Dir::SouthEast]);
    let mut resumed = load_from_str(&save_to_string(&straight)).expect("save loads");
    for world in [&mut straight, &mut resumed] {
        walk(world, &[Dir::East; 5]);
        walk(world, &[Dir::NorthEast, Dir::North, Dir::NorthWest]);
        walk(world, &[Dir::West; 4]);
        interact(world, GENERATOR, "repair");
    }
    assert_eq!(resumed, straight);
    assert_eq!(resumed.suit_o2(), straight.suit_o2());
}

#[test]
fn the_same_seed_and_commands_give_the_same_suit_run() {
    let run = || {
        let mut world = common::start_world(7);
        suit_up(&mut world);
        cycle_out(&mut world);
        walk_to_the_generator(&mut world);
        let events = interact(&mut world, GENERATOR, "repair");
        walk_back_to_the_lock(&mut world);
        cycle_in(&mut world);
        (world, events)
    };
    let (a, a_events) = run();
    let (b, b_events) = run();
    assert_eq!(a_events, b_events);
    assert_eq!(a, b);
    assert_eq!(a.state_hash(), b.state_hash());
}
