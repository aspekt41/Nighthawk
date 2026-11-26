//! The words the game shows the player: log lines for sim events.

use nighthawk_sim::{Event, Sound, Vertical};

/// Turns a sim event into a log line. Ordinary steps aren't worth logging.
///
/// Events are grouped (moving, climbing, doors) with a small function per
/// group, so no single function grows past the complexity limit as the game
/// gains more kinds of event.
pub fn describe(event: Event) -> Option<String> {
    let line = match event {
        Event::Moved { .. } => return None,
        Event::Bumped { tile, .. } => format!("You bump into the {}.", tile.name()),
        Event::BlockedByEdge => "There is nowhere to go that way.".into(),
        Event::Waited => "You wait.".into(),
        Event::Climbed { way, .. } => climbed_text(way).into(),
        Event::NoStairs { way } => no_stairs_text(way).into(),
        Event::DoorOpened { .. } | Event::DoorClosed { .. } | Event::NothingToClose { .. } => {
            door_text(event).into()
        }
        Event::Ambient(sound) => sound_text(sound).into(),
    };
    Some(line)
}

fn climbed_text(way: Vertical) -> &'static str {
    match way {
        Vertical::Up => "You climb the ladder up.",
        Vertical::Down => "You climb the ladder down.",
    }
}

fn no_stairs_text(way: Vertical) -> &'static str {
    match way {
        Vertical::Up => "There's no way up here.",
        Vertical::Down => "There's no way down here.",
    }
}

/// Log lines for door events. Other events never reach here.
fn door_text(event: Event) -> &'static str {
    match event {
        Event::DoorOpened { .. } => "You open the door.",
        Event::DoorClosed { .. } => "You close the door.",
        _ => "There's no open door there.",
    }
}

/// What each ambient sound sounds like, in the gritty tone of D4.
pub fn sound_text(sound: Sound) -> &'static str {
    match sound {
        Sound::VentTick => "A ventilation fan ticks somewhere overhead.",
        Sound::HullCreak => "The hull creaks as it cools.",
        Sound::DistantClang => "Something metal clangs, far off.",
        Sound::CoolantDrip => "Coolant drips onto a grating nearby.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nighthawk_sim::{Pos, Tile};

    #[test]
    fn ordinary_steps_are_not_logged_but_bumps_are() {
        assert_eq!(
            describe(Event::Moved {
                to: Pos::new(0, 0, 0)
            }),
            None
        );
        let bump = Event::Bumped {
            at: Pos::new(0, 0, 0),
            tile: Tile::Wall,
        };
        assert_eq!(
            describe(bump).as_deref(),
            Some("You bump into the bulkhead.")
        );
    }

    #[test]
    fn edges_and_waiting_have_lines() {
        assert_eq!(
            describe(Event::BlockedByEdge).as_deref(),
            Some("There is nowhere to go that way.")
        );
        assert_eq!(describe(Event::Waited).as_deref(), Some("You wait."));
    }

    #[test]
    fn climbing_has_lines_for_both_ways_and_for_no_stairs() {
        let at = Pos::new(0, 0, 1);
        assert_eq!(
            describe(Event::Climbed {
                to: at,
                way: Vertical::Up
            })
            .as_deref(),
            Some("You climb the ladder up.")
        );
        assert_eq!(
            describe(Event::Climbed {
                to: at,
                way: Vertical::Down
            })
            .as_deref(),
            Some("You climb the ladder down.")
        );
        assert_eq!(
            describe(Event::NoStairs { way: Vertical::Up }).as_deref(),
            Some("There's no way up here.")
        );
        assert_eq!(
            describe(Event::NoStairs {
                way: Vertical::Down
            })
            .as_deref(),
            Some("There's no way down here.")
        );
    }

    #[test]
    fn doors_have_lines() {
        let at = Pos::new(1, 1, 0);
        assert_eq!(
            describe(Event::DoorOpened { at }).as_deref(),
            Some("You open the door.")
        );
        assert_eq!(
            describe(Event::DoorClosed { at }).as_deref(),
            Some("You close the door.")
        );
        assert_eq!(
            describe(Event::NothingToClose { at }).as_deref(),
            Some("There's no open door there.")
        );
    }

    #[test]
    fn every_ambient_sound_has_its_own_line() {
        let lines: Vec<_> = Sound::ALL
            .iter()
            .map(|&s| describe(Event::Ambient(s)).unwrap())
            .collect();
        for (i, line) in lines.iter().enumerate() {
            assert!(!line.is_empty());
            assert!(!lines[i + 1..].contains(line), "{line:?} is used twice");
        }
    }
}
