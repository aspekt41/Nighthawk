//! The words the game shows the player: log lines for sim events.

use nighthawk_sim::{
    Action, Blocked, BreathLevel, Cause, DateTime, Event, ItemRef, Pos, Sound, Vertical,
    WarmthLevel, World,
};

/// What you'd bump into at `at`: the furniture there, or else the
/// structure ("the stasis chamber", "the hull plating").
fn solid_name(world: &World, at: Pos) -> Option<&str> {
    let furniture = world.furniture_kind_at(at).map(|k| k.name.as_str());
    furniture.or_else(|| world.structure_at(at).map(|s| s.name.as_str()))
}

/// Like [`describe`], but with the world to hand, so a bump names what
/// you walked into ("the hull plating", not just "the wall").
pub fn describe_in(world: &World, event: Event) -> Option<String> {
    match event {
        Event::Bumped { at, .. } if let Some(name) = solid_name(world, at) => {
            Some(format!("You bump into the {name}."))
        }
        Event::CannotCraft { blocked, .. } => Some(blocked_text(blocked, u16::MAX, Some(world))),
        Event::Stored { item, into } => Some(stored_text(world, item, into)),
        Event::Transformed { at, action, was } => Some(done_text(world, at, action, was)),
        Event::Removed { action, was, .. } => Some(removed_text(world, action, was)),
        Event::Unable { action, blocked } => Some(blocked_text(blocked, action, Some(world))),
        // Events about items get their own function, so this `match`
        // stays under the complexity limit.
        _ => item_event_text(world, event)
            .or_else(|| power_text(Some(world), event))
            .or_else(|| dress_text(Some(world), event))
            .or_else(|| describe(event)),
    }
}

/// Log lines for putting things on and taking them off (decision D61),
/// and for coming to after blacking out (D63). `world`, if given, names
/// the item or the furniture. `None` for any other event.
fn dress_text(world: Option<&World>, event: Event) -> Option<String> {
    let name = |id| world.map_or_else(|| "item".into(), |w| item_name(w, id));
    match event {
        Event::PutOn(item) => Some(format!("You put on the {}.", name(item))),
        Event::TookOff(item) => Some(format!("You take off the {}.", name(item))),
        Event::CannotDress(blocked) => Some(cannot_dress_text(world, blocked)),
        Event::WokeUp { beside } => Some(woke_text(world, beside)),
        _ => None,
    }
}

/// "You come to beside the stasis chamber, with no memory of getting here."
fn woke_text(world: Option<&World>, beside: u16) -> String {
    let furniture = world.and_then(|w| w.content().furniture().get(usize::from(beside)));
    let name = furniture.map_or("pod", |f| f.name.as_str());
    format!("You come to beside the {name}, with no memory of getting here.")
}

/// Why and when you blacked out: "You blacked out from lack of air at 25
/// Nov 2631 09:12:40."
pub fn blackout_text(cause: Cause, when: DateTime) -> String {
    let why = match cause {
        Cause::Air => "from lack of air",
        Cause::Cold => "from the cold",
    };
    format!("You blacked out {why} at {when}.")
}

/// Why you can't put something on or take it off: "Take off your deck
/// shoes first." `world`, if given, names what's in the way.
pub fn cannot_dress_text(world: Option<&World>, blocked: Blocked) -> String {
    match blocked {
        Blocked::Wearing { item } => {
            format!("Take off your {} first.", item_type_name(world, item))
        }
        _ => "You can't wear that.".into(),
    }
}

/// Log lines for loading, unloading and switching things, and for a light
/// going out (decision D59). `world`, if given, names the item. `None`
/// for any other event.
fn power_text(world: Option<&World>, event: Event) -> Option<String> {
    let name = |id| world.map_or_else(|| "item".into(), |w| item_name(w, id));
    let line = match event {
        Event::Reloaded { item, charge_kj } => {
            format!("You load a cell ({charge_kj} kJ) into the {}.", name(item))
        }
        Event::Unloaded(item) => format!("You take the cell out of the {}.", name(item)),
        Event::Switched { item, on } => {
            let state = if on { "on" } else { "off" };
            format!("You switch the {} {state}.", name(item))
        }
        Event::WentOut(item) => {
            format!(
                "Your {} flickers and goes out: its cell is flat.",
                name(item)
            )
        }
        Event::CannotPower(blocked) => cannot_power_text(blocked).into(),
        _ => return None,
    };
    Some(line)
}

