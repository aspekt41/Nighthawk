//! The world state and the one function that changes it.

use crate::hash::Fnv64;
use crate::map::{Map, Tile};
use crate::map_file::LoadedMap;
use crate::pos::{Dir, Pos};
use crate::rng::{GameRng, Stream};

/// Something the player wants to do. Front-ends turn key presses into these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Move(Dir),
    Wait,
}

/// Something that happened, for the front-end to show or log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Moved {
        to: Pos,
    },
    /// The player tried to walk into something solid. No time passes.
    Bumped {
        at: Pos,
        tile: Tile,
    },
    /// The player tried to walk off the edge of the map. No time passes.
    BlockedByEdge,
    Waited,
    /// A background noise from the ship, heard while time passes.
    Ambient(Sound),
}

/// Noises the dead ship makes. Pure atmosphere for now. Later, sounds will
/// come from real things (a pump, a breach, something in the vents).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    VentTick,
    HullCreak,
    DistantClang,
    CoolantDrip,
}

impl Sound {
    pub const ALL: [Sound; 4] = [
        Sound::VentTick,
        Sound::HullCreak,
        Sound::DistantClang,
        Sound::CoolantDrip,
    ];
}

/// How many turns each action costs. Real action costs (crafting takes
/// longer than a step) arrive with the energy scheduler.
const MOVE_COST: u64 = 1;
const WAIT_COST: u64 = 1;

/// Each turn that passes has a 1 in this many chance of an ambient sound.
const AMBIENT_ONE_IN: u32 = 8;

/// Everything about the game at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    area_name: String,
    map: Map,
    player: Pos,
    turn: u64,
    /// The seed this game started from. Same seed + same commands = same game.
    seed: u64,
    ambient_rng: GameRng,
}

impl World {
    /// A new game on a loaded map, with the player at the map's `@`, the
    /// clock at turn 0, and all randomness derived from `seed`.
    pub fn new(loaded: LoadedMap, seed: u64) -> Self {
        Self {
            area_name: loaded.name,
            map: loaded.map,
            player: loaded.player_start,
            turn: 0,
            seed,
            ambient_rng: GameRng::new(seed, Stream::Ambient),
        }
    }

    /// The human-readable name of where the player is, like "Stasis bay".
    pub fn area_name(&self) -> &str {
        &self.area_name
    }

    pub fn map(&self) -> &Map {
        &self.map
    }

    pub fn player(&self) -> Pos {
        self.player
    }

