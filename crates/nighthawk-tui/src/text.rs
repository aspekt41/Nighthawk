//! The words the game shows the player: log lines for sim events.

use nighthawk_sim::{
    Action, Blocked, BreathLevel, Cause, Event, ItemRef, Sound, Vertical, WarmthLevel, World,
};

/// Like [`describe`], but with the world to hand, so a bump names the
/// structure you walked into ("the hull plating", not just "the wall").
pub fn describe_in(world: &World, event: Event) -> Option<String> {
    match event {
        Event::Bumped { at, .. } if let Some(structure) = world.structure_at(at) => {
            Some(format!("You bump into the {}.", structure.name))
        }
        Event::PickedUp(id) | Event::NoRoom(id) | Event::Dropped(id) => {
            Some(item_text(event, &item_name(world, id)))
        }
        _ => describe(event),
    }
}

/// An item's name and material, like "basic laser cutter (Aluminium Alloy)".
pub fn item_name(world: &World, id: ItemRef) -> String {
    let kind = world.item_types().get(usize::from(id.kind));
    let material = world.materials().get(usize::from(id.material));
    match (kind, material) {
        (Some(kind), Some(material)) => format!("{} ({})", kind.name, material.name),
        (Some(kind), None) => kind.name.clone(),
        _ => "something".into(),
    }
}

/// Log lines for picking up and dropping, naming the item `name`.
fn item_text(event: Event, name: &str) -> String {
    match event {
        Event::PickedUp(_) => format!("You pick up the {name}."),
        Event::NoRoom(_) => format!("There's no room in your pockets for the {name}."),
        Event::Dropped(_) => format!("You drop the {name}."),
        _ => "There's nothing there to drop.".into(),
    }
}

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
        Event::Unable { blocked, .. } => blocked_text(blocked),
        Event::PickedUp(_) | Event::NoRoom(_) | Event::Dropped(_) | Event::NothingToDrop => {
            item_text(event, "item")
        }
        Event::AirEscaping | Event::Breath(_) | Event::Warmth(_) | Event::BlackedOut(_) => {
            survival_text(event).into()
        }
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

/// Log lines for air, breath, warmth and blacking out.
fn survival_text(event: Event) -> &'static str {
    match event {
        Event::AirEscaping => "Air hisses out somewhere nearby.",
        Event::Breath(level) => breath_text(level),
        Event::Warmth(level) => warmth_text(level),
        Event::BlackedOut(Cause::Cold) => "You stop shivering. Everything goes dark.",
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

/// Warnings as the player gets colder, and relief when they warm up.
fn warmth_text(level: WarmthLevel) -> &'static str {
    match level {
        WarmthLevel::Chilled => "You shiver. It's cold here.",
        WarmthLevel::Numb => "Your fingers are numb. You need to get warm.",
        WarmthLevel::Freezing => "You can't stop shaking. You're freezing!",
        WarmthLevel::Recovered => "The warmth comes back into your hands.",
    }
}

/// The menu's name for an action.
pub fn action_name(action: Action) -> &'static str {
    match action {
        Action::Open => "Open",
        Action::Close => "Close",
        Action::Cut => "Cut",
        Action::PickUp => "Pick up",
    }
}

/// A short reason an action can't be done, for under it in the interact
/// menu (at most 19 characters, to fit the panel).
pub fn blocked_short(blocked: Blocked) -> String {
    match blocked {
        Blocked::TooFar { reach } => format!("too far: reach {reach}"),
        Blocked::InTheWay { .. } => "path blocked".into(),
        Blocked::StandingInIt => "step out first".into(),
        Blocked::Needs { quality, level } => format!("needs {} {level}", quality.name()),
        Blocked::Uncuttable => "nothing cuts it".into(),
        Blocked::NotPossible => "not possible".into(),
    }
}

