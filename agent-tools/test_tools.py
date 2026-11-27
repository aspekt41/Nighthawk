"""Tests for the agent tools. Run with:

    python3 -m unittest discover -s agent-tools -p 'test_*.py'

`scripts/check.sh` runs them too. They never touch the real source: every
file they edit is in a temporary folder, and formatting is switched off.
"""

import io
import os
import shutil
import subprocess
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

import bless
import play
import replace
import show

RUST = """impl World {
    fn shut(&mut self, at: Pos) -> Vec<Event> {
        let events = vec![
            Event::DoorClosed { at },
            Event::Waited,
        ];
        events
    }
}
"""


def run(module, argv, stdin=None):
    """Runs a tool's main() and returns (exit status, stdout, stderr)."""
    out, err = io.StringIO(), io.StringIO()
    with redirect_stdout(out), redirect_stderr(err):
        args = (argv, io.StringIO(stdin)) if stdin is not None else (argv,)
        status = module.main(*args)
    return status, out.getvalue(), err.getvalue()


def edit_block(old, new):
    return f"<<<< OLD\n{old}\n==== NEW\n{new}\n>>>> END\n"


class TempFile(unittest.TestCase):
    """Each test gets a file in a fresh temporary folder."""

    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)

    def file(self, name, text):
        path = Path(self.folder.name) / name
        path.write_text(text)
        return path


class ShowTests(TempFile):
    def test_a_pattern_shows_numbered_lines_with_context(self):
        path = self.file("a.rs", RUST)
        status, out, _ = run(show, [str(path), "DoorClosed", "-C", "1", "--no-format"])
        self.assertEqual(status, 0)
        self.assertEqual(
            out.splitlines(),
            [
                "    3          let events = vec![",
                "    4              Event::DoorClosed { at },",
                "    5              Event::Waited,",
            ],
        )

    def test_nearby_matches_merge_and_distant_ones_are_separated(self):
        text = "\n".join(["hit", "x", "hit", "x", "x", "x", "x", "x", "hit"])
        path = self.file("a.txt", text)
        _, out, _ = run(show, [str(path), "hit", "-C", "1", "--no-format"])
        self.assertEqual(out.count("   --"), 1)
        self.assertIn("    9  hit", out)

    def test_after_and_before_set_each_side_of_the_context_on_its_own(self):
        path = self.file("a.rs", RUST)
        _, out, _ = run(show, [str(path), "fn shut", "-A", "2", "--no-format"])
        numbers = [int(line.split()[0]) for line in out.splitlines()]
        self.assertEqual(numbers, [1, 2, 3, 4], "-C 3 above (only 1 line), 2 below")
        _, out, _ = run(show, [str(path), "fn shut", "-A", "2", "-B", "0", "--no-format"])
        numbers = [int(line.split()[0]) for line in out.splitlines()]
        self.assertEqual(numbers, [2, 3, 4])
        _, out, _ = run(show, [str(path), "events$", "-B", "1", "-A", "0", "--no-format"])
        self.assertEqual(out.splitlines()[-1], "    7          events")
        self.assertEqual(len(out.splitlines()), 2)

    def test_plain_text_patterns_are_not_regexes(self):
        path = self.file("a.rs", RUST)
        status, out, _ = run(show, [str(path), "vec![", "-F", "-C", "0", "--no-format"])
        self.assertEqual(status, 0)
        self.assertIn("vec![", out)

    def test_no_match_says_so_and_fails(self):
        path = self.file("a.rs", RUST)
        status, _, err = run(show, [str(path), "nothing here", "--no-format"])
        self.assertEqual(status, 1)
        self.assertIn("no line matches", err)

    def test_a_line_range_and_the_whole_file(self):
        path = self.file("a.rs", RUST)
        _, out, _ = run(show, [str(path), "--lines", "2:3", "--no-format"])
        self.assertEqual(len(out.splitlines()), 2)
        self.assertTrue(out.startswith("    2  "))
        _, out, _ = run(show, [str(path), "--no-format"])
        self.assertIn("    1  impl World {", out)

    def test_long_files_are_cut_short_unless_all_is_asked_for(self):
        path = self.file("a.txt", "\n".join(str(n) for n in range(400)))
        _, out, _ = run(show, [str(path), "--no-format"])
        self.assertIn("more lines", out)
        _, out, _ = run(show, [str(path), "--all", "--no-format"])
        self.assertNotIn("more lines", out)

    def test_only_rust_files_are_formatted(self):
        self.assertEqual(show.format_rust(Path("notes.md")), "")

    def test_the_repo_root_is_found_from_inside_it(self):
        here = Path(__file__).resolve().parent
        self.assertTrue((show.repo_root(here) / "Cargo.toml").is_file())