/// Why you couldn't load, unload or switch something.
fn cannot_power_text(blocked: Blocked) -> &'static str {
    match blocked {
        Blocked::NoFullerCell => "You carry no fuller cell that fits it.",
        Blocked::NoCell => "There's no cell in it.",
        Blocked::Flat => "Its cell is flat.",
        _ => "You can't do that with it.",
    }
}

/// Log lines for events about one item, naming it and its material: what
/// you picked up, took out, dropped, made, or saw fall. `None` for any
/// other event.
fn item_event_text(world: &World, event: Event) -> Option<String> {
    match event {
        Event::PickedUp(id) | Event::NoRoom(id) | Event::Dropped(id) | Event::Fell(id) => {
            Some(item_text(event, &item_name(world, id)))
        }
        Event::TookOut { item, from } => Some(took_out_text(world, item, from)),
        Event::Crafted { item, .. } => {
            Some(format!("You make {}.", a_or_an(&item_name(world, item))))
        }
        Event::Pocketed { item, worn, pocket } => Some(format!(
            "You put the {} in your {}.",
            item_name(world, item),
            pocket_name(world, worn, pocket)
        )),
        _ => None,
    }
}

/// The name of pocket number `pocket` of worn item number `worn`, like
/// "left hip pocket", or just "pocket" if there's no such pocket.
pub fn pocket_name(world: &World, worn: usize, pocket: usize) -> String {
    let wearing = world.worn().get(worn);
    let kind = wearing.and_then(|w| world.item_types().get(usize::from(w.kind)));
    let found = kind.and_then(|k| k.pockets().get(pocket));
    found.map_or_else(|| "pocket".into(), |p| p.name.clone())
}

/// `name` after "a", or "an" if it starts with a vowel: "a shiv (Bone)",
/// "an item".
fn a_or_an(name: &str) -> String {
    let vowel = name.starts_with(['a', 'e', 'i', 'o', 'u']);
    format!("{} {name}", if vowel { "an" } else { "a" })
}

/// "You put the shiv (Bone) in the locker."
fn stored_text(world: &World, item: ItemRef, into: u16) -> String {
    let furniture = world.content().furniture().get(usize::from(into));
    let container = furniture.map_or("container", |f| f.name.as_str());
    format!("You put the {} in the {container}.", item_name(world, item))
}

/// Why you couldn't put something away.
fn cannot_store_text(blocked: Blocked) -> String {
    match blocked {
        Blocked::NotPossible => "There's nothing there to put things in.".into(),
        _ => blocked_text_fixed(blocked),
    }
}

/// "You take the hand lamp (Aluminium Alloy) out of the locker."
fn took_out_text(world: &World, item: ItemRef, from: u16) -> String {
    let furniture = world.content().furniture().get(usize::from(from));
    let container = furniture.map_or("container", |f| f.name.as_str());
    let name = item_name(world, item);
    format!("You take the {name} out of the {container}.")
}

/// The action's `done` log line (from `data/actions/`), naming the
/// structure as it was and as it is now: "You cut through the door."
fn done_text(world: &World, at: Pos, action: u16, was: u16) -> String {
    let now = world.structure_at(at).map_or("thing", |s| s.name.as_str());
    action_of(Some(world), action).map_or_else(
        || "Done.".into(),
        |a| a.done_text(structure_name(world, was), now),
    )
}

/// The action's `done` log line for furniture taken away, naming it:
/// "You smash the bunk to pieces."
fn removed_text(world: &World, action: u16, was: u16) -> String {
    let furniture = world.content().furniture().get(usize::from(was));
    let name = furniture.map_or("thing", |f| f.name.as_str());
    action_of(Some(world), action).map_or_else(|| "Done.".into(), |a| a.done_text(name, ""))
}

/// The name of structure number `index`, or "thing" if there's no such one.
fn structure_name(world: &World, index: u16) -> &str {
    world
        .structures()
        .get(usize::from(index))
        .map_or("thing", |s| s.name.as_str())
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
        Event::TookOut { .. } => format!("You take out the {name}."),
        Event::Fell(_) => format!("The {name} falls to the floor."),
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
        // Without the world to name the structure ([`describe_in`] does).
        Event::Bumped { .. } => "You bump into something solid.".into(),
        Event::Ambient(sound) => sound_text(sound).into(),
        Event::Unable { action, blocked } => blocked_text(blocked, action, None),
        Event::PickedUp(_)
        | Event::TookOut { .. }
        | Event::Fell(_)
        | Event::NoRoom(_)
        | Event::Dropped(_)
        | Event::NothingToDrop => item_text(event, "item"),
        Event::Transformed { .. } | Event::Removed { .. } => "Done.".into(),
        Event::Crafted { .. } => "You make something.".into(),
        Event::CannotCraft { blocked, .. } => blocked_text(blocked, u16::MAX, None),
        Event::AirEscaping | Event::Breath(_) | Event::Warmth(_) | Event::BlackedOut(_) => {
            survival_text(event).into()
        }
        _ => storing_text(event),
    };
    Some(line)
}

