# Research 01 — Genre Landscape

*Last researched: 2026-11-26*

This document surveys the games that define the space Nighthawk wants to live in.
For each one it asks: **what does it do that is worth stealing, and what should we avoid?**

There are two lineages here, and Nighthawk sits where they meet:

1. **Terminal and ASCII simulation games** (Dwarf Fortress, Cataclysm: DDA, Caves of Qud, Cogmind). The draw is the depth of the systems. The UI is plain on purpose, and players like that.
2. **Explore → gather → craft → build survival games** (Subnautica, Abiotic Factor, Minecraft, plus colony and base sims like RimWorld and Oxygen Not Included). The draw is a progression loop in which your base makes you more capable.

---

## 1. The terminal / simulation lineage

### Dwarf Fortress (Bay 12 Games; Steam edition published by Kitfox)

**State in 2026:** Still in active development, 20+ years in. The Steam edition passed
1M copies sold on Steam by April 2025. 2025–2026 brought the Siege update and a
dinosaur/extinct-creatures update. The first "Myth & Magic" update, which adds
procedurally generated magic systems tied to each world's cosmology, is announced
for late 2026 with no firm date. Classic builds were at 53.14 (May 2026), and press
reports 53.16 in August 2026.

**What to steal**
- **Materials are data with physical properties.** Every item is made of a material that has a density, melting and boiling points, a fracture strength and more. Combat, construction, fire and magma all read those same numbers, which is why the game's consequences combine in so many ways. This is the most important lesson from DF.
- **Raws.** Creatures, materials, plants and reactions are plain text data files. That made modding and community knowledge possible.
- **Z-levels and fluids.** Digging *down* is the most natural form of progression in DF. Water and magma flow, so they create hazards and engineering problems that nobody scripted.
- **Emergent narrative.** The game logs what happened, and players turn those logs into stories.

**What to avoid**
- **Illegibility.** For 15 years DF's biggest barrier was its UI. The Steam edition sold so well partly because it was more readable. Depth only matters if players can see it.
- **Unbounded scope.** DF is a lifetime project. Nighthawk needs a finishable core.

### Cataclysm: Dark Days Ahead (CleverRaven, open source)

**State in 2026:** The latest stable release is **0.H "Herbert"**. A 0.I stable-candidate
branch existed in mid-2025, and experimental builds were still shipping as of August 2026.
The Bright Nights fork releases on its own schedule, with releases through 2026.

CDDA is the closest existing game to "terminal Subnautica." It has one avatar,
survival needs, looting, crafting, building, and vehicles that work as mobile bases.

**What to steal**
- **Crafting by tool *qualities*, not specific tools.** A recipe asks for `CUT 1` and `HAMMER 2`, not "a knife" and "a hammer." Any item with those qualities works. This one abstraction lets content grow combinatorially without having to author every combination.
- **Turn-based with time-costed actions.** Every action costs "moves," and long tasks such as crafting, building or reading take in-game time. This suits a terminal game well. It works for deep simulation, is deterministic, and doesn't care about latency (SSH and browser play both work).
- **Everything in JSON.** Items, recipes, monsters, map pieces and mutations all live in `data/json`, and mods live in `data/mods` with a `modinfo.json`. The community content library is enormous because adding content needs no C++.
- **The "reality bubble."** Only the area around the player is simulated at full fidelity, and the rest of the world is updated cheaply or lazily. This is the standard answer to "how do we simulate a big world."
- **Vehicles as bases.** A base you can move is a great pattern for an exploration game.

**What to avoid**
- **Inventory and crafting tedium.** Hunting for components, and the "which pocket is it in" problem. Depth should come from decisions, not paperwork.
- **Content bloat.** Thousands of items, many of them near-duplicates, make the game harder to learn without adding many meaningful choices.
- **Weak authored progression.** CDDA is a sandbox, and many players drift once they are fed and housed. Subnautica-style games solve this, and CDDA does not.

### Caves of Qud (Freehold Games; published with Kitfox)

**State in 2026:** 1.0 released in December 2024. The Switch port came out in February 2026,
and the **Diaspora** expansion (playable plants and animals, new mutations) enters Early
Access on December 9, 2026.

**What to steal**
- **Everything is an object made of parts.** Qud's engine builds entities from attachable parts, much like a component system, and objects talk to each other through events. A door, a gun, a creature and a wall all use the same machinery, so interactions between them come almost for free.
- **Strong tone and writing.** In a text-heavy game, the prose and item descriptions do the job that art direction does in a 3D game.
- **A curated glyph-and-colour palette.** Qud shows that ASCII can look *good* when colour is used deliberately.

