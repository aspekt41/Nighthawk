# Scoping 02: Scope, Pillars and Roadmap

*Drafted: 2026-11-26. Assumes Concept C (Nighthawk, derelict ship salvage). See [01-game-concepts.md](01-game-concepts.md).*

---

## Vision

> A turn-based terminal survival game in which you reclaim a dead starship one sealed room
> at a time. Every bulkhead you open, every cable you run and every compartment you
> pressurise makes the base bigger and moves the frontier back. The ship is simulated down
> to the air in each tile, and it is trying to die.

## Design pillars

1. **The base is a machine.** Air, power and heat are simulated. Your base works because you engineered it, and it fails when you neglect it.
2. **Gates are simulation states, not locks.** You can't go there *because there's no air, it's −80 °C, or it's full of coolant*, and you can see why and plan around it.
3. **Exploration pays in capability.** The best loot is a schematic, a working machine, or a piece of the story that points to the next place.
4. **Legible depth.** If a system matters, it has an overlay, a readout and a log line. Terminal-first readability is a feature.
5. **Runs everywhere.** It's a single binary, works in any modern terminal (tmux over SSH first), and stays playable at 80×24.
6. **Time moves only when you do.** No real-time ticking anywhere. Leave it in tmux for a week and nothing has happened.
7. **The world is 3D.** Decks are z-levels, and air, heat, creatures and you move between them.

## Non-goals for the first playable (MVP)

These are deliberately out of scope, so we can say no to them:

- Multiplayer of any kind. It's dropped from the vision, not just postponed (D3).
- Real-time play. The game is turn-based only.
- A graphical tile mode (keep the glyph table data-driven so this stays open)
- An infinite or procedurally generated world. The deck layout is authored, with procedural room contents.
- NPC crew or colony management (not planned; maybe very late, D6)
- Fluid simulation (water and coolant flow). Atmosphere and temperature come first.
- Scripting and a modding API. Content is data files only.
- Audio
- A full body-part injury model. Use simple health plus needs.

---

## MVP: "The First Deck" vertical slice

**Goal:** prove that the core loop is fun and that the simulation is readable, in about
**1–2 hours of play**.

### Content budget

| Category | Target count | Examples |
|---|---|---|
| Ship sections | 1 starting hab + 3 decks | Stasis bay (start), crew quarters (vacuum), hydroponics (toxic), maintenance (dark and cold) |
| Materials | ~10 | Hull alloy, steel, copper, polymer, glass, fabric, organic matter, ceramic, battery chemicals, an unknown exotic |
| Items | ~40 | Tools, suit parts, O2 canisters, cables, patches, food, components |
| Recipes | ~25 | Using qualities and material classes, CDDA-style |
| Buildables | ~10 | Wall patch, airlock door, cable, battery bank, generator, O2 scrubber, heater, fabricator, workbench, storage locker |
| Hazards | 3 | Vacuum, cold, toxic spores |
| Hostile types | 1–2 | Something in the vents and something in hydroponics |
| Lore pieces | ~15 | Crew logs and terminal entries that build the mystery |

### Systems in the MVP

| System | MVP depth |
|---|---|
| Map | Multi-deck grid (z-levels), chunked, field of view and light radius |
| Atmosphere | Per tile O2, CO2/toxin and pressure; diffusion; doors seal; breaches vent |
| Temperature | Per tile, sharing the atmosphere grid; heaters; heat loss to vacuum |
| Power | Network graph of generators, batteries, cables and consumers; brown-outs |
| Player body | HP, O2, body heat, hunger, thirst, fatigue |
| Items and crafting | Material-based items in physical containers with pockets; quality-based recipes; crafting from your inventory and containers within reach |
| Construction | Place and deconstruct buildables, with time cost and tool qualities |
| Turn system | Energy and action-cost scheduler, interruptible long actions |
| UI | Map, sidebar status, message log, inventory, crafting screen, look mode, overlays (air, power, heat) |
| Saves | Versioned binary saves, autosave |
| Death | Respawn at the stasis pod or base; difficulty decides keep vs drop items (D5) |

### The MVP progression arc