class ReplaceTests(TempFile):
    def replace(self, path, edits, *flags):
        return run(replace, [str(path), "--no-format", *flags], edits)

    def test_an_edit_matches_however_the_code_was_wrapped(self):
        # OLD is written on one line; the file has it split over four, with a
        # trailing comma rustfmt added.
        path = self.file("a.rs", RUST)
        old = "let events = vec![Event::DoorClosed { at }, Event::Waited];"
        new = "let events = vec![Event::DoorClosed { at }];"
        status, out, _ = self.replace(path, edit_block(old, new))
        self.assertEqual(status, 0, out)
        self.assertIn("let events = vec![Event::DoorClosed { at }];", path.read_text())
        self.assertNotIn("Waited", path.read_text())

    def test_several_edits_apply_together(self):
        path = self.file("a.rs", RUST)
        edits = edit_block("fn shut(", "fn close(") + edit_block("events\n    }", "events // done\n    }")
        status, _, _ = self.replace(path, edits)
        self.assertEqual(status, 0)
        text = path.read_text()
        self.assertIn("fn close(", text)
        self.assertIn("events // done", text)

    def test_if_any_edit_misses_nothing_is_written(self):
        path = self.file("a.rs", RUST)
        edits = edit_block("fn shut(", "fn close(") + edit_block("fn open(", "fn x(")
        status, _, err = self.replace(path, edits)
        self.assertEqual(status, 1)
        self.assertIn("edit 2: matches nowhere", err)
        self.assertIn("nothing was changed", err)
        self.assertEqual(path.read_text(), RUST)

    def test_a_miss_points_at_the_closest_text(self):
        path = self.file("a.rs", RUST)
        _, _, err = self.replace(path, edit_block("fn shut(&mut self, at: Place)", ""))
        self.assertIn("is at line 2", err)

    def test_an_old_that_matches_twice_is_refused_with_both_lines(self):
        path = self.file("a.txt", "x = 1\ny = 2\nx = 1\n")
        status, _, err = self.replace(path, edit_block("x = 1", "x = 3"))
        self.assertEqual(status, 1)
        self.assertIn("matches 2 places (lines 1, 3)", err)

    def test_all_replaces_every_match_and_still_needs_one(self):
        path = self.file("a.rs", RUST)
        status, out, _ = self.replace(path, edit_block("Event::", "Happening::"), "--all")
        self.assertEqual(status, 0, out)
        self.assertIn("every match", out)
        self.assertEqual(path.read_text().count("Happening::"), 2)
        self.assertNotIn("Event::", path.read_text())
        status, _, err = self.replace(path, edit_block("Event::", "X::"), "--all")
        self.assertEqual(status, 1)
        self.assertIn("matches nowhere", err)

    def test_all_counts_overlapping_matches_once(self):
        path = self.file("a.txt", "aaa\n")
        status, _, _ = self.replace(path, edit_block("aa", "b"), "--all")
        self.assertEqual(status, 0)
        self.assertEqual(path.read_text(), "ba\n")
        path = self.file("b.txt", "aaa\n")
        status, _, err = self.replace(path, edit_block("aa", "b"))
        self.assertEqual(status, 1, "without --all, overlapping matches are ambiguous")
        self.assertIn("matches 2 places", err)

    def test_dots_in_old_stand_for_everything_between(self):
        path = self.file("a.rs", RUST)
        old = "    fn shut(&mut self, at: Pos) -> Vec<Event> {\n...\n        events\n    }"
        new = "    fn shut(&mut self) {}"
        status, out, _ = self.replace(path, edit_block(old, new))
        self.assertEqual(status, 0, out)
        self.assertEqual(path.read_text(), "impl World {\n    fn shut(&mut self) {}\n}\n")

    def test_dots_need_a_unique_head_and_a_tail_after_it(self):
        path = self.file("a.rs", RUST)
        cases = {
            "fn open(\n...\n}": "the part before `...` matches nowhere",
            "Event::\n...\n}": "the part before `...` matches 2 places",
            "fn shut(\n...\nfn other": "the part after `...` matches nowhere after line 2",
            "fn shut(\n...\n...\n}": "2 `...` lines",
            "fn shut(\n...\n": "nothing after its `...` line",
        }
        for old, message in cases.items():
            status, _, err = self.replace(path, edit_block(old, "x"))
            self.assertEqual(status, 1, old)
            self.assertIn(message, err)
        _, _, err = self.replace(path, edit_block("fn shut(\n...\n}", "x"), "--all")
        self.assertIn("can't be used with --all", err)
        self.assertEqual(path.read_text(), RUST)

    def test_overlapping_edits_are_refused(self):
        path = self.file("a.txt", "one two three\n")
        edits = edit_block("one two", "1 2") + edit_block("two three", "2 3")
        _, _, err = self.replace(path, edits)
        self.assertIn("edits 1 and 2 overlap", err)

    def test_a_dry_run_checks_but_changes_nothing(self):
        path = self.file("a.rs", RUST)
        status, out, _ = self.replace(path, edit_block("fn shut(", "fn close("), "--dry-run")
        self.assertEqual(status, 0)
        self.assertIn("dry run", out)
        self.assertEqual(path.read_text(), RUST)

    def test_empty_new_deletes_and_an_insert_keeps_its_anchor(self):
        path = self.file("a.txt", "a\nb\nc\n")
        self.replace(path, edit_block("b", ""))
        self.assertEqual(path.read_text(), "a\n\nc\n")
        self.replace(path, edit_block("a", "a\nafter a"))
        self.assertEqual(path.read_text(), "a\nafter a\n\nc\n")

    def test_indentation_in_new_is_kept_exactly_not_doubled(self):
        # Python cares about indentation and rustfmt won't fix it there.
        text = "def f():\n    if x:\n        a()\n    return 1\n"
        path = self.file("a.py", text)
        status, _, _ = self.replace(path, edit_block("        a()", "        b()\n        c()"))
        self.assertEqual(status, 0)
        self.assertEqual(
            path.read_text(),
            "def f():\n    if x:\n        b()\n        c()\n    return 1\n",
        )

    def test_an_unindented_new_replaces_just_the_match_mid_line(self):
        path = self.file("a.txt", "let x = shut(at);\n")
        self.replace(path, edit_block("shut(at)", "close(at)"))
        self.assertEqual(path.read_text(), "let x = close(at);\n")

    def crate(self):
        """A throwaway crate, so cargo fmt has something real to format."""
        folder = Path(self.folder.name)
        (folder / "Cargo.toml").write_text(
            '[package]\nname = "t"\nversion = "0.1.0"\nedition = "2024"\n'
        )
        (folder / "src").mkdir()
        lib = folder / "src" / "lib.rs"
        lib.write_text("pub fn f() -> u32 {\n    1\n}\n")
        return lib

    def test_an_edit_that_breaks_the_code_is_rolled_back(self):
        lib = self.crate()
        status, _, err = run(replace, [str(lib)], edit_block("1", "1,,"))
        self.assertEqual(status, 1)
        self.assertIn("the edit broke the code", err)
        self.assertEqual(lib.read_text(), "pub fn f() -> u32 {\n    1\n}\n")

    def test_a_good_edit_is_formatted_and_a_broken_one_kept_if_asked(self):
        lib = self.crate()
        status, _, _ = run(replace, [str(lib)], edit_block("1", "1+   2"))
        self.assertEqual(status, 0)
        self.assertIn("1 + 2", lib.read_text(), "rustfmt tidied it")
        status, _, _ = run(replace, [str(lib), "--allow-broken"], edit_block("1 + 2", "(("))
        self.assertEqual(status, 0)
        self.assertIn("((", lib.read_text())

    def test_an_old_ending_in_a_comma_never_leaves_two(self):
        text = "enum E {\n    A {\n        recipe: String,\n    },\n}\n"
        path = self.file("a.rs", text)
        old = "    A {\n        recipe: String,"
        new = "    A {\n        recipe: String,\n        item: String,"
        status, _, err = self.replace(path, edit_block(old, new))
        self.assertEqual(status, 0, err)
        self.assertEqual(
            path.read_text(),
            "enum E {\n    A {\n        recipe: String,\n        item: String,\n    },\n}\n",
        )

    def test_commas_before_closers_only_count_as_noise_in_rust(self):
        self.assertEqual(replace.normalise("f(a, b,\n)", True)[0], "f(a,b)")
        self.assertEqual(replace.normalise("f(a, b,\n)", False)[0], "f(a,b,)")

    def test_malformed_or_missing_edits_are_errors(self):
        path = self.file("a.txt", "a\n")
        for edits, message in [
            ("stray text\n", "outside an edit block"),
            ("<<<< OLD\na\n", "has no ==== NEW"),
            ("<<<< OLD\na\n==== NEW\nb\n", "has no >>>> END"),
            ("", "no edit blocks"),
            (edit_block("   ", "b"), "OLD is empty"),
            (edit_block("zzz", "b"), "not even its first character"),
        ]:
            status, _, err = self.replace(path, edits)
            self.assertEqual(status, 1, message)
            self.assertIn(message, err)


