//! The world state and the one function that changes it.

use crate::map::{Map, Tile};
use crate::map_file::LoadedMap;
use crate::pos::{Dir, Pos};

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
}

/// How many turns each action costs. Real action costs (crafting takes
/// longer than a step) arrive with the energy scheduler.
const MOVE_COST: u64 = 1;
const WAIT_COST: u64 = 1;

/// Everything about the game at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    area_name: String,
    map: Map,
    player: Pos,
    turn: u64,
}

impl World {
    /// A new game on a loaded map, with the player at the map's `@` and the
    /// clock at turn 0.
    pub fn new(loaded: LoadedMap) -> Self {
        Self {
            area_name: loaded.name,
            map: loaded.map,
            player: loaded.player_start,
            turn: 0,
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

    /// Carries out one command and reports what happened.
    ///
    /// This is the *only* place the world changes, and the only place time
    /// moves forward (decision D7). An action that fails, like bumping a
    /// wall, costs no time.
    pub fn apply(&mut self, command: Command) -> Vec<Event> {
        match command {
            Command::Move(dir) => {
                let target = self.player.step(dir);
                match self.map.get(target) {
                    None => vec![Event::BlockedByEdge],
                    Some(tile) if !tile.is_walkable() => vec![Event::Bumped { at: target, tile }],
                    Some(_) => {
                        self.player = target;
                        self.turn += MOVE_COST;
                        vec![Event::Moved { to: target }]
                    }
                }
            }
            Command::Wait => {
                self.turn += WAIT_COST;
                vec![Event::Waited]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map_file::parse_map;

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
        World::new(parse_map(&json).expect("test map is valid"))
    }

    #[test]
    fn a_new_world_starts_at_turn_zero_on_the_start_tile() {
        let world = test_world();
        assert_eq!(world.turn(), 0);
        assert_eq!(world.player(), Pos::new(2, 1, 0));
        assert_eq!(world.area_name(), "Test room");
        assert_eq!(world.map().width(), 6);
    }

    #[test]
    fn walking_onto_floor_moves_and_takes_a_turn() {
        let mut world = test_world();
        let start = world.player();

        let events = world.apply(Command::Move(Dir::East));

        let to = start.step(Dir::East);
        assert_eq!(world.player(), to);
        assert_eq!(world.turn(), 1);
        assert_eq!(events, vec![Event::Moved { to }]);
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
}
