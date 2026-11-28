//! Wearing things (decision D61), with the real content: one thing in each
//! slot, put on from your pockets, the floor or a locker beside you, and
//! taken off into a pocket or at your feet.

mod common;

use nighthawk_sim::{Blocked, Command, Dir, Dress, Event, ItemAt, Pos, Vertical, World};

/// The ids of what you wear, in order.
fn wearing(world: &World) -> Vec<&str> {
    let types = world.item_types();
    world
        .worn()
        .iter()
        .map(|w| types[usize::from(w.kind)].id.as_str())
        .collect()
}

/// Where the first wearable thing at hand with id `id` is.
fn at_hand(world: &World, id: &str) -> ItemAt {
    let kind = world.item_type_index(id).unwrap();
    let found = world
        .wearables_at_hand()
        .into_iter()
        .find(|(_, i)| i.kind == kind);
    found.unwrap_or_else(|| panic!("no {id} at hand")).0
}

fn dress(world: &mut World, dress: Dress) -> Vec<Event> {
    world.apply(Command::Dress(dress))
}

/// From the start, up the ladder, through the cabin door at (22, 4), to
/// (27, 3), beside the locker with the work boots.
fn walk_to_the_boots(world: &mut World) {
    use Command::{Climb, Move};
    let route = std::iter::repeat_n(Move(Dir::North), 5)
        .chain(std::iter::repeat_n(Move(Dir::East), 23))
        .chain([Climb(Vertical::Up)])
        .chain(std::iter::repeat_n(Move(Dir::South), 3))
        .chain(std::iter::repeat_n(Move(Dir::West), 11))
        .chain(std::iter::repeat_n(Move(Dir::North), 3)) // the first opens the door
        .chain(std::iter::repeat_n(Move(Dir::East), 5));
    for command in route {
        world.apply(command);
    }
    assert_eq!(world.player(), Pos::new(27, 3, 1));
}

#[test]
fn you_trade_your_deck_shoes_for_the_work_boots_in_the_crew_locker() {
    let mut world = common::start_world(1);
    assert_eq!(wearing(&world), ["jumpsuit", "deck_shoes"]);
    walk_to_the_boots(&mut world);
    let boots = at_hand(&world, "work_boots");
    assert!(
        matches!(boots, ItemAt::Inside { .. }),
        "still in the locker"
    );
    let shoes = world.item_type_index("deck_shoes").unwrap();
    assert_eq!(
        world.put_on_blocked(boots),
        Some(Blocked::Wearing { item: shoes }),
        "one thing on your feet"
    );
    dress(&mut world, Dress::TakeOff(1));
    assert_eq!(world.carried_count(shoes), 1, "the shoes fit a pocket");
    dress(&mut world, Dress::PutOn(boots));
    assert_eq!(wearing(&world), ["jumpsuit", "work_boots"]);
    let sheath = &world.item_types()[usize::from(world.worn()[1].kind)];
    assert_eq!(sheath.pockets()[0].name, "boot sheath");
}

/// A storeroom: you stand at `@` on a polyester scrap, wearing a jumpsuit,
/// beside a locker `L` with six more and a shiv.
fn storeroom() -> World {
    let rows = ["HHHH", "HL@H", "HHHH"];
    let json = format!(
        r#"{{ "id": "store", "name": "Store", "player_wears": [ {{ "item": "jumpsuit" }} ],
             "legend": {{ "H": "hull_plating",
                 "@": {{ "structure": "deck_plating",
                         "items": [ {{ "item": "scrap", "material": "polyester" }} ] }},
                 "L": {{ "structure": "deck_plating", "furniture": "locker", "contents": [
                     {{ "item": "scrap", "material": "polyester", "count": 6 }},
                     {{ "item": "shiv" }} ] }} }},
             "decks": [ {{ "name": "Store", "rows": {rows:?} }} ] }}"#
    );
    World::new(common::parse(&json), 1)
}

fn craft(world: &mut World, id: &str) {
    let i = world
        .content()
        .crafts()
        .iter()
        .position(|c| c.id == id)
        .unwrap();
    let number = u16::try_from(i).unwrap();
    assert_eq!(world.craft_blocked(number), None, "{id}");
    world.apply(Command::Craft(number));
}

#[test]
fn a_sling_pack_of_fabric_scrap_and_cordage_carries_eight_litres() {
    let mut world = storeroom();
    let here = world.player();
    let pick_up = world.content().action_index("pick_up").unwrap();
    world.apply(Command::Interact {
        at: here,
        action: pick_up,
    });
    craft(&mut world, "rags");
    craft(&mut world, "cordage");
    craft(&mut world, "sling_pack");
    // At 1.2 L it fits no pocket, so it lands at your feet.
    let pack = at_hand(&world, "sling_pack");
    assert!(matches!(pack, ItemAt::Floor(_)));
    let events = dress(&mut world, Dress::PutOn(pack));
    assert!(events.iter().any(|e| matches!(e, Event::PutOn(_))));
    assert_eq!(wearing(&world), ["jumpsuit", "sling_pack"]);
    let pack_type = &world.item_types()[usize::from(world.worn()[1].kind)];
    assert_eq!(pack_type.wear.as_ref().unwrap().slot, "back");
    assert_eq!(pack_type.pockets()[0].volume_ml, 8000);
}
