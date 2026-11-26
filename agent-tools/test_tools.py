"""Tests for the agent tools. Run with:

    python3 -m unittest discover -s agent-tools -p 'test_*.py'

`scripts/check.sh` runs them too. They never touch the real source: every
file they edit is in a temporary folder, and formatting is switched off.
"""

import io
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

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


if __name__ == "__main__":
    unittest.main()