class PlayScriptTests(unittest.TestCase):
    def test_plain_keys_repeats_and_named_keys(self):
        steps = play.parse_script("ll{k*3}{Esc}{show}{wait:250}{Enter}x")
        self.assertEqual(
            steps,
            [
                ("type", "llkkk"),
                ("key", "Escape"),
                ("show", None),
                ("wait", 0.25),
                ("key", "Enter"),
                ("type", "x"),
            ],
        )

    def test_a_repeat_can_be_several_keys(self):
        self.assertEqual(play.parse_script("{lk*2}"), [("type", "lklk")])

    def test_bad_braces_are_errors(self):
        for script in ["{k*3", "{Escape}", "{wait:x}"]:
            with self.assertRaises(ValueError, msg=script):
                play.parse_script(script)

    def test_typed_text_split_by_nothing_is_merged(self):
        self.assertEqual(
            play.merged([("type", "a"), ("type", "b"), ("show", None), ("type", "c")]),
            [("type", "ab"), ("show", None), ("type", "c")],
        )

    def test_an_empty_script_has_no_steps(self):
        self.assertEqual(play.parse_script(""), [])


class FakeClock:
    """A clock that moves only when slept, so settle() runs instantly."""

    def __init__(self):
        self.now = 0.0

    def __call__(self):
        return self.now

    def sleep(self, seconds):
        self.now += seconds