    /// How many turns have passed since the game began.
    pub fn turn(&self) -> u64 {
        self.turn
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Carries out one command and reports what happened.
    ///
    /// This is the *only* place the world changes, and the only place time
    /// moves forward (decision D7). An action that fails, like bumping a
    /// wall, costs no time.
    pub fn apply(&mut self, command: Command) -> Vec<Event> {
        match command {
            Command::Move(dir) => self.walk(dir),
            Command::Wait => {
                let mut events = vec![Event::Waited];
                self.pass_time(WAIT_COST, &mut events);
                events
            }
        }
    }

    fn walk(&mut self, dir: Dir) -> Vec<Event> {
        let target = self.player.step(dir);
        match self.map.get(target) {
            None => vec![Event::BlockedByEdge],
            Some(tile) if !tile.is_walkable() => vec![Event::Bumped { at: target, tile }],
            Some(_) => {
                self.player = target;
                let mut events = vec![Event::Moved { to: target }];
                self.pass_time(MOVE_COST, &mut events);
                events
            }
        }
    }

    /// Advances the clock one turn at a time, letting the world react to
    /// each turn. Every system that runs "while time passes" (air, heat,
    /// power, creatures) will hook in here, so it only ever runs because the
    /// player did something.
    fn pass_time(&mut self, turns: u64, events: &mut Vec<Event>) {
        for _ in 0..turns {
            self.turn += 1;
            if self.ambient_rng.one_in(AMBIENT_ONE_IN)
                && let Some(&sound) = self.ambient_rng.pick(&Sound::ALL)
            {
                events.push(Event::Ambient(sound));
            }
        }
    }

    /// A fingerprint of the entire game state.
    ///
    /// Two worlds with the same hash are, for all practical purposes, the same
    /// game. The determinism test plays a scripted game and compares this
    /// against a stored value, so it changes exactly when game behaviour
    /// changes. Every new piece of state must be added here.
    pub fn state_hash(&self) -> u64 {
        let mut h = Fnv64::new();
        // A version tag: bump it if the hashing scheme itself changes.
        h.str("nighthawk-world-v1").str(&self.area_name);
        self.map.hash_into(&mut h);
        h.i32(self.player.x).i32(self.player.y).i32(self.player.z);
        h.u64(self.turn)
            .u64(self.seed)
            .u64(self.ambient_rng.fingerprint());
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map_file::parse_map;

    const TEST_SEED: u64 = 1234;

    /// The events with ambient sounds removed, for tests about movement.
    fn without_ambient(events: Vec<Event>) -> Vec<Event> {
        events
            .into_iter()
            .filter(|e| !matches!(e, Event::Ambient(_)))
            .collect()
    }

    fn count_ambient(events: &[Event]) -> usize {
        events
            .iter()
            .filter(|e| matches!(e, Event::Ambient(_)))
            .count()
    }

    /// A small room: walls around the edge, one pillar, the player inside.
    ///
    /// ```text
    /// ######
    /// #.@..#
    /// #..#.#
    /// ######
    /// ```
    fn test_world() -> World {
        world_from_rows(&["######", "#.@..#", "#..#.#", "######"])
    }

    fn world_from_rows(rows: &[&str]) -> World {
        let rows: Vec<String> = rows.iter().map(|r| format!("{r:?}")).collect();
        let json = format!(
            r##"{{ "id": "t", "name": "Test room", "legend": {{ "#": "wall", ".": "floor" }},
                  "decks": [ {{ "rows": [{}] }} ] }}"##,
            rows.join(", ")
        );
        World::new(parse_map(&json).expect("test map is valid"), TEST_SEED)
    }

    #[test]
    fn a_new_world_starts_at_turn_zero_on_the_start_tile() {
        let world = test_world();
        assert_eq!(world.turn(), 0);
        assert_eq!(world.player(), Pos::new(2, 1, 0));
        assert_eq!(world.area_name(), "Test room");
        assert_eq!(world.map().width(), 6);
        assert_eq!(world.seed(), TEST_SEED);
    }

    #[test]
    fn walking_onto_floor_moves_and_takes_a_turn() {
        let mut world = test_world();
        let start = world.player();

        let events = world.apply(Command::Move(Dir::East));

        let to = start.step(Dir::East);
        assert_eq!(world.player(), to);
        assert_eq!(world.turn(), 1);
        assert_eq!(without_ambient(events), vec![Event::Moved { to }]);
    }

    #[test]
    fn bumping_a_wall_costs_no_time() {
        let mut world = test_world();

        let events = world.apply(Command::Move(Dir::North));

        assert_eq!(world.player(), Pos::new(2, 1, 0));
        assert_eq!(world.turn(), 0);
        assert_eq!(
            events,
            vec![Event::Bumped {
                at: Pos::new(2, 0, 0),
                tile: Tile::Wall
            }]
        );
    }

    #[test]
    fn diagonal_moves_are_blocked_by_walls_too() {
        let mut world = test_world();
        // South-east of the start is the pillar at (3, 2).
        let events = world.apply(Command::Move(Dir::SouthEast));
        assert!(matches!(
            events[..],
            [Event::Bumped {
                tile: Tile::Wall,
                ..
            }]
        ));
        assert_eq!(world.turn(), 0);
    }