1. Wake in the stasis bay. The **stasis pod is your lifepod** (D15): a trickle of emergency O2 and heat, a basic fabricator, and your respawn point. It's enough to scrape by on. **Restore emergency power** by repairing a generator, and learn to scavenge and craft.
2. **Seal and pressurise** the hab ring: patch the breach, build an airlock, run an O2 scrubber. *The base exists.*
3. **Build a fabricator** and craft the first **EVA suit**, which opens the vacuum section (crew quarters) on a timer.
4. Find the **heater and insulation** schematics in crew quarters, which open the cold maintenance deck and its power junctions.
5. **Reroute power** to the hydroponics deck, deal with the toxic spores (filters or fire), and find the clue that points to the next sealed deck. *End of the slice.*

### Success criteria for the MVP

- A new player can always say what they're trying to do next.
- At least one **unscripted base failure** (a breach, fire or brown-out) happens in a normal playthrough, and it's interesting rather than frustrating.
- Players use the overlays without being told to.
- The sim stays deterministic: the same seed and the same inputs give the same state hash.
- Turns process in under 5 ms in the reality bubble, and the game is playable at 80×24.

---

## Milestones

Each milestone ends with something you can play or run, plus exit criteria. There are no
calendar estimates yet. Re-estimate after M0.

| Milestone | Deliverable | Exit criteria |
|---|---|---|
| **M0: Tech spike** | M0.1 an `@` walking around one room (sim and tui crates, 3D positions, turn counter that only moves when you act) ✅ done. M0.2 the room loaded from JSON ✅ done. M0.3 seeded RNG and a determinism test in CI ✅ done. M0.4 save and load, moved ahead of decks so every later system is saved from the start (D23): M0.4a save/load in the sim ✅ done; M0.4b save files on disk, save on quit ✅ done; M0.4c title menu (New game / Continue / Quit). M0.5 a second deck and stairs. M0.6 a minimal browser build. | Runs in tmux over SSH on Debian and natively on macOS; tests pass in CI |
| **M1: World and simulation grid** | Chunked multi-deck map; FOV and lighting; atmosphere and temperature diffusion on dense arrays; doors and breaches; debug overlays | A breach visibly vents a room; profiled sim stays inside budget; soak test passes 100k turns |
| **M2: Items and crafting** | Materials → items → recipes from data; container-based inventory; quality-based crafting from containers within reach | All ~25 MVP recipes are craftable; a content-validation test passes |
| **M3: Building and power** | Construction system; power network solver; buildables with power draw; brown-outs | You can build a pressurised, powered, heated room from scratch |
| **M4: The first deck** | Three authored decks with procedural room contents; hazards; 1–2 creatures; lore; the progression arc | The full arc can be played start to finish |
| **M5: Legibility pass** | UI polish: look mode, tooltips, a contextual help panel, a log that explains itself, keymap, accessibility settings for colour | Playtests with 3+ fresh players meet the MVP success criteria |
| **M6: Distribution** | cargo-dist release pipeline; browser build hosted; save compatibility policy | Anyone can download and run it, or play it in a browser |

### After the MVP (candidate phases, unordered)

- Fluids (flooding, coolant loops, plumbing)
- Automation (drones, fabricator queues, item transport)
- More decks and the ship-wide mystery
- Away missions (asteroids, wrecks, planets)
- Waking or rescuing crew: the DF-style management layer
- Scripting and mod support
- An SSH server mode, and possibly a shared world

---

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Systems depth balloons the scope | High | Pillars and non-goals above. Every new system has to name the gate or loop step it serves. |
| Atmosphere sim is fun on paper but unreadable in play | High | Overlays and readouts are part of the system's definition of done, not polish. Playtest in M1. |
| Crafting UI tedium (CDDA's problem) | High | Craft from nearby storage, qualities over exact items, batch crafting, and remember the last recipe. |
| Progression feels like a list of keys | Medium | Every gate is a simulation state (pillar 2); content review against the gating table. |
| Engine work never turns into game | Medium | M0–M4 stay focused on one playable slice. No general-purpose engine work. |

---

## Decisions

The six open questions from the first draft were all answered in review on 2026-11-26. They're recorded with the rest of the decisions in [03-decisions.md](03-decisions.md):

| # | Question | Decision |
|---|---|---|
| 1 | Concept | Derelict ship, as laid out (D1) |
| 2 | SSH / multiplayer | Single-player only; Rust (D2, D3) |
| 3 | Tone | Gritty realism; writing can shift the mood (D4) |
| 4 | Content format | JSON, with a schema generated from the Rust types (D17) |
| 5 | Death | Respawn at base; difficulty decides keep vs drop items (D5) |
| 6 | Crew layer | Not planned; maybe very late (D6) |