class PlaySettleTests(unittest.TestCase):
    def test_waits_until_the_screen_stops_changing(self):
        frames = iter(["", "", "loading", "game", "game", "game", "game", "game"])
        clock = FakeClock()
        screen = play.settle(lambda: next(frames), 10, clock=clock, sleep=clock.sleep)
        self.assertEqual(screen, "game")
        # Steady from 0.15 s, so done 0.15 s later, give or take one poll
        # (floating-point sums of 0.05 aren't exact).
        self.assertTrue(0.25 <= clock.now <= 0.36, clock.now)

    def test_gives_up_after_the_timeout_with_the_last_screen(self):
        counter = iter(range(1000))
        clock = FakeClock()
        screen = play.settle(lambda: str(next(counter)), 1, clock=clock, sleep=clock.sleep)
        self.assertEqual(screen, "20")
        self.assertGreaterEqual(clock.now, 1)


class PlayOutputTests(unittest.TestCase):
    def args(self, *argv):
        return play.make_parser().parse_args(list(argv))

    def test_lines_and_grep_cut_the_screen_down(self):
        screen = "one\ntwo\nthree\nfour\n"
        self.assertEqual(play.filtered(screen, self.args("--lines", "2:3")), "two\nthree")
        self.assertEqual(play.filtered(screen, self.args("--grep", "^t")), "two\nthree")
        self.assertEqual(play.filtered(screen, self.args()), "one\ntwo\nthree\nfour")

    def test_the_game_always_saves_to_the_scratch_folder(self):
        command = play.game_command(Path("/repo"), self.args(), "/tmp/x y")
        self.assertTrue(command.startswith("NIGHTHAWK_SAVE_DIR='/tmp/x y' "))
        self.assertIn("'/repo/target/debug/nighthawk-tui' --seed 42;", command)
        self.assertIn(play.EXITED, command)
        no_seed = play.game_command(Path("/repo"), self.args("--no-seed", "--data", "/d"), "/s")
        self.assertIn("NIGHTHAWK_DATA='/d'", no_seed)
        self.assertNotIn("--seed", no_seed)

    def test_quoting_survives_single_quotes(self):
        self.assertEqual(play.sh_quote("it's"), "'it'\\''s'")


GAME = show.repo_root(Path(__file__).resolve().parent) / "target" / "debug" / "nighthawk-tui"