/// Log lines for putting things away, without the world to name them,
/// and else for getting about.
fn storing_text(event: Event) -> String {
    match event {
        Event::Stored { .. } => "You put it away.".into(),
        Event::Pocketed { .. } => "You move it to another pocket.".into(),
        Event::CannotStore(blocked) => cannot_store_text(blocked),
        _ => power_text(None, event)
            .or_else(|| dress_text(None, event))
            .unwrap_or_else(|| movement_text(event).into()),
    }
}

/// Log lines for getting about: edges, waiting and ladders. (Opening and
/// closing doors are recipe actions, logged with their `done` lines.)
fn movement_text(event: Event) -> &'static str {
    match event {
        Event::BlockedByEdge => "There is nowhere to go that way.",
        Event::Climbed { way, .. } => climbed_text(way),
        Event::NoStairs { way } => no_stairs_text(way),
        // Only waiting is left by the time `describe` calls this.
        _ => "You wait.",
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

/// Action number `action`, if there's a world to ask and it has one.
fn action_of(world: Option<&World>, action: u16) -> Option<&Action> {
    world.and_then(|w| w.content().actions().get(usize::from(action)))
}

/// The menu's name for action number `action`, like "Cut" (from
/// `data/actions/`), or "?" for one that isn't there.
pub fn action_name(world: &World, action: u16) -> &str {
    action_of(Some(world), action).map_or("?", |a| a.name.as_str())
}

/// The action's name as a verb for a sentence, like "cut" or "pick up",
/// or "do" without a world to ask.
fn verb(world: Option<&World>, action: u16) -> String {
    action_of(world, action).map_or_else(|| "do".into(), |a| a.name.to_lowercase())
}

/// The name of item type number `item`, or "item" without a world to ask.
fn item_type_name(world: Option<&World>, item: u16) -> String {
    world
        .and_then(|w| w.item_types().get(usize::from(item)))
        .map_or_else(|| "item".into(), |t| t.name.clone())
}

/// The name of item type number `item`, after the material tag number
/// `tag` if there is one: "metal scrap".
fn tagged_item_name(world: Option<&World>, item: u16, tag: Option<u16>) -> String {
    let tags = world
        .map(|w| w.content().material_tags())
        .unwrap_or_default();
    let tag = tag.and_then(|t| tags.get(usize::from(t)).copied());
    let name = item_type_name(world, item);
    match tag {
        Some(tag) => format!("{tag} {name}"),
        None => name,
    }
}

/// The id of quality number `quality`, like "LASER", if there's a world
/// to ask and it has one.
fn quality_id(world: Option<&World>, quality: u16) -> Option<&str> {
    world
        .and_then(|w| w.content().qualities().get(usize::from(quality)))
        .map(|q| q.id.as_str())
}

/// A short reason action number `action` can't be done, for under it in
/// the interact menu (at most 19 characters, to fit the panel; longer ones
/// are cut). `world`, if given, names the action, items and qualities.
pub fn blocked_short(blocked: Blocked, action: u16, world: Option<&World>) -> String {
    blocked_reason(blocked, action, world)
        .chars()
        .take(19)
        .collect()
}

/// The same short reason, uncut, for where there's room, like the
/// crafting screen: "needs 1 ductile scrap".
pub fn blocked_reason(blocked: Blocked, action: u16, world: Option<&World>) -> String {
    match blocked {
        Blocked::Missing {
            item, need, tag, ..
        } => {
            format!("needs {need} {}", tagged_item_name(world, item, tag))
        }
        Blocked::Needs { quality, level } => match quality_id(world, quality) {
            Some(id) => format!("needs {id} {level}"),
            None => "needs a better tool".into(),
        },
        Blocked::Cluttered => "clear it first".into(),
        Blocked::Unworkable => format!("nothing can {} it", verb(world, action)),
        Blocked::LowCharge {
            need_kj, have_kj, ..
        } => format!("cell {have_kj}/{need_kj} kJ"),
        _ => blocked_short_fixed(blocked),
    }
}

/// The short reasons that need no names.
fn blocked_short_fixed(blocked: Blocked) -> String {
    match blocked {
        Blocked::TooFar { reach } => format!("too far: reach {reach}"),
        Blocked::InTheWay { .. } => "path blocked".into(),
        Blocked::StandingInIt => "step out first".into(),
        Blocked::CantSee => "can't see it".into(),
        Blocked::NotKnown => "not known yet".into(),
        Blocked::Full => "full".into(),
        _ => "not possible".into(),
    }
}

/// The log line for action number `action` when it couldn't be done.
/// `world`, if given, names the action, items and qualities.
pub fn blocked_text(blocked: Blocked, action: u16, world: Option<&World>) -> String {
    match blocked {
        Blocked::Unworkable => format!(
            "Nothing you could carry would {} that.",
            verb(world, action)
        ),
        Blocked::NotPossible => format!("There's nothing to {} there.", verb(world, action)),
        Blocked::Needs { quality, level } => match quality_id(world, quality) {
            Some(id) => format!("You need a tool with {id} {level} for that."),
            None => "You need a better tool for that.".into(),
        },
        Blocked::Missing {
            item,
            need,
            have,
            tag,
        } => format!(
            "You need {need} {} for that; you have {have}.",
            tagged_item_name(world, item, tag)
        ),
        Blocked::Cluttered => "Something is lying on it. Move it out of the way first.".into(),
        Blocked::LowCharge {
            item,
            need_kj,
            have_kj,
        } => format!(
            "The {}'s cell is too low for that: it needs {need_kj} kJ and has {have_kj}.",
            item_type_name(world, item)
        ),
        _ => blocked_text_fixed(blocked),
    }
}

/// The log lines that need no names.
fn blocked_text_fixed(blocked: Blocked) -> String {
    match blocked {
        Blocked::TooFar { reach: 1 } => {
            "That's too far away: you need to be right beside it.".into()
        }
        Blocked::TooFar { reach } => {
            format!("That's out of reach: you need to be within {reach} tiles.")
        }
        Blocked::InTheWay { .. } => "Something solid is in the way.".into(),
        Blocked::StandingInIt => "You're standing in it. Step out first.".into(),
        Blocked::CantSee => "You can't see there.".into(),
        Blocked::NotKnown => "You don't know how to make that yet.".into(),
        Blocked::Full => "There's no room left in there.".into(),
        _ => "You can't do that there.".into(),
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
            tile: Tile::default(),
        };
        assert_eq!(
            describe(bump).as_deref(),
            Some("You bump into something solid."),
            "without a world to name it (describe_in names it)"
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
    fn opening_and_closing_doors_read_from_the_actions_done_lines() {
        let world = real_world();
        let door = Pos::new(6, 7, 0);
        let (shut, open) = (
            world.content().structure_index("door").unwrap(),
            world.content().structure_index("door_open").unwrap(),
        );
        let opened = Event::Transformed {
            at: door,
            action: number(&world, "open"),
            was: shut,
        };
        // The side room's door is shut in the real map, so "now" reads
        // "door" here; the line only uses "was".
        assert_eq!(
            describe_in(&world, opened).as_deref(),
            Some("You open the door.")
        );
        let closed = Event::Transformed {
            at: door,
            action: number(&world, "close"),
            was: open,
        };
        assert_eq!(
            describe_in(&world, closed).as_deref(),
            Some("You close the door.")
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

    /// A new game on the real starting map, with the shipped content.
    fn real_world() -> World {
        World::new(crate::content::load_start_map().unwrap(), 1)
    }

    /// The number of the shipped action with this id.
    fn number(world: &World, id: &str) -> u16 {
        world.content().action_index(id).unwrap()
    }

    /// One of each kind of blocked reason.
    fn every_blocked() -> [Blocked; 13] {
        [
            Blocked::CantSee,
            Blocked::NotKnown,
            Blocked::Full,
            Blocked::TooFar { reach: 1 },
            Blocked::TooFar { reach: 2 },
            Blocked::InTheWay {
                at: Pos::new(0, 0, 0),
            },
            Blocked::StandingInIt,
            Blocked::Needs {
                quality: 0,
                level: 5,
            },
            Blocked::Unworkable,
            Blocked::NotPossible,
            Blocked::LowCharge {
                item: 0,
                need_kj: 6,
                have_kj: 1,
            },
            Blocked::Missing {
                item: 0,
                need: 2,
                have: 1,
                tag: None,
            },
            Blocked::Cluttered,
        ]
    }

    #[test]
    fn blocked_actions_explain_themselves_in_the_log() {
        // The real content's first quality is LASER.
        let world = real_world();
        let (laser, cut) = (
            world.content().quality_index("LASER").unwrap(),
            number(&world, "cut"),
        );
        let unable = Event::Unable {
            action: cut,
            blocked: Blocked::Needs {
                quality: laser,
                level: 5,
            },
        };
        assert_eq!(
            describe_in(&world, unable).as_deref(),
            Some("You need a tool with LASER 5 for that.")
        );
        assert_eq!(
            describe(unable).as_deref(),
            Some("You need a better tool for that."),
            "without a world to name the quality"
        );
        assert_eq!(
            blocked_text(Blocked::TooFar { reach: 1 }, cut, None),
            "That's too far away: you need to be right beside it."
        );
        assert_eq!(
            blocked_text(Blocked::TooFar { reach: 2 }, cut, None),
            "That's out of reach: you need to be within 2 tiles."
        );
        let lines: Vec<String> = every_blocked()
            .map(|b| blocked_text(b, cut, Some(&world)))
            .to_vec();
        for (i, line) in lines.iter().enumerate() {
            assert!(!lines[i + 1..].contains(line), "{line:?} is used twice");
        }
    }

    #[test]
    fn reasons_that_depend_on_the_action_use_its_name_as_a_verb() {
        let world = real_world();
        let (cut, close) = (number(&world, "cut"), number(&world, "close"));
        assert_eq!(
            blocked_text(Blocked::Unworkable, cut, Some(&world)),
            "Nothing you could carry would cut that."
        );
        assert_eq!(
            blocked_text(Blocked::NotPossible, close, Some(&world)),
            "There's nothing to close there."
        );
        assert_eq!(
            blocked_text(Blocked::NotPossible, close, None),
            "There's nothing to do there.",
            "without a world to name the action"
        );
        assert_eq!(
            blocked_short(Blocked::Unworkable, cut, Some(&world)),
            "nothing can cut it"
        );
        assert_eq!(
            blocked_text(Blocked::NotPossible, 999, Some(&world)),
            "There's nothing to do there.",
            "an action that isn't there"
        );
    }

    #[test]
    fn short_reasons_fit_under_an_option_in_the_panel() {
        let world = real_world();
        for action in 0..world.content().actions().len() {
            let action = u16::try_from(action).unwrap();
            for blocked in every_blocked() {
                let short = blocked_short(blocked, action, Some(&world));
                assert!(short.chars().count() <= 19, "{short:?}");
            }
        }
        assert_eq!(
            blocked_short(Blocked::TooFar { reach: 1 }, 0, None),
            "too far: reach 1"
        );
        assert_eq!(blocked_short(Blocked::CantSee, 0, None), "can't see it");
        assert_eq!(
            blocked_text(Blocked::CantSee, 0, None),
            "You can't see there."
        );
        let needs = Blocked::Needs {
            quality: world.content().quality_index("LASER").unwrap(),
            level: 1,
        };
        assert_eq!(blocked_short(needs, 0, Some(&world)), "needs LASER 1");
        assert_eq!(blocked_short(needs, 0, None), "needs a better tool");
    }

    #[test]
    fn actions_have_menu_names_from_the_content() {
        let world = real_world();
        let names: Vec<&str> = (0..5).map(|n| action_name(&world, n)).collect();
        assert_eq!(names, ["Open", "Close", "Cut", "Repair", "Pick up"]);
        assert_eq!(action_name(&world, 999), "?");
    }

    #[test]
    fn crafting_is_logged_with_an_article_and_refusals_say_why() {
        let world = real_world();
        let shiv = ItemRef {
            kind: world.item_type_index("shiv").unwrap(),
            material: world.content().material_index("bone").unwrap(),
        };
        let made = Event::Crafted {
            craft: 0,
            item: shiv,
        };
        assert_eq!(
            describe_in(&world, made).as_deref(),
            Some("You make a shiv (Bone).")
        );
        assert_eq!(describe(made).as_deref(), Some("You make something."));
        assert_eq!(a_or_an("item"), "an item");
        let unknown = Event::CannotCraft {
            craft: 0,
            blocked: Blocked::NotKnown,
        };
        let line = "You don't know how to make that yet.";
        assert_eq!(describe_in(&world, unknown).as_deref(), Some(line));
        assert_eq!(describe(unknown).as_deref(), Some(line));
        assert_eq!(blocked_short(Blocked::NotKnown, 0, None), "not known yet");
    }

    #[test]
    fn furniture_taken_away_and_what_falls_out_are_logged() {
        let world = real_world();
        let (smash, dismantle) = (number(&world, "smash"), number(&world, "dismantle"));
        let bunk = world.content().furniture_index("bunk").unwrap();
        let at = Pos::new(0, 0, 0);
        let removed = |action, was| {
            describe_in(&world, Event::Removed { at, action, was }).unwrap_or_default()
        };
        assert_eq!(removed(smash, bunk), "You smash the bunk to pieces.");
        assert_eq!(removed(dismantle, bunk), "You take the bunk apart.");
        assert_eq!(removed(smash, 999), "You smash the thing to pieces.");
        assert_eq!(removed(999, bunk), "Done.", "an action that isn't there");
        let scrap = ItemRef {
            kind: world.item_type_index("scrap").unwrap(),
            material: world.content().material_index("glass").unwrap(),
        };
        assert_eq!(
            describe_in(&world, Event::Fell(scrap)).as_deref(),
            Some("The scrap (Glass) falls to the floor.")
        );
        assert_eq!(
            describe(Event::Fell(scrap)).as_deref(),
            Some("The item falls to the floor.")
        );
        let unnamed = Event::Removed {
            at,
            action: smash,
            was: bunk,
        };
        assert_eq!(describe(unnamed).as_deref(), Some("Done."));
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
            tile: Tile::default(),
        };
        assert_eq!(
            describe_in(&world, bump).as_deref(),
            Some("You bump into the hull plating.")
        );
        let chamber = Event::Bumped {
            at: Pos::new(12, 6, 0),
            tile: Tile::default(),
        };
        assert_eq!(
            describe_in(&world, chamber).as_deref(),
            Some("You bump into the stasis chamber."),
            "furniture is named before the floor under it"
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
        // The cutter, first in the gear locker.
        let locker = world.furniture_at(Pos::new(7, 4, 0)).unwrap();
        let laser = locker.contents[0].id();
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
        let locker = world.content().furniture_index("locker").unwrap();
        let took = Event::TookOut {
            item: laser,
            from: locker,
        };
        assert_eq!(
            describe_in(&world, took).as_deref(),
            Some("You take the basic laser cutter (Aluminium Alloy) out of the locker.")
        );
        let from_nothing = Event::TookOut {
            item: laser,
            from: 999,
        };
        assert_eq!(
            describe_in(&world, from_nothing).as_deref(),
            Some("You take the basic laser cutter (Aluminium Alloy) out of the container.")
        );
        assert_eq!(describe(took).as_deref(), Some("You take out the item."));
    }

    #[test]
    fn loading_unloading_and_switching_say_what_happened_or_why_not() {
        let world = real_world();
        let lamp = ItemRef {
            kind: world.item_type_index("hand_lamp").unwrap(),
            material: world.content().material_index("aluminium_alloy").unwrap(),
        };
        let line = |event| describe_in(&world, event).unwrap_or_default();
        let loaded = Event::Reloaded {
            item: lamp,
            charge_kj: 120,
        };
        assert_eq!(
            line(loaded),
            "You load a cell (120 kJ) into the hand lamp (Aluminium Alloy)."
        );
        assert_eq!(
            describe(loaded).as_deref(),
            Some("You load a cell (120 kJ) into the item.")
        );
        assert_eq!(
            line(Event::Unloaded(lamp)),
            "You take the cell out of the hand lamp (Aluminium Alloy)."
        );
        let switched = |on| line(Event::Switched { item: lamp, on });
        assert_eq!(
            switched(true),
            "You switch the hand lamp (Aluminium Alloy) on."
        );
        assert_eq!(
            switched(false),
            "You switch the hand lamp (Aluminium Alloy) off."
        );
        assert_eq!(
            line(Event::WentOut(lamp)),
            "Your hand lamp (Aluminium Alloy) flickers and goes out: its cell is flat."
        );
        let cannot = |blocked| line(Event::CannotPower(blocked));
        assert_eq!(
            cannot(Blocked::NoFullerCell),
            "You carry no fuller cell that fits it."
        );
        assert_eq!(cannot(Blocked::NoCell), "There's no cell in it.");
        assert_eq!(cannot(Blocked::Flat), "Its cell is flat.");
        assert_eq!(cannot(Blocked::NotPossible), "You can't do that with it.");
        assert_eq!(
            describe(Event::CannotPower(Blocked::Flat)).as_deref(),
            Some("Its cell is flat.")
        );
    }

    #[test]
    fn a_long_reason_is_cut_for_the_interact_panel_but_whole_elsewhere() {
        let world = real_world();
        let ductile = world
            .content()
            .material_tags()
            .iter()
            .position(|&t| t == "ductile");
        let missing = Blocked::Missing {
            item: world.item_type_index("scrap").unwrap(),
            need: 1,
            have: 0,
            tag: ductile.map(|i| u16::try_from(i).unwrap()),
        };
        assert_eq!(
            blocked_reason(missing, 0, Some(&world)),
            "needs 1 ductile scrap"
        );
        assert_eq!(
            blocked_short(missing, 0, Some(&world)),
            "needs 1 ductile scr"
        );
    }

    #[test]
    fn blacking_out_and_coming_to_say_why_when_and_where() {
        let when = nighthawk_sim::SHIP_START;
        assert!(blackout_text(Cause::Air, when).starts_with("You blacked out from lack of air at"));
        assert!(blackout_text(Cause::Cold, when).starts_with("You blacked out from the cold at"));
        let world = real_world();
        let yours = world
            .content()
            .furniture_index("your_stasis_chamber")
            .unwrap();
        let woke = Event::WokeUp { beside: yours };
        assert_eq!(
            describe_in(&world, woke).as_deref(),
            Some("You come to beside the stasis chamber, with no memory of getting here.")
        );
        assert!(
            describe(woke)
                .unwrap()
                .starts_with("You come to beside the pod")
        );
    }

    #[test]
    fn moving_to_another_pocket_names_the_pocket() {
        let world = real_world();
        let shoes = ItemRef {
            kind: world.item_type_index("deck_shoes").unwrap(),
            material: world.content().material_index("polyester").unwrap(),
        };
        let moved = |worn, pocket| Event::Pocketed {
            item: shoes,
            worn,
            pocket,
        };
        assert_eq!(
            describe_in(&world, moved(0, 2)).as_deref(),
            Some("You put the deck shoes (Polyester) in your left leg pocket.")
        );
        assert_eq!(
            describe_in(&world, moved(5, 0)).as_deref(),
            Some("You put the deck shoes (Polyester) in your pocket."),
            "a pocket that isn't there"
        );
        assert_eq!(
            describe(moved(0, 2)).as_deref(),
            Some("You move it to another pocket.")
        );
    }

    #[test]
    fn putting_on_and_taking_off_say_what_and_why_not() {
        let world = real_world();
        let shoes = ItemRef {
            kind: world.item_type_index("deck_shoes").unwrap(),
            material: world.content().material_index("polyester").unwrap(),
        };
        let line = |event| describe_in(&world, event).unwrap_or_default();
        assert_eq!(
            line(Event::PutOn(shoes)),
            "You put on the deck shoes (Polyester)."
        );
        assert_eq!(
            line(Event::TookOff(shoes)),
            "You take off the deck shoes (Polyester)."
        );
        let wearing = Blocked::Wearing { item: shoes.kind };
        assert_eq!(
            line(Event::CannotDress(wearing)),
            "Take off your deck shoes first."
        );
        assert_eq!(
            line(Event::CannotDress(Blocked::NotPossible)),
            "You can't wear that."
        );
        assert_eq!(
            describe(Event::PutOn(shoes)).as_deref(),
            Some("You put on the item.")
        );
        assert_eq!(
            describe(Event::CannotDress(wearing)).as_deref(),
            Some("Take off your item first.")
        );
    }

    #[test]
    fn a_cell_too_low_for_the_work_says_by_how_much() {
        let world = real_world();
        let cutter = world.item_type_index("basic_laser_cutter").unwrap();
        let low = Blocked::LowCharge {
            item: cutter,
            need_kj: 120,
            have_kj: 30,
        };
        assert_eq!(
            blocked_text(low, 0, Some(&world)),
            "The basic laser cutter's cell is too low for that: it needs 120 kJ and has 30."
        );
        assert_eq!(blocked_short(low, 0, Some(&world)), "cell 30/120 kJ");
    }

    #[test]
    fn putting_things_away_says_what_went_where_or_why_not() {
        let world = real_world();
        let shiv = ItemRef {
            kind: world.item_type_index("shiv").unwrap(),
            material: world.content().material_index("bone").unwrap(),
        };
        let locker = world.content().furniture_index("locker").unwrap();
        let stored = |into| describe_in(&world, Event::Stored { item: shiv, into });
        assert_eq!(
            stored(locker).as_deref(),
            Some("You put the shiv (Bone) in the locker.")
        );
        assert_eq!(
            stored(999).as_deref(),
            Some("You put the shiv (Bone) in the container.")
        );
        let stored = Event::Stored {
            item: shiv,
            into: locker,
        };
        assert_eq!(describe(stored).as_deref(), Some("You put it away."));
        let cannot = |blocked| describe(Event::CannotStore(blocked)).unwrap_or_default();
        assert_eq!(cannot(Blocked::Full), "There's no room left in there.");
        assert_eq!(
            cannot(Blocked::NotPossible),
            "There's nothing there to put things in."
        );
        assert_eq!(cannot(Blocked::CantSee), "You can't see there.");
        assert_eq!(
            describe_in(&world, Event::CannotStore(Blocked::Full)).as_deref(),
            Some("There's no room left in there.")
        );
        assert_eq!(blocked_short(Blocked::Full, 0, None), "full");
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

    #[test]
    fn cutting_through_names_what_was_cut() {
        let world = real_world();
        let door = world.content().structure_index("door").unwrap();
        let cut = Event::Transformed {
            at: Pos::new(6, 7, 0),
            action: number(&world, "cut"),
            was: door,
        };
        assert_eq!(
            describe_in(&world, cut).as_deref(),
            Some("You cut through the door.")
        );
        let unknown = Event::Transformed {
            at: Pos::new(6, 7, 0),
            action: number(&world, "cut"),
            was: 999,
        };
        assert_eq!(
            describe_in(&world, unknown).as_deref(),
            Some("You cut through the thing."),
            "never panics, even for a structure that isn't there"
        );
        let no_action = Event::Transformed {
            at: Pos::new(6, 7, 0),
            action: 999,
            was: door,
        };
        assert_eq!(describe_in(&world, no_action).as_deref(), Some("Done."));
        assert_eq!(describe(cut).as_deref(), Some("Done."));
    }

    #[test]
    fn missing_items_are_named_when_the_world_is_to_hand() {
        let world = nighthawk_sim::World::new(crate::content::load_start_map().unwrap(), 1);
        let scrap = world.item_type_index("scrap").unwrap();
        let missing = Blocked::Missing {
            item: scrap,
            need: 3,
            have: 1,
            tag: None,
        };
        let repair = number(&world, "repair");
        assert_eq!(
            blocked_text(missing, repair, Some(&world)),
            "You need 3 scrap for that; you have 1."
        );
        assert_eq!(
            blocked_short(missing, repair, Some(&world)),
            "needs 3 scrap"
        );
        assert_eq!(
            blocked_text(missing, repair, None),
            "You need 3 item for that; you have 1.",
            "without a world it still says something"
        );
        let unable = Event::Unable {
            action: repair,
            blocked: missing,
        };
        assert_eq!(
            describe_in(&world, unable).as_deref(),
            Some("You need 3 scrap for that; you have 1.")
        );
        let metal = world
            .content()
            .material_tags()
            .iter()
            .position(|&t| t == "metal");
        let metal_only = Blocked::Missing {
            item: scrap,
            need: 3,
            have: 0,
            tag: metal.and_then(|i| u16::try_from(i).ok()),
        };
        assert_eq!(
            blocked_text(metal_only, repair, Some(&world)),
            "You need 3 metal scrap for that; you have 0."
        );
        assert_eq!(
            blocked_short(metal_only, repair, Some(&world)),
            "needs 3 metal scrap"
        );
        assert_eq!(
            blocked_text(metal_only, repair, None),
            "You need 3 item for that; you have 0.",
            "without a world, no tag either"
        );
    }

    #[test]
    fn long_item_names_are_cut_to_fit_the_panel() {
        let world = nighthawk_sim::World::new(crate::content::load_start_map().unwrap(), 1);
        let laser = world.item_type_index("basic_laser_cutter").unwrap();
        let short = blocked_short(
            Blocked::Missing {
                item: laser,
                need: 1,
                have: 0,
                tag: None,
            },
            number(&world, "repair"),
            Some(&world),
        );
        assert_eq!(short, "needs 1 basic laser");
    }

    #[test]
    fn repairing_names_what_was_repaired() {
        let world = real_world();
        let crack = world.content().structure_index("hull_crack").unwrap();
        let repair = number(&world, "repair");
        let repaired = Event::Transformed {
            at: Pos::new(23, 0, 1),
            action: repair,
            was: crack,
        };
        let name = &world.structures()[usize::from(crack)].name;
        assert_eq!(
            describe_in(&world, repaired),
            Some(format!("You repair the {name}."))
        );
        assert_eq!(
            describe(Event::Unable {
                action: repair,
                blocked: Blocked::Cluttered,
            })
            .as_deref(),
            Some("Something is lying on it. Move it out of the way first.")
        );
    }
}