### Cogmind (Grid Sage Games)

**What to steal**
- **The best UI in the genre.** Cogmind shows that a pure terminal-style grid can be stylish, animated and very readable. It does this through careful information hierarchy, contextual highlighting and generous feedback.
- **"You are your inventory."** You build yourself out of salvaged parts. That is a short, legible crafting loop with real trade-offs, and it is relevant to suit and tool modding in Nighthawk.

### Other terminal survival / base-builders

The field is thin. Besides CDDA there is *Robinson* (castaway survival) and a handful of
small itch.io projects. The 2026 "ASCII" hits, such as *ASCII Survivors* (May 2026), are
action games that use text as an art style. They are not systems games.

> **Opportunity:** No terminal game combines Subnautica-style *authored, gated progression*
> with DF/CDDA-style *systemic depth*. CDDA has the depth but not the directed loop.
> Subnautica and Abiotic Factor have the loop but not the depth. That gap is Nighthawk's
> reason to exist.

---

## 2. The explore → craft → build lineage

### Subnautica (Unknown Worlds) and Subnautica 2

**State in 2026:** Subnautica 2 entered Early Access on **May 14, 2026** (Steam, Epic, Xbox/Game Pass)
after a delay and a public leadership dispute with Krafton. Early Access is expected to last 2–3 years.

**What to steal**
- **The environment is the gate.** Depth equals danger, pressure, darkness and oxygen cost. Nothing stops you swimming deeper, but you won't survive it yet. Progression comes from *extending your survivable envelope*: more O2, better depth rating, vehicles.
- **Knowledge as loot.** Scanning fragments unlocks blueprints. Exploration pays you in *capability*, not just materials.
- **The base is life support.** Habitats give O2, storage, fabrication, power and a place to recharge vehicles. A base far from danger is safe. Expanding toward danger, with outposts, is a deliberate choice.
- **A gradient of biomes** with a clear "next place" you can see but not reach yet. Breadcrumbs (signals, wrecks, PDA logs) pull the player forward.

**Translation risk for a terminal game:** Much of Subnautica's power is audiovisual: the
dark water, the sound of a leviathan. A top-down grid can't copy that directly, so dread has
to come from *systems and information*: sensor pings, a failing O2 counter, warnings, and
glyphs half-seen at the edge of your light radius.

### Abiotic Factor (Deep Field Games)

**State in 2026:** 1.0 released in July 2025. The spring 2026 "Cosmic Companions" update added
a chemistry and distillation system, with pets planned. The roadmap runs through 2027 and
includes a quest system, tree farming, official mod support and a paid DLC ("Entropic Break")
with boats and fishing.

**What to steal**
- **A facility made of sectors.** A huge built interior divided into sectors, each with its own hazards and themes, opened by keycards, power restoration, environmental gear or story events. This suits a grid-and-rooms terminal game *perfectly*.
- **Crafting benches as progression nodes.** New benches and tiers unlock new recipe families. Adding a bench to your base is a visible milestone.
- **Mundane things repurposed.** Office chairs, keyboards and vending machines become armour and weapons. Salvage is funny and makes sense, which makes the world feel real.
- **Excursion worlds.** Portal trips bring back exotic resources. It is a good pattern for adding content in self-contained biomes.

### Minecraft

**What to steal**
- **Legible material tiers.** Wood → stone → iron → diamond. Each tier is a tool that unlocks the next material. It is the easiest progression ladder to understand ever designed.
- **Dig down.** Vertical progression, where deeper means rarer and more dangerous, is cheap to build and feels natural. It also maps directly onto terminal z-levels.
- **Automation as the late game.** Redstone and modded tech trees turn "I craft things" into "I build machines that craft things." That later layer is where systems-loving players live for hundreds of hours.

### RimWorld (Ludeon) and Oxygen Not Included (Klei)

These are colony sims rather than avatar games, but they matter for *base depth*.

- **RimWorld:** a *storyteller* AI controls the pacing of threats and events, so drama comes from tension and release, not a fixed difficulty curve. The Odyssey DLC (July 2025) was its biggest expansion by revenue, so the genre's audience is healthy and growing.
- **Oxygen Not Included:** the clearest example of **"the base is a machine."** Gases, liquids and temperature are simulated per tile. Your base is a set of plumbing, power and atmosphere problems, and those problems make up the gameplay. That model sits closest to Nighthawk's systems ambitions.

---

## 3. Synthesis — what Nighthawk should take