class BlessTests(TempFile):
    SNAPSHOT = 'const EXPECTED: &str = r#"{\n  "old": 1\n}"#;\nconst OTHER: &str = r#"x"#;\n'
    FAILURE = (
        "thread 'the_format_is_stable' panicked at src/a.rs:9:5:\n"
        "assertion `left == right` failed\n"
        '  left: "{\\n  \\"new\\": 2,\\n  \\"tab\\": \\"\\t\\\\\\u{e9}\\"\\n}"\n'
        ' right: "{\\n  \\"old\\": 1\\n}"\n'
    )

    def fake_runner(self, outcomes):
        """A test runner that gives each (passed, output) in turn."""
        calls = iter(outcomes)
        return lambda root, test: next(calls)

    def test_rust_debug_strings_are_unescaped(self):
        self.assertEqual(bless.debug_unescape(r'"a\n\"b\" \\ \t \u{e9}"'), 'a\n"b" \\ \t \u00e9')
        with self.assertRaises(ValueError):
            bless.debug_unescape("no quotes")

    def test_the_left_value_goes_into_the_constant_and_nothing_else_changes(self):
        path = self.file("a.txt", self.SNAPSHOT)
        runner = self.fake_runner([(False, self.FAILURE), (True, "ok")])
        out = io.StringIO()
        with redirect_stdout(out):
            status = bless.main(["the_format", str(path), "EXPECTED"], runner)
        self.assertEqual(status, 0)
        text = path.read_text()
        self.assertIn('r#"{\n  "new": 2,\n  "tab": "\t\\\u00e9"\n}"#;', text)
        self.assertIn('const OTHER: &str = r#"x"#;', text, "other constants are untouched")

    def test_a_passing_test_needs_no_blessing(self):
        path = self.file("a.txt", self.SNAPSHOT)
        status, out, _ = run_with(bless, ["t", str(path), "EXPECTED"], self.fake_runner([(True, "")]))
        self.assertEqual(status, 0)
        self.assertIn("already passes", out)
        self.assertEqual(path.read_text(), self.SNAPSHOT)

    def test_mistakes_change_nothing(self):
        path = self.file("a.txt", self.SNAPSHOT)
        no_left = self.fake_runner([(False, "error[E0425]: cannot find value")])
        status, _, err = run_with(bless, ["t", str(path), "EXPECTED"], no_left)
        self.assertEqual(status, 1)
        self.assertIn("no `left:` line", err)
        wrong_name = self.fake_runner([(False, self.FAILURE)])
        status, _, err = run_with(bless, ["t", str(path), "MISSING"], wrong_name)
        self.assertIn("no `const MISSING", err)
        self.assertEqual(path.read_text(), self.SNAPSHOT)

    def test_a_build_failure_says_so_with_its_errors(self):
        path = self.file("a.txt", self.SNAPSHOT)
        broken = "error[E0432]: unresolved import `x::Y`\n  --> src/a.rs:3:5\nmore\nerror: could not compile `a`\n"
        status, _, err = run_with(bless, ["t", str(path), "EXPECTED"], self.fake_runner([(False, broken)]))
        self.assertEqual(status, 1)
        self.assertIn("didn't compile", err)
        self.assertIn("--> src/a.rs:3:5", err)
        self.assertNotIn("more", err)

    def test_a_test_that_still_fails_is_reported(self):
        path = self.file("a.txt", self.SNAPSHOT)
        runner = self.fake_runner([(False, self.FAILURE), (False, "still broken")])
        status, _, err = run_with(bless, ["t", str(path), "EXPECTED"], runner)
        self.assertEqual(status, 1)
        self.assertIn("still fails", err)

    def test_dry_run_prints_without_writing(self):
        path = self.file("a.txt", self.SNAPSHOT)
        runner = self.fake_runner([(False, self.FAILURE)])
        status, out, _ = run_with(bless, ["t", str(path), "EXPECTED", "--dry-run"], runner)
        self.assertEqual(status, 0)
        self.assertIn('"new": 2', out)
        self.assertEqual(path.read_text(), self.SNAPSHOT)

    def test_a_value_that_would_close_the_raw_string_is_refused(self):
        with self.assertRaises(ValueError):
            bless.replace_const(self.SNAPSHOT, "EXPECTED", 'a "# b')


def run_with(module, argv, runner):
    """Like `run`, for a main() that also takes a test runner."""
    out, err = io.StringIO(), io.StringIO()
    with redirect_stdout(out), redirect_stderr(err):
        status = module.main(argv, runner)
    return status, out.getvalue(), err.getvalue()


@unittest.skipUnless(shutil.which("tmux") and GAME.exists(), "needs tmux and a built game")
class PlayInTmuxTests(unittest.TestCase):
    def test_a_real_game_shows_its_screen_and_leaves_no_session_behind(self):
        status, out, _ = run(play, ["--no-build", "lll", "--grep", "Pos"])
        self.assertEqual(status, 0, out)
        self.assertIn("Pos 13, 7", out)
        sessions = subprocess.run(
            ["tmux", "-L", play.SOCKET, "list-sessions"], capture_output=True, text=True
        ).stdout
        self.assertNotIn(f"nh-play-{os.getpid()}", sessions)

    def test_a_game_that_exits_says_so(self):
        with tempfile.TemporaryDirectory() as empty:
            status, out, _ = run(play, ["--no-build", "--data", empty])
        self.assertEqual(status, 1)
        self.assertIn(play.EXITED, out)


if __name__ == "__main__":
    unittest.main()
