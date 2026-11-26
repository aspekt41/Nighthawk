# Research 03: Technology for a Terminal Simulation Game

*Last researched: 2026-11-26. Versions checked against crates.io, PyPI and the Go module proxy on this date.*

---

## 1. Requirements that drive the choice

| # | Requirement | Why |
|---|---|---|
| R1 | **Runs everywhere**. Primary target: Debian, in tmux, over SSH from a macOS terminal (D16). Native Linux and macOS are expected; Windows is best-effort. A browser front-end is possible later. | This is a core part of the appeal |
| R2 | **Simulation performance**: per-tile atmosphere, temperature and power over tens of thousands of tiles, plus hundreds to thousands of entities | "The depth of the systems is the game" |
| R3 | **Determinism**: same seed plus same inputs gives the same world | Debugging, replays, save integrity, automated testing |
| R4 | **Data-driven content**: items, materials, recipes and map pieces live in data files | CDDA and DF show this is what lets content scale, and it enables modding |
| R5 | **Fast iteration** for one or a few developers | We want to actually ship |
| R6 | **Long-lived codebase** | Games like this grow for years |
| R7 | **Single-binary distribution** with no runtime to install | Supports "it just runs" |

---

## 2. Language and framework options

| Option | Current version (Nov 2026) | Strengths | Weaknesses | Verdict |
|---|---|---|---|---|
| **Rust + Ratatui + Crossterm** | ratatui **0.30.2** (Jun 2026), crossterm **0.29.0** | Native speed with no GC pauses. One static binary per platform. Crossterm works on Windows, macOS and Linux. Ratatui is the leading Rust TUI framework, and 0.30 modularised the crates and added `no_std` support. Strong ecosystem for ECS, serialisation, procgen and pathfinding. WASM target via **Ratzilla 0.3.1**. SSH via **russh 0.65**. | Slower to iterate than Go or Python. Steeper learning curve. Compile times. | ✅ **Recommended** |
| **Go + Bubble Tea v2 + Wish** | bubbletea **v2.1.3** (25 Nov 2026); v2 launched Feb 2026 with the new "Cursed Renderer" | Very fast iteration. **Wish turns any Bubble Tea app into an SSH server**, which is the most turnkey "play over SSH" and multiplayer story available (see snakes.run, SSHTron). Single binary. | The Elm-style architecture suits UI more than a heavy sim core. GC pauses (fine for turn-based play). A thinner game ecosystem (ECS, procgen). | 🥈 Runner-up. **Not pursued:** multiplayer was dropped (D3) |
| **C++ (ncurses / FTXUI / custom)** | — | Lineage of DF and CDDA, maximum control and performance | Build systems, cross-platform pain, memory safety, slow solo iteration | ❌ Not worth the cost for a new project |
| **Python + Textual** | textual **8.2.8** | The fastest UI prototyping by far. Beautiful widgets and CSS-like styling. | Too slow for deep per-tile simulation unless the core is written in another language. Distributing a game that needs a Python runtime is awkward. | ❌ for the game; ✅ for quick content and debug tools |
| **C# / .NET (Terminal.Gui, etc.)** | — | Good language, good tooling | Heavier runtime story for "just runs in a terminal." SadConsole renders graphically, not in a real terminal. | ❌ |
| **Zig / Odin** | — | Fast, simple, fun | Immature TUI and game ecosystems. You'd build everything yourself. | ❌ for now |

### Why Rust wins here

- R2, R3 and R7 favour a compiled language with no GC and strong control over memory layout. Dense per-tile grids are the core workload.
- The **sim core can be a pure library crate** that compiles to native and to `wasm32`. The terminal front-end, a browser front-end (Ratzilla) and an SSH front-end (russh) all sit on top of it.
- Ratatui has momentum. Real terminal roguelikes in 2025–2026 are built on ratatui 0.30 + crossterm 0.29 (for example, *Crawlfest*), and older roguelike-specific libraries are effectively unmaintained (see §4).

### When to reconsider and pick Go

*Resolved: Rust was chosen (D2) and multiplayer dropped (D3). Kept for the record.*