/// The log line for an action that couldn't be done.
pub fn blocked_text(blocked: Blocked) -> String {
    match blocked {
        Blocked::TooFar { reach: 1 } => {
            "That's too far away: you need to be right beside it.".into()
        }
        Blocked::TooFar { reach } => {
            format!("That's out of reach: you need to be within {reach} tiles.")
        }
        Blocked::InTheWay { .. } => "Something solid is in the way.".into(),
        Blocked::StandingInIt => "You're standing in the doorway.".into(),
        Blocked::Needs { quality, level } => {
            format!("You need a tool with {} {level} for that.", quality.name())
        }
        Blocked::Uncuttable => "Nothing you could carry would cut that.".into(),
        Blocked::NotPossible => "You can't do that there.".into(),
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
            describe(Event::BlackedOut(Cause::Air)).as_deref(),
            Some("Your vision goes grey. You black out.")
        );
        assert_eq!(
            describe(Event::BlackedOut(Cause::Cold)).as_deref(),
            Some("You stop shivering. Everything goes dark.")
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
    fn warmth_has_a_line_for_each_level() {
        let levels = [
            WarmthLevel::Chilled,
            WarmthLevel::Numb,
            WarmthLevel::Freezing,
            WarmthLevel::Recovered,
        ];
        let lines: Vec<String> = levels
            .iter()
            .map(|&l| describe(Event::Warmth(l)).unwrap())
            .collect();
        assert_eq!(lines[0], "You shiver. It's cold here.");
        assert!(lines[2].contains("freezing"));
        for (i, line) in lines.iter().enumerate() {
            assert!(!lines[i + 1..].contains(line), "{line:?} is used twice");
        }
    }

    /// One of each kind of blocked reason.
    fn every_blocked() -> [Blocked; 7] {
        [
            Blocked::TooFar { reach: 1 },
            Blocked::TooFar { reach: 2 },
            Blocked::InTheWay {
                at: Pos::new(0, 0, 0),
            },
            Blocked::StandingInIt,
            Blocked::Needs {
                quality: nighthawk_sim::Quality::Laser,
                level: 5,
            },
            Blocked::Uncuttable,
            Blocked::NotPossible,
        ]
    }

    #[test]
    fn blocked_actions_explain_themselves_in_the_log() {
        let unable = Event::Unable {
            action: Action::Cut,
            blocked: Blocked::Needs {
                quality: nighthawk_sim::Quality::Laser,
                level: 5,
            },
        };
        assert_eq!(
            describe(unable).as_deref(),
            Some("You need a tool with LASER 5 for that.")
        );
        assert_eq!(
            blocked_text(Blocked::TooFar { reach: 1 }),
            "That's too far away: you need to be right beside it."
        );
        assert_eq!(
            blocked_text(Blocked::TooFar { reach: 2 }),
            "That's out of reach: you need to be within 2 tiles."
        );
        let lines: Vec<String> = every_blocked().map(blocked_text).to_vec();
        for (i, line) in lines.iter().enumerate() {
            assert!(!lines[i + 1..].contains(line), "{line:?} is used twice");
        }
    }

    #[test]
    fn short_reasons_fit_under_an_option_in_the_panel() {
        for blocked in every_blocked() {
            let short = blocked_short(blocked);
            assert!(short.chars().count() <= 19, "{short:?}");
        }
        assert_eq!(
            blocked_short(Blocked::TooFar { reach: 1 }),
            "too far: reach 1"
        );
        assert_eq!(
            blocked_short(Blocked::Needs {
                quality: nighthawk_sim::Quality::Laser,
                level: 1
            }),
            "needs LASER 1"
        );
    }

    #[test]
    fn actions_have_menu_names() {
        assert_eq!(
            Action::ALL.map(action_name),
            ["Open", "Close", "Cut", "Pick up"]
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

    #[test]
    fn a_bump_names_the_structure_you_walked_into() {
        let world = nighthawk_sim::World::new(crate::content::load_start_map().unwrap(), 1);
        let bump = Event::Bumped {
            at: Pos::new(0, 7, 0),
            tile: Tile::Wall,
        };
        assert_eq!(
            describe_in(&world, bump).as_deref(),
            Some("You bump into the hull plating.")
        );
        assert_eq!(
            describe_in(&world, Event::Waited).as_deref(),
            Some("You wait."),
            "other events read as usual"
        );
    }

    #[test]
    fn item_events_name_the_item_and_its_material() {
        let world = nighthawk_sim::World::new(crate::content::load_start_map().unwrap(), 1);
        let laser = world.floor_items()[0].item.id();
        let lines: Vec<String> = [
            Event::PickedUp(laser),
            Event::NoRoom(laser),
            Event::Dropped(laser),
        ]
        .into_iter()
        .filter_map(|e| describe_in(&world, e))
        .collect();
        assert_eq!(
            lines,
            vec![
                "You pick up the basic laser cutter (Aluminium Alloy).",
                "There's no room in your pockets for the basic laser cutter (Aluminium Alloy).",
                "You drop the basic laser cutter (Aluminium Alloy).",
            ]
        );
        assert_eq!(
            describe_in(&world, Event::NothingToDrop).as_deref(),
            Some("There's nothing there to drop.")
        );
        assert_eq!(
            describe(Event::PickedUp(laser)).as_deref(),
            Some("You pick up the item.")
        );
    }

    #[test]
    fn unknown_items_still_get_a_name() {
        let world = nighthawk_sim::World::new(crate::content::load_start_map().unwrap(), 1);
        let odd = ItemRef {
            kind: 0,
            material: 99,
        };
        assert_eq!(item_name(&world, odd), "jumpsuit");
        let odder = ItemRef {
            kind: 99,
            material: 0,
        };
        assert_eq!(item_name(&world, odder), "something");
    }
}
