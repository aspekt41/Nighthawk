# Agent tools

Small scripts that help an AI agent (or anyone) work on Nighthawk quickly
and without avoidable mistakes. They use only the Python 3 standard library
and are tested by `agent-tools/test_tools.py`, which `scripts/check.sh` runs.

Use them from the repository root.

## `show.py`: read a file as it really is

Formats Rust files with `cargo fmt` first, then shows matching lines with
line numbers and context, a range of lines, or the whole file. Reading
after formatting means the text you copy into an edit is the text that's
actually there.

```sh
agent-tools/show.py crates/nighthawk-sim/src/world.rs "fn cut" -C 10
agent-tools/show.py crates/nighthawk-sim/src/world.rs "fn cut" -A 30  # 30 lines after (-B: before)
agent-tools/show.py crates/nighthawk-sim/src/world.rs "vec![" -F      # plain text, not a regex
agent-tools/show.py data/README.md --lines 40:80
agent-tools/show.py crates/nighthawk-sim/src/pos.rs                   # whole file (first 300 lines)
```

Use it instead of `cat`, `sed -n` or `grep -n` when you're about to edit
what you read. It exits 1 if the pattern matches nothing.

## `replace.py`: exact edits that survive rustfmt

Reads edit blocks from standard input and applies them to one file, all or
nothing. Each block is an OLD text to find and the NEW text to put in its
place:

```text
<<<< OLD
    fn shut(&mut self, at: Pos) -> Vec<Event> {
==== NEW
    /// Shuts the door at `at`.
    fn shut(&mut self, at: Pos) -> Vec<Event> {
>>>> END
```

Pass the blocks on standard input, for example with a quoted heredoc
(quoting the word, as in `<<'EDITS'`, stops the shell changing `$` or
backticks inside), using any end word that doesn't appear in the edits:

```sh
agent-tools/replace.py crates/nighthawk-sim/src/world.rs <<'EDITS'
<<<< OLD
...
==== NEW
...
>>>> END
EDITS
```

- Formats Rust files before matching and after editing.
- Matches ignoring whitespace and line breaks, and in Rust files a comma
  just before `)`, `]` or `}`: the changes rustfmt makes when it reflows
  code. So an OLD written on one line still matches code rustfmt split over
  several.
- Every OLD must match exactly one place. If any edit misses or matches
  more than once, nothing is written, and the error gives the line of the
  closest text or every matching line.
- A match must start and end at word edges: `Action::Cut` doesn't match
  inside `RecipeAction::Cut`, nor `cut` inside `cutter`. (A rename with
  `--all` once turned `RecipeAction::Cut` into `RecipeCUT`.) If the only
  matches are inside longer words, the error says so.
- If OLD ends in a comma, the file's comma after the match is replaced
  too, so NEW's own comma doesn't make two.
- If the edited file no longer parses (rustfmt fails), it's put back as it
  was and the tool exits 1, so a bad edit never leaves broken code behind.
  `--allow-broken` keeps it anyway, for a change made in several steps.
- `--dry-run` checks the edits without changing anything.
- A line holding only `...` in OLD stands for everything between: the text
  before it must match exactly one place, and the match runs on to the
  first place after that where the text after it matches. So a whole
  function can be replaced by giving its first line, `...`, and its last
  lines, without copying the middle. Make the text after `...` distinctive
  (not just `}`), since the first match after the head is used.
- `--all` lets each OLD match any number of places (at least one) and
  replaces them all: for renames, like `catalogue.` → `content.`. Make
  OLD distinctive, since every match is replaced.

Use it for every edit to an existing file instead of ad-hoc Python or `sed`
replacements. (`sed` with patterns is risky anyway: in a regex `.` matches
anything, and `.` is also the floor glyph in maps.) To insert, put the
anchor line in OLD and the anchor plus the new lines in NEW; to delete,
leave NEW empty. New files are still best written whole.

## `bless.py`: update a snapshot on purpose

Some tests compare output with a stored copy, like the save-format test,
which checks a tiny save against `const EXPECTED_SAVE: &str = r#"..."#` in
`crates/nighthawk-sim/src/save.rs`. When the output changes on purpose
(the save layout changed, and `SAVE_VERSION` was bumped), this runs the
test, takes the actual text from its `left:` line, writes it into the
constant, and runs the test again to check it passes.

```sh
agent-tools/bless.py the_save_format_is_stable crates/nighthawk-sim/src/save.rs EXPECTED_SAVE
agent-tools/bless.py ... -p nighthawk-sim    # test one crate, if another doesn't build yet
agent-tools/bless.py ... --dry-run           # print the new text, change nothing
```

- The test must be `assert_eq!(actual, CONST)`, with the actual value first.
- If the tests don't compile, it says so and lists the errors, instead of
  failing to find a `left:` line.
- Blessing says "the new output is right", so read the diff before
  committing, as with any snapshot.

Use it instead of copying a long expected value out of test output by hand.

## `play.py`: play the game and see the screen, in one command

Builds the game, starts it in its own session on the private `nhtest` tmux
socket (never anyone else's tmux, never `kill-server`), sends keys, waits
for the screen to stop changing, prints it, and kills only its own
session. Saves always go to a fresh scratch folder, so the real save slot
is never touched.

```sh
agent-tools/play.py 'llk.'                         # type keys, show the final screen
agent-tools/play.py '{k*5}{l*23}>' '{show}' jjj    # repeat keys; snapshot midway
agent-tools/play.py 'V{h*4}' '{Esc}' l             # named keys: {Esc} {Enter} {Up} ...
agent-tools/play.py 'lelg{h*3}ehhx' --grep 'cut through|Pos'   # only the lines you need
agent-tools/play.py lll --lines 19:23              # only screen rows 19 to 23
agent-tools/play.py lll --twice                    # same keys twice: identical screens?
agent-tools/play.py --seed 7 --size 100x30 ...     # another ship or terminal size
agent-tools/play.py --data /tmp/bad                # point at a bad data folder
agent-tools/play.py --no-seed n                    # start on the title menu, press n
```

- Named keys are sent on their own, with the screen settling either side,
  so `{Esc}` can't merge with the next key into Alt-key.
- Waiting is by watching the screen, not fixed sleeps.
- If the game exits (a load error, say), the screen shows its message and
  `[game exited: N]`, and the tool exits 1.
- `--save-dir DIR` keeps saves somewhere you choose, to test Continue
  across runs.

Use it whenever you need to see the game, instead of hand-written tmux
commands.