If, after the prototype, **"drop into a shared world over SSH"** turns out to be the main
attraction, Go + Bubble Tea + Wish gets there with far less effort. Decide this early
(see [Open Questions](../scoping/02-scope-and-roadmap.md#open-questions)).

---

## 3. Recommended architecture

```
┌──────────────────────────────────────────────────────────────┐
│ nighthawk-sim  (library crate, no I/O, deterministic)       │
│                                                              │
│  World grid (chunked, dense arrays per layer)                │
│    ├─ terrain/tile ids     ├─ atmosphere (O2, CO2, pressure) │
│    ├─ temperature          ├─ power network graph            │
│    └─ light / visibility                                     │
│  Entities (ECS): actors, items, machines, projectiles        │
│  Systems: needs, AI, crafting, construction, flow solvers    │
│  Content DB: loaded from data files (materials, items, …)    │
│  Turn scheduler: action-time / energy system                 │
│  Seeded RNG streams                                          │
└──────────────────────────────▲───────────────────────────────┘
                               │ commands in, state/events out
        ┌──────────────────────┼──────────────────────────┐
        │                      │                          │
┌───────┴────────┐   ┌─────────┴────────┐    ┌────────────┴───────┐
│ nighthawk-tui  │   │ nighthawk-web    │    │ nighthawk-ssh      │
│ ratatui +      │   │ ratzilla (wasm)  │    │ russh + tui        │
│ crossterm      │   │ (later)          │    │ (later / maybe)    │
└────────────────┘   └──────────────────┘    └────────────────────┘
```

### Key architectural decisions

1. **Sim and UI separation is non-negotiable.** The sim takes `Command`s (move, craft, build…) and emits `Event`s and state. It never touches the terminal. This gives us headless tests, multiple front-ends and replays for free. Commands and events are plain serialisable data, so a future web front-end could stream them over WebSockets or HTTP (D18).

2. **Turn-based with action costs (an "energy" scheduler).** Each actor gains time and spends it on actions of different cost, as in CDDA's move-point model. Environmental systems (atmosphere, temperature, power) run on a fixed tick. A "turn" can be one second of game time, and long actions advance many ticks.

3. **Grid data does *not* live in the ECS.** Atmosphere, temperature, tiles and light are stored as **dense struct-of-arrays per chunk** (for example a 32×32 chunk with one `Vec<f32>` per gas). Flow solvers are tight loops over contiguous memory. The ECS holds *things*: creatures, items, machines.

4. **Reality bubble plus lazy catch-up.** Simulate the chunks near the player at full fidelity. Distant chunks are frozen and "caught up" approximately when they're loaded again, or they run at low frequency. This keeps the cost bounded however large the world gets.

5. **Deterministic RNG.** Use seeded PCG (`rand_pcg` **0.10.3**) with separate streams per subsystem (worldgen, combat, loot), so adding a dice roll in one system doesn't change every other outcome.

6. **Content is data, loaded at startup and hot-reloaded in dev builds.** Materials → items → recipes → buildables → map pieces. Recipes ask for **qualities and material classes** (the CDDA pattern), not specific item IDs.

---

## 4. Libraries (verified Nov 2026)

| Purpose | Choice | Version / last update | Notes |
|---|---|---|---|
| Terminal I/O | **crossterm** | 0.29.0 (Apr 2025) | Cross-platform raw mode, events, synchronized updates, keyboard-enhancement flags. Stable and mature. |
| TUI layout and widgets | **ratatui** | 0.30.2 (Jun 2026) | Buffer-diffing renderer. Use widgets for panels, menus and logs. Draw the map viewport as a **custom widget** that writes cells directly into the buffer. |
| ECS (**deferred**, D19: plain structs until entity counts force a choice) | **bevy_ecs** (standalone) | 0.20.1 (Oct 2026) | Schedules, system ordering, change detection, events, large community. Breaking changes about every 3–4 months, so **pin versions and upgrade deliberately**. |
| ECS (alternative) | **hecs** | 0.11.2 (Oct 2026) | Minimal and stable. You'd write your own scheduling. A good fallback if bevy_ecs churn starts to hurt. |
| ECS (alternative) | shipyard | 0.11.5 (Jul 2026) | Viable, with a smaller community. |
| Serialisation | **serde** | 1.0.229 | Universal. |
| Content format | **JSON** (decided, D17) via serde_json | serde_json 1.0.154 | Everyone knows it. Strict loading with `deny_unknown_fields` turns typos into load errors. |
| Content schema | **schemars** | 1.2.3 (Oct 2026) | Generates a JSON Schema from the Rust types, for editor autocompletion and validation |
| Save files | **postcard** + zstd | postcard 1.2.0 (Nov 2026) | Compact binary. Put a version on the save schema from the first save. |
| Scripting (deferred) | **rhai** or **mlua** | rhai 1.27.0, mlua 0.12.3 | Rhai is pure Rust, sandboxed and WASM-friendly. mlua (Lua/Luau) is faster and familiar to modders but has a C dependency, which makes WASM harder. **Don't add scripting until data-only content clearly hits its limits.** |
| Pathfinding | **pathfinding** | 4.16.0 (Sep 2026) | A\*, Dijkstra, BFS. Also plan on Dijkstra maps (a roguelike staple) written in-house. |
| Noise / procgen | noise / fastnoise-lite | 0.9.0 / 1.1.1 | Both stable and slow-moving, which is fine. |
| Release packaging | **cargo-dist** | 0.34.0 (Nov 2026) | Builds GitHub release binaries and installers for every platform. |
| Browser build | **ratzilla** | 0.3.1 (Jun 2026) | Ratatui on WASM with DOM, Canvas and WebGL2 backends. Ships a Pong example. Prove it out early with a spike. |
| SSH server | **russh** | 0.65.4 (Nov 2026) | Actively maintained. More hand-wiring than Go's Wish. |
| Snapshot testing | ratatui `TestBackend` + insta | — | Render into a buffer and snapshot it. Gives UI regression tests without a real terminal. |

**Avoid (stale):** `bracket-lib`/`rltk` (last release Oct 2022), `legion` (last release 2021),
`specs` (2023), `tcod` bindings (2019), `doryen-rs` (2022). They were great tutorials in
their day, but they aren't foundations for a multi-year project in 2026.

---

## 5. The terminal as a platform

### Colour
- **Truecolor (24-bit)** is effectively universal in modern terminals (Ghostty, kitty, WezTerm, iTerm2, Windows Terminal, GNOME/VTE, Alacritty, foot).
- Still **fall back to 256 colours** (check `COLORTERM`, offer a setting), because some SSH chains and multiplexers degrade colour. tmux passes truecolor only when told the outer terminal supports it (see the tmux section).
- Design the palette so meaning survives in 16 colours, and never rely on colour *alone*. Glyph shape has to carry meaning too, for accessibility and colour blindness.

### Glyphs
- **Safe:** printable ASCII, box drawing (`─│┌┐└┘├┤┬┴┼` and double-line versions), block elements (`░▒▓█▀▄`), and a hand-picked set of Latin-1 and math symbols.
- **Avoid:** emoji, East Asian wide characters, and anything with *ambiguous width*. Terminals disagree on how wide these are and the grid breaks. Keep everything single-width.
- Keep the glyph vocabulary in **one data-driven table**, so a tileset or alternative glyph scheme can be swapped in later.

### Rendering
- Ratatui diffs frames, so only changed cells are written.
- Wrap each frame in **synchronized output** (DEC mode 2026, `BeginSynchronizedUpdate`/`EndSynchronizedUpdate` in crossterm). This prevents tearing on fast terminals. Ghostty documents it and other modern terminals support it, and terminals without support ignore it safely.
- Turn-based play means frames are redrawn only on input or animation. CPU use stays near zero when idle.

### Input
- Design for **keyboard first**, with a fully remappable keymap from day one.
- Modern terminals (kitty, Ghostty, WezTerm, foot and others) support the **kitty keyboard protocol**, which gives clean modifier handling and key-release events. Enable it with crossterm's keyboard-enhancement flags as a *progressive enhancement*. Never require it, because older terminals, many SSH paths and tmux in its default setup don't pass it through. **The default keymap must work with plain keys.**
- **Mouse** as an optional extra (click to look, scroll the log). Useful in a browser build.

### Screen size
- **Minimum 80×24** (play still works, with some panels collapsed); **target 120×40+**.
- Use a responsive layout: map viewport, sidebar and log reflow with the terminal size.

### tmux over SSH (the primary target, D16)
- **Test inside tmux every time**, not only in a bare terminal.
- **Escape delay:** by default tmux waits before passing a lone `Esc` through, which makes Esc-to-cancel feel laggy. Recommend `set -sg escape-time 10` in `~/.tmux.conf`.
- **Truecolor:** add `set -as terminal-features ',xterm-256color:RGB'` (with the outer terminal's TERM). Without it, tmux may downgrade to 256 colours, so the palette must still read well there.
- **Synchronized output:** tmux 3.4+ can forward mode 2026 when configured (`set -as terminal-features ',xterm*:sync'`). Without it, frames still draw correctly; redraws are just less atomic.
- **Resizing:** tmux pane and window resizes arrive as resize events, so the layout must reflow at any time.
- **Mouse:** only works with `set -g mouse on`, which is another reason to keep it optional.

### Windows
- Best-effort. crossterm handles the Windows console, and Windows Terminal should work.

---

## 6. Testing strategy (start on day one)

| Layer | Test |
|---|---|
| Sim determinism | Same seed plus same command script gives byte-identical state hashes after N turns |
| Soak | Run the sim headless for 100k turns with random-walk inputs and check for no panics and conserved quantities (gas mass, energy) within tolerance |
| Systems | Unit tests for flow solvers, power network solving and recipe resolution |
| Content | Load every data file at test time: references resolve, there are no orphan recipes, and every item is obtainable |
| UI | Snapshot tests of rendered screens with `TestBackend` |

---

## 7. Technical risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Atmosphere and fluid solvers are slow or unstable | Medium | Start with simple diffusion over fixed ticks on dense arrays. Profile early. Limit full-rate simulation to the reality bubble. |
| bevy_ecs API churn | High (by design) | Pin versions and upgrade per release in a dedicated PR. Keep ECS usage behind our own thin module. hecs is the fallback. |
| Terminal inconsistency | Medium | Conservative glyph set, capability detection, a 256-colour fallback, and a test matrix of terminals |
| WASM/browser surprises | Medium | Keep the sim crate free of std I/O, threads and the system clock. Run a Ratzilla spike in M0. |
| Scope creep from "systems depth" | **High** | Strict MVP (see the scoping docs). Add a system only when it serves the loop. |

---

## Sources

- Ratatui: [v0.30 highlights](https://ratatui.rs/highlights/v030/) · [GitHub releases](https://github.com/ratatui-org/ratatui/releases)
- Ratzilla: [docs.rs](https://docs.rs/crate/ratzilla/0.3.1) · [lib.rs](https://lib.rs/crates/ratzilla)
- Bubble Tea / Charm v2: [Charm v2 announcement](https://charm.land/blog/v2/) · [Bubble Tea releases](https://github.com/charmbracelet/bubbletea/releases) · [Wish v2](https://newreleases.io/project/github/charmbracelet/wish/release/v2.0.0) · [How snakes.run works](https://eieio.games/how-snake) · [Terminal Trove SSH apps](https://terminaltrove.com/categories/ssh-apps/)
- Bevy ECS: [Bevy 0.20](https://bevy.org/news/bevy-0-20/) · [Bevy 0.19](https://bevy.org/news/bevy-0-19/) · [bevy_ecs docs](https://docs.rs/bevy_ecs)
- Textual: [PyPI](https://pypi.org/project/textual/8.2.6/) · [Debian tracker](https://tracker.debian.org/textual)
- Terminal protocols: [Ghostty: synchronized output](https://ghostty.org/docs/help/synchronized-output) · [Ghostty features](https://ghostty.org/docs/features) · [Synchronized output spec](https://github.com/contour-terminal/vt-extensions/blob/master/synchronized-output.md) · [kitty keyboard protocol](https://sources.debian.org/src/kitty/0.46.1-1/docs/keyboard-protocol.rst)
- Example Rust terminal roguelikes: [Crawlfest](https://blog.hofstede.it/crawlfest-building-a-terminal-roguelike-in-rust/) · [rlddtcrt (ratatui + hecs)](https://codeberg.org/hiecaq/rlddtcrt)
- Version data: crates.io API, PyPI JSON API and proxy.golang.org, queried 2026-11-26.
