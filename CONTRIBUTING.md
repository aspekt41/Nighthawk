# Contributing to Nighthawk

These are the rules and workflow everyone follows: people and AI agents
alike. Agents also read [AGENTS.md](AGENTS.md). For installing and using the
tools, see [TOOLING.md](TOOLING.md).

## What we're building

A turn-based terminal survival game in which you reclaim a dead starship one
sealed room at a time, with the depth in the simulation rather than the
graphics. The background is in [`docs/`](docs/):

| Read this | When |
|---|---|
| [Decision log](docs/scoping/03-decisions.md) | **Before any design or architecture work.** Every decision so far (D1–D65) with its reasoning. |
| [Scope and roadmap](docs/scoping/02-scope-and-roadmap.md) | To find the next step (milestones M0–M6) and what is out of scope |
| [Tech stack research](docs/research/03-terminal-tech-stack.md) | Before adding a library or touching terminal handling |
| [Progression loop](docs/research/02-progression-loop-design.md) | Before designing a game system |

## Architecture rules

These hold everywhere. Breaking one is a design change and needs a new
decision in the log first.

1. **The sim is pure.** `crates/nighthawk-sim` has no terminal, no file I/O
   and no clock. Front-ends read files and pass the *text* in (D18).
2. **The sim is deterministic.** All randomness goes through `GameRng`, on a
   `Stream` of its own, seeded from the world seed. Any new state is added
   to `World::state_hash`. The golden determinism test must pass, or be
   updated on purpose with the reason in the commit message (D22).
   All game state lives in `World`, which is also the save file (D23). If a
   change alters the save layout, bump `SAVE_VERSION`.
3. **Time moves only when the player acts.** `World::apply` is the only thing
   that changes the world or advances the clock (`seconds`). Each action takes
   a set number of seconds, simulated one second at a time (D29). Nothing runs
   in the background (D7).
4. **Everything is 3D.** Positions are always `Pos { x, y, z }`, even when
   there's one deck (D8).
5. **Content is data.** Maps, and later items and recipes, are JSON in `data/`,
   loaded with strict checking: unknown fields are errors (D17).
6. **Plain keys work everywhere.** The default keymap needs no special
   terminal features, so it works in tmux over SSH (D16).
7. **No `unsafe`.** It's forbidden by the workspace lints.

## The workflow

1. **Pick a small step.** Take the next unchecked item in the roadmap's M0–M6
   table. If it's big, split it. M0.1 was "walk an `@` around one room"; M0.2
   was "load that room from JSON".
2. **Write the tests with the code**, not afterwards. Every piece of real
   logic (rules, parsing, validation, the keymap) gets unit tests covering its
   normal case, its edge cases and each way it can fail.
3. **Keep functions small.** No function over 60 lines, 4 levels of nesting,
   or cyclomatic complexity 10. When one grows, split it.
4. **Run `scripts/check.sh`** until it passes. It formats, lints, tests,
   builds docs, measures coverage and measures complexity.
5. **Try it for real.** Run the game (`cargo run -p nighthawk-tui`), ideally
   inside tmux, and check that the change works and feels right.
6. **Update the docs** in the same commit:
   - tick the step in the roadmap (`docs/scoping/02-scope-and-roadmap.md`);
   - add a decision to the log for any new choice about direction, design or tech;
   - update `README.md` if how to run or play changed;
   - update `data/README.md` if a content format changed.
7. **Commit** (see below).

## Definition of done

- [ ] `scripts/check.sh` passes, with no steps skipped
- [ ] CI is green on Linux and macOS
- [ ] New logic has unit tests, including its failure cases
- [ ] The game runs and the change works when played
- [ ] Roadmap, decision log and READMEs match the code
- [ ] No quality gate was loosened (thresholds, `#[allow]`, skipped tests) without a written reason and the owner's agreement

## Code style

- Format with `cargo fmt` and don't argue with it.
- **Comments explain *why*, in plain English, for a reader who is learning
  Rust.** Explain a Rust idiom the first time it appears in a file, for
  example why something is `Copy` or what `?` does there.
- Every module starts with a `//!` comment saying what it's for. Every public
  item has a `///` comment.
- Errors are enums whose variants say exactly what went wrong and where, and
  they implement `Display` with a message a content author can act on.
- Name tests as sentences about behaviour: `bumping_a_wall_costs_no_time`,
  not `test_move_2`.
- Prefer small functions and data tables over long `match` blocks or deep
  nesting.

## Adding content

Content needs no Rust. Maps are described in [data/README.md](data/README.md).
After adding or changing a file, run `cargo test`: the content test loads every
map and reports exactly what's wrong with any that fail.

## Adding a dependency

Check it's maintained, pin the exact current version (`cargo add name@x.y.z`),
and add a row to the libraries table in the tech stack research if it matters
to the architecture. Fewer dependencies is better.

## Commits

- One logical change per commit, with the code, tests and docs together.
- The first line is a short summary in the imperative ("Load the starting room
  from JSON"), under about 72 characters.
- The body says what changed and why, as a short list if there are several parts.
- Never commit `target/`, editor settings or secrets.
