# Research 02: The Progression Loop

*Last researched: 2026-11-26*

This document takes apart the loop you see in Subnautica, Abiotic Factor and Minecraft.
It asks how that loop works mechanically, and what changes when it has to run in a
deep simulation that is drawn in a terminal.

---

## 1. The loop

```
        ┌──────────────────────────────────────────────────────┐
        │                                                      │
        ▼                                                      │
   EXPLORE  ──►  FIND  ──►  CRAFT / BUILD  ──►  CAPABILITY  ───┘
   (new area)   materials,  items, tools,       survive longer,
                blueprints, base modules,       reach further,
                knowledge   infrastructure      process more
```

Each step needs a hook:

| Step | The player's question | What has to be true |
|---|---|---|
| **Explore** | "What's over there?" | The next area can be seen or hinted at before the player can reach it: a signal, a sealed door, a deeper level. |
| **Find** | "What did I get?" | Loot includes *knowledge* (blueprints, schematics, logs) as well as raw materials. Rare finds feel rare. |
| **Craft / Build** | "What can I make now?" | New recipes open clearly visible *new options*, not just +5% stats. |
| **Capability** | "Where can I go now?" | The new capability is the key to the area the player saw but couldn't reach. **This closes the loop.** |

If the capability step doesn't open a *new place* or a *new way of acting*, the loop
becomes a grind. Most failed survival-crafting games break at exactly this step.

---

## 2. Gating: what keeps the next area closed

How the gate is built decides how the game feels. These are the main types, ordered from
most systemic to most arbitrary:

| Gate type | Example | Systemic? | Notes |
|---|---|---|---|
| **Environmental envelope** | O2, pressure/depth, cold, heat, radiation, toxic air, darkness | ★★★ | The best kind. It is a *soft* gate: you can go in, but you can't stay. It falls straight out of the simulation. |
| **Logistics / range** | Carry weight, travel time, battery capacity, the need for outposts | ★★★ | Gives a reason to build *forward bases* and vehicles. |
| **Infrastructure** | "This fabricator needs 5 kW," "this reactor needs coolant" | ★★☆ | Makes the base itself the gate. Strong fit for "the base is a machine." |
| **Tool / material tier** | You can't cut hull plate without a plasma cutter | ★★☆ | Minecraft-style. Easy to read, but it can feel like a list of keys unless the tier has a physical reason (hardness and melting points). |
| **Knowledge** | You need the blueprint, or you must scan or research it | ★☆☆ | Good as *loot*, but don't make it the only gate. Being blocked by "you haven't read the book" feels arbitrary. |
| **Access keys** | Keycards, codes, story flags | ☆☆☆ | Use sparingly, as narrative punctuation. Abiotic Factor mixes keycards with environmental gates, which works. |

**Design principle for Nighthawk:** *Every important gate should be a simulation state, not a
flag.* A vacuum deck is closed because it has no air. If you seal the breach and pump
atmosphere in, it is open. The same rules that make the base work are the ones that keep the
world closed.

---

## 3. The base must matter

In weaker survival games, the base is decoration plus a storage box. In the games worth
copying, the base is **mechanically necessary** and also **a place of risk**.

What a base should do, and why:

1. **Life support.** It is the only place where the environment is survivable for long. The base defines your safe area.
2. **Processing.** Raw materials turn into useful ones there: smelting, refining, chemistry. Stations are physical objects with power and input needs.
3. **Storage and logistics.** It holds what you can't carry. Later, it moves items by itself.
4. **Power.** It generates and distributes power. Power is a shared budget that forces trade-offs.
5. **Restoration and recovery.** Medical care, rest, repair and recharging.
6. **Projection.** It extends your reach through vehicles, beacons, forward outposts and drones.
7. **It can fail.** Breaches, fires, power loss, contamination and intruders. The base is a living machine that needs maintenance. *This is where emergent stories come from.*

Item 7 is what separates ONI and DF from games where the base is just a menu with walls.

---

## 4. Which simulation layers give the most depth

The aim is depth that the player feels and can act on. Below is a first ranking of candidate
systems by **value to the loop** against **implementation cost**, with a note on how well
each one reads in a terminal.

