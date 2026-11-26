//! The words the game shows the player: log lines for sim events.

use nighthawk_sim::{BreathLevel, Event, Sound, Vertical};

/// Turns a sim event into a log line. Ordinary steps aren't worth logging.
///
/// Events are grouped (moving about, survival), with a small function per
/// group, so no single function grows past the complexity limit as the game
/// gains more kinds of event.
pub fn describe(event: Event) -> Option<String> {
    let line = match event {
        Event::Moved { .. } => return None,
        Event::Bumped { tile, .. } => format!("You bump into the {}.", tile.name()),
        Event::Ambient(sound) => sound_text(sound).into(),
        Event::AirEscaping | Event::Breath(_) | Event::BlackedOut => survival_text(event).into(),
        _ => movement_text(event).into(),
    };
    Some(line)
}

/// Log lines for getting about: edges, waiting, ladders and doors.
fn movement_text(event: Event) -> &'static str {
    match event {
        Event::BlockedByEdge => "There is nowhere to go that way.",
        Event::Waited => "You wait.",
        Event::Climbed { way, .. } => climbed_text(way),
        Event::NoStairs { way } => no_stairs_text(way),
        // Only door events are left by the time `describe` calls this.
        _ => door_text(event),
    }
}

/// Log lines for air and breath.
fn survival_text(event: Event) -> &'static str {
    match event {
        Event::AirEscaping => "Air hisses out somewhere nearby.",
        Event::Breath(level) => breath_text(level),
        _ => "Your vision goes grey. You black out.",
    }
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

/// Warnings as the player's breath runs down, and relief when it comes back.
fn breath_text(level: BreathLevel) -> &'static str {
    match level {
        BreathLevel::Short => "You're short of breath. The air here is thin.",
        BreathLevel::Struggling => "Your vision narrows. You need air.",
        BreathLevel::Fading => "Your chest burns. You're about to black out!",
        BreathLevel::Recovered => "You catch your breath.",
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
    fn air_and_breath_have_lines() {
        assert_eq!(
            describe(Event::AirEscaping).as_deref(),
            Some("Air hisses out somewhere nearby.")
        );
        assert_eq!(
            describe(Event::BlackedOut).as_deref(),
            Some("Your vision goes grey. You black out.")
        );
        let levels = [
            BreathLevel::Short,
            BreathLevel::Struggling,
            BreathLevel::Fading,
            BreathLevel::Recovered,
        ];
        let lines: Vec<String> = levels
            .iter()
            .map(|&l| describe(Event::Breath(l)).unwrap())
            .collect();
        assert!(lines[0].starts_with("You're short of breath"));
        assert!(lines[2].contains("about to black out"));
        assert_eq!(lines[3], "You catch your breath.");
        for (i, line) in lines.iter().enumerate() {
            assert!(!lines[i + 1..].contains(line), "{line:?} is used twice");
        }
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