    #[test]
    fn walking_off_the_map_edge_is_blocked_and_costs_no_time() {
        // No walls: the start tile is on the map's top edge.
        let mut world = world_from_rows(&[".@.", "..."]);

        let events = world.apply(Command::Move(Dir::North));

        assert_eq!(events, vec![Event::BlockedByEdge]);
        assert_eq!(world.player(), Pos::new(1, 0, 0));
        assert_eq!(world.turn(), 0);
    }

    #[test]
    fn waiting_passes_time_without_moving() {
        let mut world = test_world();
        let start = world.player();
        world.apply(Command::Wait);
        world.apply(Command::Wait);
        assert_eq!(world.player(), start);
        assert_eq!(world.turn(), 2);
    }

    #[test]
    fn same_commands_give_the_same_world() {
        // The first determinism check. It becomes a seeded, hashed test in M0.3.
        let script = [
            Command::Move(Dir::East),
            Command::Move(Dir::SouthEast),
            Command::Wait,
            Command::Move(Dir::North),
            Command::Move(Dir::West),
        ];
        let mut a = test_world();
        let mut b = test_world();
        for c in script {
            a.apply(c);
            b.apply(c);
        }
        assert_eq!(a, b);
    }

    #[test]
    fn failed_actions_make_no_sound_and_change_nothing() {
        let mut world = test_world();
        let before = world.state_hash();
        for _ in 0..200 {
            let events = world.apply(Command::Move(Dir::North)); // into the wall
            assert_eq!(count_ambient(&events), 0);
        }
        assert_eq!(world.state_hash(), before, "bumping must not touch the RNG");
    }

    #[test]
    fn ambient_sounds_happen_about_one_turn_in_eight() {
        let mut world = test_world();
        let heard: usize = (0..800)
            .map(|_| count_ambient(&world.apply(Command::Wait)))
            .sum();
        // Expect about 100. Wide bounds: the test is about the rate, not luck.
        assert!(
            (60..=140).contains(&heard),
            "heard {heard} sounds in 800 turns"
        );
    }

    #[test]
    fn every_sound_can_happen() {
        let mut world = test_world();
        let heard: Vec<Sound> = (0..2000)
            .flat_map(|_| world.apply(Command::Wait))
            .filter_map(|e| match e {
                Event::Ambient(sound) => Some(sound),
                _ => None,
            })
            .collect();
        for sound in Sound::ALL {
            assert!(heard.contains(&sound), "{sound:?} never happened");
        }
    }

    #[test]
    fn same_seed_gives_the_same_sounds_and_different_seeds_differ() {
        let script = |seed| {
            let mut world = test_world();
            world = World {
                ambient_rng: GameRng::new(seed, Stream::Ambient),
                seed,
                ..world
            };
            (0..100)
                .flat_map(|_| world.apply(Command::Wait))
                .collect::<Vec<_>>()
        };
        assert_eq!(script(5), script(5));
        assert_ne!(script(5), script(6));
    }

    #[test]
    fn state_hash_is_equal_for_equal_worlds() {
        assert_eq!(test_world().state_hash(), test_world().state_hash());
    }

    #[test]
    fn state_hash_changes_when_anything_changes() {
        let start = test_world().state_hash();

        let mut moved = test_world();
        moved.apply(Command::Move(Dir::East));
        assert_ne!(moved.state_hash(), start, "position and turn");

        let mut waited = test_world();
        waited.apply(Command::Wait);
        assert_ne!(waited.state_hash(), start, "turn and RNG");

        let other_seed = World::new(
            parse_map(
                r#"{ "id": "t", "name": "Test room", "legend": { ".": "floor" },
                     "decks": [ { "rows": ["@"] } ] }"#,
            )
            .unwrap(),
            TEST_SEED,
        );
        assert_ne!(other_seed.state_hash(), start, "map contents");
    }

    #[test]
    fn state_hash_includes_the_seed() {
        let a = test_world();
        let b = World {
            seed: TEST_SEED + 1,
            ..test_world()
        };
        assert_ne!(a.state_hash(), b.state_hash());
    }
}