| System | Loop value | Cost | Reads in terminal? | Recommendation |
|---|---|---|---|---|
| **Materials with properties** (hardness, conductivity, melting point, flammability, density) | Very high: drives tiers, crafting, fire, damage | Medium | Yes (item info, colours) | **Core, from day one** |
| **Atmosphere per tile** (O2, CO2/toxins, pressure) | Very high: environmental gating, breaches, base expansion | Medium-high | Yes (overlay mode, tinted floor) | **Core** |
| **Power network** (generators, storage, consumers, cables) | High: infrastructure gating, base trade-offs | Medium | Yes (overlay, cable glyphs) | **Core** |
| **Temperature per tile** | High: cold and heat zones, fire, cryo, reactors | Medium (shares atmosphere's grid code) | Yes (overlay) | **Core-lite**, add with atmosphere |
| **Fluids** (water, coolant, fuel) | Medium-high: flooding, plumbing, hazards | High | Yes, but tricky | **Phase 2** |
| **Body / health** (needs, injuries by body part) | Medium: survival tension | Low (needs) to high (body parts) | Yes | **Needs early, body parts later** |
| **Life support: water and waste** (drinking water, recycling, waste disposal) | High: closes the ONI-style base loop | Medium (as items) to high (as plumbing) | Yes | **Water in the MVP, waste right after** (see D14) |
| **Radiation** (sources, shielding by material, dose) | High: gates the reactor deck | Medium (shares the grid code) | Yes (overlay, dosimeter) | **Right after the MVP** |
| **Lighting / visibility** | High for atmosphere and dread | Low-medium | Very good, a classic roguelike strength | **Core** |
| **Sound / noise propagation** | Medium: stealth, creature behaviour | Medium | Yes (log, indicators) | Phase 2 |
| **Ecology / creature AI with needs** | Medium-high: a world that is alive | High | Yes | Phase 2–3 |
| **Automation / logistics** | Very high in the late game | High | Yes | Phase 3 |
| **NPC crew / colony management** | High but changes the genre | Very high | Yes (DF proves it) | Phase 3+, or never |

A useful rule: **pick systems that share infrastructure.** Atmosphere, temperature, fluids and
power can all use the same dense per-tile grid and flow solver. One well-built grid engine
supports four gameplay systems.

---

## 5. Pacing: the first hour

A deep game still has to hook players fast. Subnautica's first 30 minutes are a model:
safe shallows, a lifepod, a fabricator, the first scans, and a first glimpse of somewhere
deeper.

A target outline for Nighthawk's opening:

| Time | Beat |
|---|---|
| 0–5 min | Wake up. Learn to move, look and pick up. One immediate need, such as falling O2 or the cold. |
| 5–15 min | Scavenge the starting area. Craft a first tool. Fix one thing (restore emergency power). The base "turns on." |
| 15–30 min | First trip into hostile territory with a hard limit (O2 or temperature). Come back with something new. |
| 30–60 min | Build the first real station. Extend the safe area by sealing and pressurising a room. See the next gate: a sealed bulkhead, an irradiated deck, a signal. |

**Test:** a new player should be able to *say out loud* what they're trying to do next, at
every point in the first hour.

---

## 6. Terminal-specific design constraints

| Constraint | Implication | Mitigation |
|---|---|---|
| One cell per thing, with 1–2 glyphs of visual vocabulary | The world must be readable from glyph, colour and context alone | A strict glyph and colour language; a look/inspect mode; overlays (atmosphere, power, temperature) |
| No ambient audio or visuals for dread | Tension has to come from information | Sensor pings, warning logs, a narrowing light radius, countdowns, unreliable readouts |
| Text is cheap and images aren't | Lore and item descriptions carry the tone | Invest in writing: item flavour text, logs, terminals in the world |
| Menus can become tedious | Crafting and inventory UI decide whether the game is fun | Cogmind-level UI polish: filtered lists, crafting from containers within reach, batch actions, sensible defaults |
| Turn-based input | Action games are out; tactical play is in | Make the turn system expressive: time costs, interruptions, "wait until healed or until something appears" |

---

## 7. Anti-patterns to design against

- **Grinding for counts.** "Collect 40 copper" is boring. Make material *quality and kind* matter more than quantity.
- **Recipe-book bloat.** Prefer fewer recipes with flexible inputs (qualities, material classes) over thousands of fixed recipes.
- **Inventory paperwork.** Bad UI is the problem, not logistics itself. Logistics stays a problem the player solves (decision D12): early crafting uses what's on you and in containers within reach, and networked storage and transport are machines you build later, as in Applied Energistics 2 and Mekanism. Containers should be meaningful (a medical pouch that survives dropping your pack), with smooth UI for moving things between them.
- **Gates with no reason.** If players can't explain why they can't go somewhere, the gate is bad.
- **Being safe at home forever.** The base should need upkeep, and the next important resource should be out there.
- **Hidden depth.** If a system matters, give it an overlay, a readout and a log line. Depth the player can't see doesn't count.

---

## Sources

General design analysis. Factual references for the individual games are in
[01-genre-landscape.md](01-genre-landscape.md). CDDA tool-quality and recipe structure:
[CDDA JSON_INFO](https://docs.cataclysmdda.org/JSON_INFO.html) ·
[CDDA recipe JSON](https://srgnis.github.io/cdda-wiki/cdda_wiki/Recipe_JSON_Info.html).
Abiotic Factor sector and bench progression: [GamingTrend roadmap coverage](https://gamingtrend.com/news/abiotic-factor-got-an-updated-roadmap-for-2026/).
