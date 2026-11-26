#!/usr/bin/env python3
"""Make exact edits to one file, without tripping over formatting.

Reads edit blocks from standard input, each an OLD text to find and the NEW
text to put in its place:

    <<<< OLD
    fn shut(&mut self, at: Pos) -> Vec<Event> {
    ==== NEW
    fn shut(&mut self, at: Pos) -> Vec<Event> {
        // a new comment
    >>>> END

    agent-tools/replace.py FILE < edits.txt
    agent-tools/replace.py FILE --dry-run < edits.txt   # check, change nothing

Why not just search and replace:
- Rust files are run through `cargo fmt` first, so OLD is matched against the
  file as the formatter left it, and again afterwards, so the result is tidy.
- Matching ignores whitespace (spaces, indentation, line breaks), and in Rust
  files a comma just before a closing ), ] or }. rustfmt changes exactly
  those things when it reflows code, so an OLD copied before a reflow still
  matches.
- Every OLD must match exactly one place. If any edit matches nowhere, or in
  more than one place, nothing at all is written, and the error says where
  the closest text is (or where all the matches are).
- NEW goes in exactly as written (then rustfmt tidies Rust files).

To insert, put the line you're inserting after in OLD, and that line plus the
new ones in NEW. To delete, leave NEW empty.

Exit status: 0 if every edit applied, 1 if none did (the file is untouched).
Only the Python standard library is used.
"""

import argparse
import sys
from pathlib import Path

from show import format_rust

OLD, NEW, END = "<<<< OLD", "==== NEW", ">>>> END"
CLOSERS = ")]}"


def parse_edits(text):
    """The (old, new) pairs in `text`. Raises ValueError on a malformed block."""
    edits, state, old, new = [], None, [], []
    for number, line in enumerate(text.split("\n"), start=1):
        marker = line.strip()
        if state is None and marker == OLD:
            state, old, new = "old", [], []
        elif state == "old" and marker == NEW:
            state = "new"
        elif state == "new" and marker == END:
            edits.append(("\n".join(old), "\n".join(new)))
            state = None
        elif state == "old":
            old.append(line)
        elif state == "new":
            new.append(line)
        elif marker:
            raise ValueError(f"line {number}: text outside an edit block: {line!r}")
    if state is not None:
        raise ValueError(f"the last edit block has no {END if state == 'new' else NEW}")
    return edits


def normalise(text, rust):
    """`text` without the differences rustfmt makes, and where each kept
    character came from: (kept text, list of original positions).

    Whitespace is dropped. In Rust, so is a comma whose next non-space
    character is a closing bracket (the trailing comma rustfmt adds when it
    splits a list over lines).
    """
    kept, origin = [], []
    for i, ch in enumerate(text):
        if ch.isspace():
            continue
        if rust and ch == "," and next_visible(text, i + 1) in CLOSERS:
            continue
        kept.append(ch)
        origin.append(i)
    return "".join(kept), origin


def next_visible(text, start):
    """The first non-whitespace character at or after `start`, or ""."""
    for ch in text[start:]:
        if not ch.isspace():
            return ch
    return ""


def find_all(haystack, needle):
    """Every start index of `needle` in `haystack`."""
    starts, at = [], haystack.find(needle)
    while at != -1:
        starts.append(at)
        at = haystack.find(needle, at + 1)
    return starts


def line_of(text, position):
    """The 1-based line number of `position` in `text`."""
    return text.count("\n", 0, position) + 1


def closest(file_text, norm_file, origin, norm_old):
    """Where the longest start of `norm_old` appears in the file, as a hint
    for an edit that didn't match: (line number, how much of OLD matched)."""
    low, high, best = 1, len(norm_old), None
    while low <= high:  # binary search for the longest matching prefix
        mid = (low + high) // 2
        at = norm_file.find(norm_old[:mid])
        if at == -1:
            high = mid - 1
        else:
            best, low = (line_of(file_text, origin[at]), mid), mid + 1
    return best


def locate(file_text, old, rust):
    """The (start, end) of the one place `old` matches, or an error message."""
    norm_file, origin = normalise(file_text, rust)
    norm_old, _ = normalise(old, rust)
    if not norm_old:
        return "OLD is empty"
    starts = find_all(norm_file, norm_old)
    if len(starts) == 1:
        first = starts[0]
        return origin[first], origin[first + len(norm_old) - 1] + 1
    if starts:
        lines = ", ".join(str(line_of(file_text, origin[s])) for s in starts)
        return f"matches {len(starts)} places (lines {lines}); add more context to OLD"
    hint = closest(file_text, norm_file, origin, norm_old)
    if hint is None:
        return "matches nowhere, not even its first character"
    line, matched = hint
    return (
        f"matches nowhere; the longest start of it ({matched} of {len(norm_old)} "
        f"non-space characters) is at line {line}"
    )


def apply(file_text, edits, rust):
    """The text with every edit applied, or a list of error messages."""
    spans, errors = [], []
    for number, (old, new) in enumerate(edits, start=1):
        found = locate(file_text, old, rust)
        if isinstance(found, str):
            errors.append(f"edit {number}: {found}")
        else:
            spans.append((found[0], found[1], new, number))
    spans.sort()
    for (_, end_a, _, a), (start_b, _, _, b) in zip(spans, spans[1:]):
        if start_b < end_a:
            errors.append(f"edits {a} and {b} overlap")
    if errors:
        return errors
    # Apply from the end back, so earlier positions stay valid.
    for start, end, new, _ in reversed(spans):
        file_text = file_text[:start] + new + file_text[end:]
    return file_text


def main(argv=None, stdin=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("file", type=Path)
    parser.add_argument("--dry-run", action="store_true", help="check only; change nothing")
    parser.add_argument("--no-format", action="store_true", help="don't run cargo fmt")
    args = parser.parse_args(argv)
    rust = args.file.suffix == ".rs"
    try:
        edits = parse_edits((stdin or sys.stdin).read())
    except ValueError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    if not edits:
        print("error: no edit blocks given", file=sys.stderr)
        return 1
    if not args.no_format:
        format_rust(args.file)
    result = apply(args.file.read_text(), edits, rust)
    if isinstance(result, list):
        print("\n".join(result) + "\nnothing was changed", file=sys.stderr)
        return 1
    if args.dry_run:
        print(f"all {len(edits)} edits match; nothing was changed (dry run)")
        return 0
    args.file.write_text(result)
    warning = "" if args.no_format else format_rust(args.file)
    print(f"applied {len(edits)} edits to {args.file}")
    if warning:
        print(warning, file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
