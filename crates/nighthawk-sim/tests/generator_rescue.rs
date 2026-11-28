//! The generator rescue (decisions D64 to D66), played on the real ship
//! from a fresh start: emergency power is off because the generator's main
//! fuse has blown, and the passage to it is venting through a hull breach.
//!
//! This file plays the slow, safe route: patch the breach from inside the
//! sealed airlock, let the bay's air into the passage, walk in and fit the
//! fuse.

mod common;

use nighthawk_sim::{Command, Dir, Event, Pos, SAFE_O2_PA, World};

const GEAR_LOCKER: Pos = Pos::new(7, 4, 0);
/// The emergency locker beside the airlock: the fuse, a hull patch kit
/// and a hammer.
const EMERGENCY_LOCKER: Pos = Pos::new(25, 12, 0);
const INNER_DOOR: Pos = Pos::new(26, 11, 0);
/// The airlock's second tile, beside its outer door.
const LOCK: Pos = Pos::new(28, 11, 0);
const OUTER_DOOR: Pos = Pos::new(29, 11, 0);
/// The breach, at the corner beside the outer door: diagonally beside the
/// lock, so it can be patched from inside.
const BREACH: Pos = Pos::new(29, 12, 0);
/// Just past the outer door, in the passage.
const PASSAGE: Pos = Pos::new(30, 11, 0);
const GENERATOR: Pos = Pos::new(30, 9, 0);
/// Where you stand to work on the generator.
const AT_THE_GENERATOR: Pos = Pos::new(31, 9, 0);
/// The light on the generator's circuit, over the corridor beside it.
const CIRCUIT_LIGHT: Pos = Pos::new(33, 10, 0);
const READOUT: Pos = Pos::new(15, 5, 0);
/// The open floor beside your stasis chamber.
const BY_THE_POD: Pos = Pos::new(13, 6, 0);

fn action(world: &World, id: &str) -> u16 {
    world.content().action_index(id).unwrap()
}

fn walk(world: &mut World, dirs: &[Dir]) {
    for &dir in dirs {
        world.apply(Command::Move(dir));
    }
}

fn take_out(world: &mut World, at: Pos) {
    world.apply(Command::Interact {
        at,
        action: action(world, "take_out"),
    });
}

fn carrying(world: &World, id: &str) -> u32 {
    world.carried_count(world.item_type_index(id).unwrap())
}

/// From the start, takes the gear (with the screwdriver) out of the gear
/// locker and the supplies out of the emergency locker, ending beside the
/// airlock's inner door at (25, 11).
fn fetch_the_supplies(world: &mut World) {
    use Dir::{East, NorthWest, South, SouthEast};
    walk(world, &[NorthWest, NorthWest]);
    take_out(world, GEAR_LOCKER);
    walk(world, &[SouthEast; 5]);
    walk(world, &[East; 12]);
    walk(world, &[South]);
    assert_eq!(world.player(), Pos::new(25, 11, 0));
    take_out(world, EMERGENCY_LOCKER);
    for id in ["screwdriver", "fuse", "hull_patch_kit", "makeshift_hammer"] {
        assert_eq!(carrying(world, id), 1, "{id}");
    }
}

/// The air pressure at `p`, in pascals.
fn pressure(world: &World, p: Pos) -> u64 {
    world.map().air(p).unwrap().total()
}

#[test]
fn at_the_start_the_fault_is_readable_and_the_passage_is_venting() {
    let world = common::start_world(1);
    let readout = world.structure_description(READOUT).unwrap();
    assert!(readout.contains("MAIN FUSE BLOWN"), "{readout}");
    assert!(readout.contains("screwdriver"), "names the tool it takes");
    assert!(world.is_visible(READOUT), "in view of where you wake");
    assert!(world.fed_circuits().is_empty());
    assert!(world.is_unpowered(CIRCUIT_LIGHT));
    let generator = world.structure_description(GENERATOR).unwrap();
    assert!(generator.contains("fuse has blown"), "{generator}");
    assert!(
        generator.contains("Repair it with a new fuse"),
        "{generator}"
    );
    let survey = world.map().survey();
    assert!(survey.is_venting(PASSAGE));
    assert!(!survey.is_venting(LOCK), "the lock is sealed");
    assert!(!survey.is_venting(BY_THE_POD), "and so is the bay");
}

#[test]
fn the_breach_is_patched_from_the_sealed_lock_in_breathable_air() {
    let mut world = common::start_world(1);
    fetch_the_supplies(&mut world);
    walk(&mut world, &[Dir::East, Dir::East, Dir::East, Dir::East]);
    assert_eq!(world.player(), LOCK, "the first step opened the door");
    let before = world.seconds();
    world.apply(Command::Interact {
        at: BREACH,
        action: action(&world, "repair"),
    });
    assert_eq!(world.seconds() - before, 300, "a five-minute patch");
    assert_eq!(world.structure_at(BREACH).unwrap().id, "hull_plating");
    let o2 = world.map().air(LOCK).unwrap().o2;
    assert!(o2 >= SAFE_O2_PA, "breathable all along: {o2} Pa");
    assert!(!world.map().survey().is_venting(PASSAGE), "sealed");
    assert_eq!(pressure(&world, PASSAGE), 0, "but still vacuum");
}

