//! The world state and the one function that changes it.

use crate::map::{Map, Tile};
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
    map: Map,
    player: Pos,
    turn: u64,
}

impl World {
    /// The M0.1 starting area: one room, the stasis bay on deck 0, with a
    /// couple of structural pillars so walls show up in the middle too.
    pub fn stasis_bay() -> Self {
        let (w, h) = (32, 14);
        let mut map = Map::filled(w, h, 1, Tile::Floor);

        // Outer bulkheads.
        for x in 0..w {
            map.set(Pos::new(x, 0, 0), Tile::Wall);
            map.set(Pos::new(x, h - 1, 0), Tile::Wall);
        }
        for y in 0..h {
            map.set(Pos::new(0, y, 0), Tile::Wall);
            map.set(Pos::new(w - 1, y, 0), Tile::Wall);
        }

        // Two 2×2 support pillars.
        for (px, py) in [(9, 5), (21, 7)] {
            for dx in 0..2 {
                for dy in 0..2 {
                    map.set(Pos::new(px + dx, py + dy, 0), Tile::Wall);
                }
            }
        }

        Self {
            map,
            player: Pos::new(4, 7, 0),
            turn: 0,
        }
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

    #[test]
    fn walking_onto_floor_moves_and_takes_a_turn() {
        let mut world = World::stasis_bay();
        let start = world.player();

        let events = world.apply(Command::Move(Dir::East));

        assert_eq!(world.player(), start.step(Dir::East));
        assert_eq!(world.turn(), 1);
        assert_eq!(
            events,
            vec![Event::Moved {
                to: start.step(Dir::East)
            }]
        );
    }

    #[test]
    fn bumping_a_wall_costs_no_time() {
        let mut world = World::stasis_bay();
        // Walk west until the next step is the outer bulkhead.
        while world.map().get(world.player().step(Dir::West)) == Some(Tile::Floor) {
            world.apply(Command::Move(Dir::West));
        }
        let (pos, turn) = (world.player(), world.turn());

        let events = world.apply(Command::Move(Dir::West));

        assert_eq!(world.player(), pos);
        assert_eq!(world.turn(), turn);
        assert!(matches!(
            events[..],
            [Event::Bumped {
                tile: Tile::Wall,
                ..
            }]
        ));
    }

    #[test]
    fn waiting_passes_time_without_moving() {
        let mut world = World::stasis_bay();
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
            Command::Move(Dir::NorthEast),
            Command::Wait,
            Command::Move(Dir::North),
            Command::Move(Dir::North),
        ];
        let mut a = World::stasis_bay();
        let mut b = World::stasis_bay();
        for c in script {
            a.apply(c);
            b.apply(c);
        }
        assert_eq!(a, b);
    }
}
