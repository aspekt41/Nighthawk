//! Blacking out and coming to beside your stasis chamber (decision D63),
//! on the real ship.

mod common;

use nighthawk_sim::{Cause, Command, Dir, Event, Pos, Vertical, WAKE_SECONDS, World};

/// From the start, take the gear out of the locker, then go up to the
/// crew deck and open the breached room's door at (11, 7), and wait there
/// until the air gives out.
fn black_out_upstairs(world: &mut World) {
    use Command::{Climb, Move};
    let take_out = world.content().action_index("take_out").unwrap();
    world.apply(Move(Dir::NorthWest));
    world.apply(Move(Dir::NorthWest));
    world.apply(Command::Interact {
        at: Pos::new(7, 4, 0),
        action: take_out,
    });
    // From (8, 5): up to the ladder at (33, 2), climb, and along to the door.
    let route = std::iter::repeat_n(Move(Dir::North), 3)
        .chain(std::iter::repeat_n(Move(Dir::East), 25))
        .chain([Climb(Vertical::Up)])
        .chain(std::iter::repeat_n(Move(Dir::South), 4))
        .chain(std::iter::repeat_n(Move(Dir::West), 22))
        .chain([Move(Dir::South)]); // opens the door
    for command in route {
        world.apply(command);
    }
    for _ in 0..2_000 {
        if !world.is_conscious() {
            return;
        }
        world.apply(Command::Wait);
    }
    panic!("never blacked out");
}

#[test]
fn you_come_to_beside_your_stasis_chamber_with_your_gear() {
    let mut world = common::start_world(7);
    black_out_upstairs(&mut world);
    assert_eq!(world.blackout_cause(), Some(Cause::Air));
    let carried = world.carried().len();
    assert_eq!(carried, 3, "the cutter, lamp and screwdriver");
    let before = world.seconds();
    let events = world.apply(Command::WakeUp);
    eprintln!(
        "PROBE events {:?}",
        events
            .iter()
            .filter(|e| !matches!(e, Event::Ambient(_)))
            .collect::<Vec<_>>()
    );
    eprintln!(
        "PROBE air at wake {:?} temp {:?} breath {} warmth {}",
        world.map().air(world.player()),
        world.map().temperature(world.player()),
        world.breath(),
        world.warmth()
    );
    let yours = world
        .content()
        .furniture_index("your_stasis_chamber")
        .unwrap();
    assert_eq!(events.first(), Some(&Event::WokeUp { beside: yours }));
    assert!(world.is_conscious());
    // Your chamber is at (11, 6)-(12, 6) in the stasis bay; the first free
    // tile around it is just north of it.
    assert_eq!(world.player(), Pos::new(11, 5, 0));
    assert_eq!(world.seconds() - before, WAKE_SECONDS);
    assert_eq!(world.carried().len(), carried, "everything still on you");
    // The open door upstairs has drained the bay through the ladder shaft,
    // but beside your chamber its emergency air keeps you going.
    let o2 = world.map().air(world.player()).unwrap().o2;
    assert!(o2 < nighthawk_sim::SAFE_O2_PA, "thin air: {o2} Pa of O2");
    for _ in 0..20 {
        world.apply(Command::Wait);
    }
    assert_eq!(world.breath(), nighthawk_sim::BREATH_FULL);
    // Two steps away, you're on your own.
    world.apply(Command::Move(Dir::North));
    world.apply(Command::Move(Dir::North));
    world.apply(Command::Wait);
    assert!(world.breath() < nighthawk_sim::BREATH_FULL);
}
