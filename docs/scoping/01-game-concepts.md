# Scoping 01: Game Concepts

*Drafted: 2026-11-26. All concepts are original premises that work within the explore/craft/build genre.*

Four candidate directions follow. Each is judged against what the research says matters:
a closed **explore → find → craft → capability** loop, **gates that come from the
simulation**, a **base that is a machine**, and a world that **reads well in a terminal**.

Two design choices are shared by every concept, and the research points the same way on both:

- **Avatar game, not colony manager (at first).** Subnautica, Abiotic Factor and Minecraft are all *you, in the world*, so CDDA's control model is the right fit. DF-style management of many agents can come later as an optional layer (for example, drones or rescued crew).
- **Turn-based with action costs.** This suits deep simulation, determinism, SSH and browser play. It's also how CDDA and DF adventure mode work.

---

## Concept A: *Undertow* (flooded ocean colony)

**Fantasy:** A research colony on an ocean world has gone quiet. You're the survivor of a
supply drop, working down through flooded habitats and the open sea below them.

- **Loop:** Scavenge habitats → craft dive gear and pressure hulls → descend to deeper layers → find rarer materials and colony logs.
- **Gating:** Depth, pressure, O2, darkness and cold.
- **Base:** Floating or seabed habitats you pressurise and extend.
- **Signature systems:** Water and pressure, O2, light radius, and creature ecology.
- **Terminal fit:** ★★☆. Vertical depth works as z-levels, but open water is mostly empty grid and the dread of Subnautica's ocean is largely audiovisual. It would sit closest to Subnautica and gain the least from a terminal.
- **Risk:** It would be measured against Subnautica 2, which entered Early Access in May 2026.

## Concept B: *Sublevel* (containment-breach facility)

**Fantasy:** You're a junior technician in a vast underground research complex after
something goes very wrong. Dozens of sectors, each with its own failure and its own
anomalies.

- **Loop:** Clear and loot a sector → repurpose office and lab junk → restore power and doors → open the next sector.
- **Gating:** Power, keycards, hazards (toxic gas, radiation, anomalous zones) and hostile entities.
- **Base:** A reclaimed office block or lab that you power, barricade and fill with benches.
- **Signature systems:** Power, atmosphere and toxins, materials, and creature AI.
- **Terminal fit:** ★★★. Rooms and corridors are what roguelikes do best.
- **Risk:** Very close to Abiotic Factor's premise, and to Half-Life and SCP in general. Little "outdoors" for a sense of exploration.

## Concept C: *Nighthawk* (derelict ship salvage) ⭐ recommended

**Fantasy:** You wake from emergency stasis aboard the **Nighthawk**, a vast long-haul
vessel (a generation ship, a mining hauler, or a station hulk) that has been drifting dark
for an unknown time. Most of it is airless, frozen, flooded, overgrown or irradiated. You
seem to be the only one awake. The ship is the world, and taking it back is the game.

- **Loop:**
  1. *Explore* a deck or section, each one a "biome" defined by **how it failed**.
  2. *Find* salvage, schematics in old fabricator memory, crew logs, and intact machines.
  3. *Craft and build* tools, suit upgrades, patches, cables, pumps, generators and fabricators.
  4. *Capability:* reseal, re-pressurise and re-power sections. **Your base literally grows into the ship.** The new safe area becomes the staging ground for the next deck.

- **Gating: all of it comes from the simulation.**
  | Section state | Why you can't enter yet | What opens it |
  |---|---|---|
  | Vacuum / breached | No air, so your O2 runs out | EVA suit and O2 tanks; later, patching the breach and pressurising |
  | Cryo / frozen | Temperature drains your heat | Insulated suit, heaters, or restoring the heating loop |
  | Flooded (coolant or water) | Drowning, currents, electrical hazard | Pumps, rebreather, isolating the power |
  | Overgrown hydroponics | Toxic spores, hostile flora and fauna | Filters, fire, herbicide chemistry |
  | Reactor / engineering | Radiation, heat, high voltage | Shielding, dosimetry, insulated tools |
  | Locked command decks | Security systems, power required | Power routing, rebuilt access systems |