#[test]
fn once_sealed_the_bay_air_fills_the_passage_and_the_pod_area_stays_safe() {
    let mut world = common::start_world(1);
    fetch_the_supplies(&mut world);
    walk(&mut world, &[Dir::East, Dir::East, Dir::East, Dir::East]);
    world.apply(Command::Interact {
        at: BREACH,
        action: action(&world, "repair"),
    });
    let bay_before = pressure(&world, BY_THE_POD);
    // Both doors open: the inner one is still open behind you.
    assert_eq!(
        world.structure_at(INNER_DOOR).unwrap().id,
        "airlock_door_open"
    );
    let events = world.apply(Command::Move(Dir::East));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Transformed { at, .. } if *at == OUTER_DOOR)),
        "{events:?}"
    );
    // Air moves through a door once it has finished opening (D46), so
    // the rush comes in the next second.
    world.apply(Command::Wait);
    let bay_after = pressure(&world, BY_THE_POD);
    assert!(pressure(&world, PASSAGE) > 0, "air rushed in");
    // One space now: even, give or take the pascal per gas left over when
    // it's shared out (D30).
    assert!(pressure(&world, PASSAGE).abs_diff(bay_after) <= 3);
    assert!(
        bay_after * 100 >= bay_before * 95,
        "the bay lost under 5%: {bay_before} to {bay_after} Pa"
    );
    let o2 = world.map().air(BY_THE_POD).unwrap().o2;
    assert!(o2 >= SAFE_O2_PA, "the pod area stays breathable: {o2} Pa");
    assert!(world.map().air(PASSAGE).unwrap().o2 >= SAFE_O2_PA);
}

/// From the lock, through the open outer door and along the passage to
/// the generator: along the bottom, up the far end, back along the top.
fn walk_to_the_generator(world: &mut World) {
    use Dir::{East, North, NorthEast, NorthWest, SouthEast, West};
    walk(world, &[East, SouthEast]);
    walk(world, &[East; 5]);
    walk(world, &[NorthEast, North, NorthWest]);
    walk(world, &[West; 4]);
    assert_eq!(world.player(), AT_THE_GENERATOR);
}

#[test]
fn fitting_the_fuse_restarts_the_generator_and_its_light_comes_on_at_once() {
    let mut world = safe_route_to_the_generator(1);
    assert!(!world.is_lit(Pos::new(34, 9, 0)), "dark before");
    let before = world.seconds();
    let events = repair_the_generator(&mut world);
    assert_eq!(world.seconds() - before, 40, "the repair's time");
    assert!(
        events.contains(&Event::Powered {
            circuit: 0,
            lights: 1
        }),
        "{events:?}"
    );
    assert_eq!(
        world.structure_at(GENERATOR).unwrap().id,
        "emergency_generator"
    );
    assert_eq!(carrying(&world, "fuse"), 0, "the fuse was used");
    assert!(!world.is_unpowered(CIRCUIT_LIGHT));
    assert!(world.is_lit(Pos::new(34, 9, 0)), "the corridor lights up");
    let readout = world.structure_description(READOUT).unwrap();
    assert!(readout.contains("LIVE"), "{readout}");
}

/// From a fresh start, patches the breach, opens the passage and walks to
/// the generator, carrying the supplies.
fn safe_route_to_the_generator(seed: u64) -> World {
    let mut world = common::start_world(seed);
    fetch_the_supplies(&mut world);
    walk(&mut world, &[Dir::East, Dir::East, Dir::East, Dir::East]);
    world.apply(Command::Interact {
        at: BREACH,
        action: action(&world, "repair"),
    });
    world.apply(Command::Move(Dir::East)); // opens the outer door
    walk_to_the_generator(&mut world);
    world
}

fn repair_the_generator(world: &mut World) -> Vec<Event> {
    world.apply(Command::Interact {
        at: GENERATOR,
        action: action(world, "repair"),
    })
}

#[test]
fn without_a_fuse_the_repair_is_refused_and_takes_no_time() {
    let mut world = safe_route_to_the_generator(1);
    let fuse = world.item_type_index("fuse").unwrap();
    let (path, _) = world
        .carried()
        .into_iter()
        .find(|(_, item)| item.kind == fuse)
        .unwrap();
    world.apply(Command::Drop(path));
    let before = world.seconds();
    let events = repair_the_generator(&mut world);
    let refused = Event::Unable {
        action: action(&world, "repair"),
        blocked: nighthawk_sim::Blocked::Missing {
            item: fuse,
            need: 1,
            have: 0,
            tag: None,
        },
    };
    assert_eq!(events, [refused]);
    assert_eq!(world.seconds(), before);
    assert_eq!(
        world.structure_at(GENERATOR).unwrap().id,
        "emergency_generator_blown"
    );
}

#[test]
fn saving_and_loading_before_or_after_the_repair_changes_nothing() {
    use nighthawk_sim::{load_from_str, save_to_string};
    let reload = |world: &World| load_from_str(&save_to_string(world)).expect("save loads");
    let mut straight = safe_route_to_the_generator(1);
    let mut saved_before = reload(&straight);
    repair_the_generator(&mut straight);
    repair_the_generator(&mut saved_before);
    assert_eq!(saved_before.state_hash(), straight.state_hash());
    let saved_after = reload(&straight);
    assert_eq!(saved_after.state_hash(), straight.state_hash());
    assert_eq!(saved_after.fed_circuits(), [0], "still running");
    assert!(
        saved_after.is_lit(Pos::new(34, 9, 0)),
        "and the light is on"
    );
}

#[test]
fn the_same_seed_and_commands_give_the_same_rescue() {
    let mut a = safe_route_to_the_generator(7);
    let mut b = safe_route_to_the_generator(7);
    assert_eq!(repair_the_generator(&mut a), repair_the_generator(&mut b));
    assert_eq!(a, b);
}