| Source | Borrow | Leave behind |
|---|---|---|
| Dwarf Fortress | Material properties drive everything; data-file content; z-levels; fluids | Illegible UI; infinite scope |
| CDDA | Tool qualities; time-costed turns; JSON content; reality bubble; mobile bases | Inventory tedium; item bloat; no directed progression |
| Caves of Qud | Parts/components plus events; strong writing; deliberate colour | — |
| Cogmind | UI clarity and feedback as a core feature | — |
| Subnautica | Environmental gating; knowledge-as-loot; base as life support; visible "next place" | Reliance on audiovisual dread |
| Abiotic Factor | Sector-gated facility; benches as milestones; salvage humour; excursions | — |
| Minecraft | Tiered materials; dig down; automation late game | Infinite world (out of scope) |
| ONI / RimWorld | Base as a simulated machine; storyteller pacing | Full colony management (later, maybe) |

**One-line takeaway:** *Subnautica's loop, ONI's base, CDDA's interface model, and DF's
material simulation, made readable the way Cogmind is.*

---

## Sources

- Dwarf Fortress: [Bay 12 Games](https://www.bay12games.com/dwarves/) · [PC Gamer: 1M on Steam](https://www.pcgamer.com/games/sim/dwarf-fortress-sells-1-million-copies-on-steam-so-now-bay-12-games-can-afford-that-little-party-it-planned) · [PC Gamer: Myth & Magic update](https://www.pcgamer.com/games/sim/holy-crap-its-happening-dwarf-fortress-gets-its-first-myth-and-magic-update-later-this-year-bringing-each-players-world-its-own-procedural-magic-system/) · [PCGamesN: Dino update](https://www.pcgamesn.com/dwarf-fortress/dino-update) · [GameNGuide: Myth & Magic](https://www.gamenguide.com/articles/108900/20260828/dwarf-fortress-myth-magic-update-adds-procedural-sorcery-enchantments-magical-research.htm) · [Kotaku: Siege update](https://kotaku.com/games/dwarf-fortress-siege-update)
- Cataclysm: DDA: [Releases](https://cataclysmdda.org/releases) · [GitHub releases](https://github.com/CleverRaven/Cataclysm-DDA/releases) · [JSON_INFO](https://docs.cataclysmdda.org/JSON_INFO.html) · [Modding guide](https://docs.cataclysmdda.org/MODDING.html) · [Bright Nights releases](https://newreleases.io/project/github/cataclysmbn/Cataclysm-BN/release/2026-06-14)
- Caves of Qud: [RPGamer: Diaspora announced](https://rpgamer.com/2026/10/caves-of-qud-diaspora-expansion-announced/) · [Nintendo Life: Switch release](https://nintendolife.com/news/2025/11/15-years-in-the-making-caves-of-qud-is-finally-coming-to-nintendo-switch) · [Wiki changelog](https://wiki.cavesofqud.com/wiki/Changelog)
- Subnautica 2: [Unknown Worlds: EA released](https://unknownworlds.com/news/subnautica-2-early-access-released) · [Gematsu: May 14 launch](https://www.gematsu.com/2026/04/subnautica-2-launches-in-early-access-on-may-14) · [GamesRadar](https://www.gamesradar.com/games/survival/subnautica-2-launches-may-14-in-early-access-on-steam-epic-and-xbox-with-krafton-not-listed-as-publisher-after-all-that-legal-drama/)
- Abiotic Factor: [GamingTrend: 2026 roadmap](https://gamingtrend.com/news/abiotic-factor-got-an-updated-roadmap-for-2026/) · [GamingTrend: Cosmic Companions](https://gamingtrend.com/abiotic-factor/the-cosmic-companions-update-is-coming-to-abiotic-factor-this-spring) · [foro3d: roadmap to 2027](https://foro3d.com/en/2026/octubre/abiotic-factor-confirma-su-hoja-de-ruta-hasta-2027-con-mods-y-dlc.html)
- RimWorld: [GameDiscoverCo: Odyssey performance](https://newsletter.gamediscover.co/p/how-rimworlds-latest-dlc-was-its) · [ComicBook: Odyssey Steam charts](https://comicbook.com/gaming/news/rimworld-steam-charts-odyssey-dlc-basically-rimworld-2)
- Terminal survival games: [itch.io ascii + survival](https://itch.io/games/tag-ascii/tag-survival) · [Game Rant: ASCII roguelikes](https://gamerant.com/best-roguelikes-ascii-art/) · [ASCII Survivors (Steam)](https://store.steampowered.com/app/4588220/ASCII_Survivors/)
