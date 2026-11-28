# Nighthawk

A terminal-native survival and base-building game where the systems run deep. It follows
the Dwarf Fortress / Cataclysm: DDA tradition and is built around the explore → find →
craft → build loop of Subnautica, Abiotic Factor and Minecraft.

**Status:** M2, items and crafting, steps M2.1 to M2.9, and the first three steps of M2.10, the generator rescue: you wake with emergency power off. The power readout by your chamber says the generator's main fuse has blown, and the passage to it, beyond an airlock, is venting through a hull breach (the air overlay, `a`, draws a venting space in red; look at an airlock door with `V` to read the gauges on each side). The airlock's breach interlock won't let a breach bleed the bay. Take the hull patch kit, hammer and spare fuse from the emergency locker beside the airlock and the screwdriver from your gear locker, patch the breach from inside the airlock, open both of its doors to let the bay's air into the passage, and fit the fuse at the generator: the light on its circuit comes on at once. Or go the quick way: put on the rescue suit from the emergency locker, cycle through the airlock (shut the inner door behind you first), cross the still-venting passage on its oxygen canister (the sidebar counts down the air it has left, and the log warns at half, a quarter and a tenth), fit the fuse and come back, leaving the breach for later. Before that, the stasis bay has your own empty stasis chamber beside where you wake, more along the walls, and bones and torn padding on the floor; the crew deck has bunks, mess tables, chairs and lockers. Furniture can cover several tiles, gets in your way (chairs don't), and lockers hide what's behind them. Lockers are containers: your gear (a laser cutter, a hand lamp and a screwdriver) is in a locker in view of where you wake, and you take it out with `e` then `t`. Any furniture can be smashed (quick, no tool, a random share of its materials) or dismantled with the screwdriver (slower, everything) into scrap of what it was made of: steel, aluminium, glass, polyester, copper. Only metal scrap patches the hull. And you can craft: carrying a kind of item for the first time gives you ideas, so your first scrap teaches you to grind a shiv (a blade, made of whatever scrap you used: steel, glass, even bone), and a shiv cuts fabric scrap into rags. Lockers work both ways (M2.5): `p` on the inventory screen puts what you carry into a locker beside you, and what's in a locker beside you counts when you craft or repair, tools and all, so you needn't carry everything. And power (M2.6): the laser cutter and the hand lamp run on power cells, CDDA-style. The cutter draws a kilowatt while it cuts (a door takes 120 kJ of a cell's 300), the lamp 20 W while it's switched on, and a cell that's too low won't do the job. On the inventory screen, `a` switches the lamp on or off, `r` loads the fullest cell you carry and `u` takes one out. Spare cells are scarce, but your own stasis chamber has a charging port: leave a cell, or the cutter, in it and it slowly fills. And hand tools (M2.7): rags twist into cordage, cordage and two scraps make a makeshift hammer, and the hammer beats scrap into metal plate and soft metal into wire. Plate becomes a pry bar, which levers open the jammed cabin door upstairs; a wrench, which unbolts bunks and lockers; or, with wire and a rag, a hull patch kit, the only thing that now seals a breach. And clothes (M2.8a): you wake in a jumpsuit and deck shoes, with heavier work boots (a sheath in one holds a blade) in a crew-deck locker, and you can sew a sling pack from fabric scrap and cordage that carries 8 L, more than all your pockets. `W` opens the wear screen: what you wear and what you could put on, from your pockets, the floor or a locker beside you; one thing per slot, so the boots mean taking off the shoes. The inventory lists what you wear as well as what you carry: `e` takes a closer look at the highlighted thing (everything about it, and only the keys that apply to it), and `i` moves it to another pocket. Before it, M1 (the soak test and performance budget wait for M5, D51), which ended with M1.8, field of view and light: you see only what's in your line of sight, and everything you've seen stays on the map, dimmed, as you last saw it. The ship is dark: a few emergency lights still glow, and away from them you see only what's right around you, unless you pick up the hand lamp lying near where you wake. Before it, M1.7d and a refactor so that everything in the game is data: new structures, tools, actions and recipes need only JSON (see [`data/README.md`](data/README.md#how-to-add-things)). A title menu, save and load, a clock in real seconds, air (O2, N2 and CO2) that evens out across every connected space each second, hull breaches that vent it to space, heat that moves with the air and leaks out through the hull, breath and warmth meters that run down in thin or cold air, structures (bulkheads, doors, hull plating) made of materials with real properties, items to pick up and carry in your jumpsuit's four pockets, a laser cutter that cuts through aluminium doors, scrap to patch hull cracks and mend cut doors with (look at any tile with `V`, act on one with `e`), and two decks to explore, the Stasis bay (with a small side room behind a shut door) and the Crew quarters above it (with doors to open and close), joined by a ladder (loaded from [`data/maps/aft_ring.json`](data/maps/aft_ring.json)), while the ship creaks around you. Every game comes from a seed, so the same seed and moves replay exactly. See [Running it](#running-it).

## Documents

### Research
- [01 — Genre landscape](docs/research/01-genre-landscape.md): what the reference games do well, what to take from each, and the gap Nighthawk fills
- [02 — Progression loop design](docs/research/02-progression-loop-design.md): how the explore/craft/build loop works, gating types, making the base matter, which simulation layers to build
- [03 — Terminal tech stack](docs/research/03-terminal-tech-stack.md): language and framework comparison, architecture, library versions as of Nov 2026, the terminal as a platform

### Scoping
- [01 — Game concepts](docs/scoping/01-game-concepts.md): four candidate concepts, a scored comparison, and the recommendation
- [02 — Scope and roadmap](docs/scoping/02-scope-and-roadmap.md): vision, pillars, non-goals, the MVP vertical slice, milestones and risks
- [03 — Decision log](docs/scoping/03-decisions.md): every decision made so far, with the reasoning

## Current direction (decided)

- **Concept:** *Nighthawk*. You wake alone on a drifting, dead starship and take it back one sealed, powered and pressurised room at a time. Base expansion and world progression are the same mechanic.
- **Stack:** Rust, with ratatui + crossterm for the terminal UI. The simulation core is a deterministic library crate with no I/O, and it can also target the browser (Ratzilla/WASM) and SSH later.
- **Play model:** a single avatar, turn-based: each action takes a set number of seconds (a step 3 s, a ladder 20 s), and the world is simulated one second at a time on 1 m tiles. Air, heat and power are simulated per tile.

## Reviewing

The same documents are also published as a commentable Claude doc, *Nighthawk: Research & Scoping*, with one tab per file. Feedback can be left there as comments. The markdown files in this repo stay the source of truth, and any changes made through review are copied back here.

## Running it

You need [rustup](https://rustup.rs). The Rust version is pinned in `rust-toolchain.toml` and installs itself on first use. Full setup, including Debian packages and tmux settings, is in [TOOLING.md](TOOLING.md).

```sh
cargo run -p nighthawk-tui                  # opens the title menu
cargo run -p nighthawk-tui -- --seed 42     # skip the menu and play a specific ship
cargo test                                  # run all tests
```

**Title menu:** `j`/`k` or arrows to move, Enter to choose, or `n` (New game),
`c` (Continue), `q` (Quit). Continue is greyed out until you've saved.

**In a game:** arrows, `hjkl` + `yubn` (vi keys) or the numpad to move; `.` or `5`
to wait; `>` to climb a ladder up and `<` to climb down (ladders are drawn with
the same characters); walk into a door to open it,
`c` then a direction to close one; `a` to show or hide the air overlay and `t` the
heat overlay; `V` to look at any tile (what it is, what it's made of, what cuts it; out of sight, only what you remember; Esc to stop); `e` to interact: move the cursor onto a tile and press the key of an action (`o` open, `c` close, `x` cut, `r` repair, `g` pick up, `t` take out of a locker, `s` smash furniture, `d` dismantle it with a screwdriver or wrench, `p` pry open a jammed door; Enter for the first that's possible), and anything out of reach or needing a tool or scrap says why; `i` for your inventory (`j`/`k` to choose, `e` to look closer at it, `i` to move it to another pocket, `d` to drop, `p` to put it in a locker beside you, asking which way if there are several, `W` to put it on or take it off, `a` to switch a lamp on or off, `r` to load a cell, `u` to take one out, Esc or `q` to close); `C` to craft (everything you know how to make, `j`/`k` to choose, Enter to make it, Esc to close); `W` to wear (what you have on and what you could put on, Enter to take off or put on, Esc to close); `S` to save; `q` to save and return to the menu; `Q` to return without
saving; Ctrl-C to save and exit.

**Staying alive:** the sidebar's **Breath** meter refills in good air and
runs down when oxygen is thin (in vacuum it lasts about 36 seconds). Hull
damage (`x` cracks, `X` breaches) vents air to space; a closed door keeps a
leak to its own room. The **Warmth** meter refills in air of 10 °C or more
and runs down in colder air (at −20 °C it lasts about an hour). The crew
quarters are far below freezing, and cold air spreads out of a room you
open, so close doors behind you. If either meter runs out you black out:
a box offers to wake up or quit. Waking up puts you beside your own stasis
chamber half an hour later, with everything you had; while you stand beside
it, its emergency air and heat keep you breathing and warm, however bad the
air around it. Quitting saves the game, blackout and all. If your chamber is
gone (you can take it apart), nothing can pull you back.

The game saves to one slot: `~/.local/share/nighthawk/save.json` on Linux,
`~/Library/Application Support/nighthawk/save.json` on macOS, or the folder in
`$NIGHTHAWK_SAVE_DIR`.
Time only passes when you act, so it's safe to leave running in tmux. The seed is shown in the sidebar and in the first log line.

Recommended `~/.tmux.conf` lines for the primary setup (macOS terminal → SSH → Debian → tmux):

```tmux
set -sg escape-time 10                              # Esc responds immediately
set -as terminal-features ',xterm-256color:RGB'     # truecolor passthrough
set -as terminal-features ',xterm*:sync'            # synchronized output (tmux 3.4+)
```

## Quality checks

One command runs every check (formatting, lints, tests, docs, coverage of at
least 90%, and cyclomatic complexity of at most 10) and stops at the first failure:

```sh
scripts/check.sh
```

How to install and use each tool is in [TOOLING.md](TOOLING.md).

## Contributing

- [CONTRIBUTING.md](CONTRIBUTING.md): the rules, workflow and definition of done, for everyone
- [TOOLING.md](TOOLING.md): installing and using cargo, rustfmt, clippy, tests, coverage and complexity tools
- [AGENTS.md](AGENTS.md): instructions for AI coding agents ([CLAUDE.md](CLAUDE.md) imports it)

## Layout

```
crates/
  nighthawk-sim/   the game: world state, rules, no I/O (deterministic, testable)
  nighthawk-tui/   the terminal front-end: input, drawing, message log, file loading
data/              game content as JSON, all of it: structures, materials, items,
                   tool qualities, actions and recipes (see data/README.md, which
                   has a how-to; data/schema/ lets editors check the files)
docs/              research, scoping and the decision log
scripts/check.sh   every quality check in one command
agent-tools/       helper scripts for AI agents (see agent-tools/tool-descriptions.md)
rust-toolchain.toml  the pinned Rust version
```