- **The base is a machine.** Atmosphere is simulated per tile, with doors, seals and breaches. Power runs over a real network with cables, breakers, batteries and loads. Heat and cooling matter. Every room you reclaim is plumbing and power you now have to *maintain*. Breaches, fires, power brown-outs and things coming through the ducts make the stories.

- **Materials depth (DF lesson):** Hull alloy, polymers, glass, organics and exotic materials, each with hardness, conductivity, melting point, flammability and radiation shielding. Tiers come from physics: you need a plasma cutter because hull alloy is hard and melts at high temperature.

- **Breadcrumbs and mystery:** Crew logs and ship terminals tell you *what happened*, which itself is a puzzle about why the ship went dark. Signals and sealed doors are the visible "next places."

- **Late-game layers (post-MVP):** Automation (cargo drones, fabricator queues), rescuing or waking crew (an optional DF-style management layer), restoring propulsion and navigation, and **away missions** to asteroids, wrecks or planets for exotic materials (Abiotic Factor's portal worlds as a pattern).

- **Terminal fit:** ★★★. Decks are z-levels. Corridors, rooms and machines suit glyphs well. Overlays (air, power, heat) are natural "ship systems readouts," so the terminal UI feels like *diegetic ship computer screens*. Being in a terminal becomes part of the fiction.

- **Scope control:** The world is **finite and largely hand-shaped with procedural detail**. Deck layouts are generated from authored room and module templates (CDDA-style map pieces), and the overall deck graph is authored. That's much smaller than an infinite procedural planet.

- **Risks:** Space-derelict settings are familiar (*System Shock*, *Dead Space*, *Barotrauma*, *FTL*), but none of them is this game. The atmosphere and power simulation is the most demanding piece of engineering, and the MVP has to prove it early.

## Concept D: *Frontier* (open procedural planet)

**Fantasy:** A colonist on a procedurally generated wilderness world. Biomes on the
surface, deep cave systems below.

- **Loop:** Classic Minecraft/CDDA. Gather → tier up materials → dig deeper → automate.
- **Gating:** Material tiers, depth, climate.
- **Base:** Anywhere. Built from what you mine.
- **Terminal fit:** ★★☆. It works (CDDA proves that), but open terrain is visually noisy, and the infinite world needs the reality-bubble machinery from day one.
- **Risk:** The largest scope. The weakest directed progression, which is the exact flaw the research found in CDDA. Competes directly with CDDA's own space.

---

## Comparison

Scores are 1 (weak) to 5 (strong). Scope risk is inverted, so 5 means the most manageable.

| Criterion | A: Undertow | B: Sublevel | **C: Nighthawk** | D: Frontier |
|---|---|---|---|---|
| Fits the explore/find/craft/base loop the user loves | 5 | 4 | **5** | 4 |
| Gates come from the simulation | 4 | 3 | **5** | 3 |
| "Base is a machine" depth | 3 | 3 | **5** | 3 |
| Terminal fit and legibility | 2 | 5 | **5** | 3 |
| Distinct from its inspirations | 2 | 2 | **4** | 2 |
| Scope manageability | 3 | 4 | **4** | 1 |
| Room to grow (automation, crew, away missions) | 3 | 3 | **5** | 5 |
| **Total** | 22 | 24 | **33** | 21 |

## Recommendation

**Go with Concept C, *Nighthawk*.** It's the only concept where *the thing you build and
the thing you explore are the same object*. Base expansion and world progression are one
mechanic: reclaiming the ship. That ties the progression loop and the systems depth together
instead of having them pull against each other. It also makes the terminal part of the
fiction, because you are reading the ship's own systems.

Concept B's strengths, sector gating and dense interiors, are already part of C. Concept D's
open exploration can come back later as **away missions**, without making the core an
infinite-world problem.

**Possible variant (decide in pre-production):** *The Nighthawk has crash-landed.* The wreck
is your base, and the hostile planet surface and caves around it are the exploration space.
This gives up some of C's purity (base and world stop being the same object) for outdoor
variety. See [open questions](02-scope-and-roadmap.md#open-questions).
