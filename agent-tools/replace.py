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
    agent-tools/replace.py FILE --all < edits.txt       # replace every match (a rename)

Why not just search and replace:
- Rust files are run through `cargo fmt` first, so OLD is matched against the
  file as the formatter left it, and again afterwards, so the result is tidy.
- Matching ignores whitespace (spaces, indentation, line breaks), and in Rust
  files a comma just before a closing ), ] or }. rustfmt changes exactly
  those things when it reflows code, so an OLD copied before a reflow still
  matches.
- Every OLD must match exactly one place. If any edit matches nowhere, or in
  more than one place, nothing at all is written, and the error says where
  the closest text is (or where all the matches are). With --all, an OLD
  may match any number of places (at least one) and every one is replaced:
  for renames. Make OLD distinctive (`catalogue.` rather than `cat`), as
  every match is replaced.
- NEW goes in exactly as written (then rustfmt tidies Rust files).

To insert, put the line you're inserting after in OLD, and that line plus the
new ones in NEW. To delete, leave NEW empty.

To replace a long block without copying it all, put a line holding only
`...` in OLD: the text before it must match exactly one place, and the
match runs on to the first place after that where the text after it
matches. So OLD can be a function's first line, `...`, and its last lines.

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


def swallow_comma(file_text, end):
    """`end`, moved past the next comma if only whitespace comes before it.

    An OLD ending in a comma ("recipe: String,") has that comma dropped when
    normalised, as the end of OLD might be just before a closing bracket.
    So the match stops before the file's comma; this takes it in too, or
    NEW's own trailing comma would make two."""
    at = end
    while at < len(file_text) and file_text[at].isspace():
        at += 1
    return at + 1 if at < len(file_text) and file_text[at] == "," else end


def next_visible(text, start):
    """The first non-whitespace character at or after `start`, or "" at the
    end. Note that "" counts as `in CLOSERS` (every string contains the empty
    string): the end of a snippet is treated like a closing bracket, since
    the snippet may stop just before one."""
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


ELISION = "..."


def split_elision(old):
    """(head, tail) if OLD has one `...` line, (old, None) if it has none.
    Raises ValueError for more than one."""
    lines = old.split("\n")
    marks = [i for i, line in enumerate(lines) if line.strip() == ELISION]
    if not marks:
        return old, None
    if len(marks) > 1:
        raise ValueError(f"OLD has {len(marks)} `...` lines; use at most one")
    i = marks[0]
    return "\n".join(lines[:i]), "\n".join(lines[i + 1 :])


def locate(file_text, old, rust, many=False):
    """The (start, end) spans where `old` matches (exactly one, or with
    `many` at least one, none overlapping), or an error message."""
    try:
        head, tail = split_elision(old)
    except ValueError as e:
        return str(e)
    if tail is not None:
        return locate_elided(file_text, head, tail, rust, many)
    return locate_exact(file_text, old, rust, many)


def locate_elided(file_text, head, tail, rust, many):
    """The span from the one match of `head` to the end of the first match
    of `tail` after it, or an error message."""
    if many:
        return "`...` can't be used with --all"
    if not normalise(tail, rust)[0]:
        return "OLD has nothing after its `...` line"
    found = locate_exact(file_text, head, rust)
    if isinstance(found, str):
        return f"the part before `...` {found}"
    start, head_end = found[0]
    norm_file, origin = normalise(file_text, rust)
    norm_tail, _ = normalise(tail, rust)
    from_here = next((i for i, pos in enumerate(origin) if pos >= head_end), len(norm_file))
    at = norm_file.find(norm_tail, from_here)
    if at == -1:
        return f"the part after `...` matches nowhere after line {line_of(file_text, head_end)}"
    return [(start, span(file_text, origin, at, norm_tail, tail, rust)[1])]


def locate_exact(file_text, old, rust, many=False):
    """`locate` for an OLD without a `...` line."""
    norm_file, origin = normalise(file_text, rust)
    norm_old, _ = normalise(old, rust)
    if not norm_old:
        return "OLD is empty"
    starts = find_all(norm_file, norm_old)
    if many:
        starts = without_overlaps(starts, len(norm_old))
    if len(starts) == 1 or (many and starts):
        return [span(file_text, origin, first, norm_old, old, rust) for first in starts]
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


def without_overlaps(starts, length):
    """`starts`, skipping any that begin inside the match before ("aa" in
    "aaa" matches once, not twice)."""
    kept = []
    for s in starts:
        if not kept or s >= kept[-1] + length:
            kept.append(s)
    return kept


def span(file_text, origin, first, norm_old, old, rust):
    """The (start, end) in the file of the match at `first` in the
    normalised text."""
    end = origin[first + len(norm_old) - 1] + 1
    if rust and old.rstrip().endswith(","):
        end = swallow_comma(file_text, end)
    return origin[first], end


def apply(file_text, edits, rust, many=False):
    """The text with every edit applied, or a list of error messages."""
    spans, errors = [], []
    for number, (old, new) in enumerate(edits, start=1):
        found = locate(file_text, old, rust, many)
        if isinstance(found, str):
            errors.append(f"edit {number}: {found}")
        else:
            spans.extend((start, end, new, number) for start, end in found)
    spans.sort()
    for (_, end_a, _, a), (start_b, _, _, b) in zip(spans, spans[1:]):
        if start_b < end_a:
            errors.append(f"edits {a} and {b} overlap")
    if errors:
        return errors
    # Apply from the end back, so earlier positions stay valid.
    for start, end, new, _ in reversed(spans):
        start = from_line_start(file_text, start, new)
        file_text = file_text[:start] + new + file_text[end:]
    return file_text


def from_line_start(file_text, start, new):
    """Where to start replacing. A match starts at its first non-space
    character, but if NEW brings its own indentation and only indentation
    comes before the match on its line, replace from the start of the line,
    so the indentation isn't doubled (which matters in Python and Markdown,
    where rustfmt won't tidy it)."""
    line_start = file_text.rfind("\n", 0, start) + 1
    indented = new[:1] in (" ", "\t")
    if indented and not file_text[line_start:start].strip():
        return line_start
    return start


def main(argv=None, stdin=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("file", type=Path)
    parser.add_argument("--dry-run", action="store_true", help="check only; change nothing")
    parser.add_argument("--all", action="store_true", help="replace every match of each OLD")
    parser.add_argument("--no-format", action="store_true", help="don't run cargo fmt")
    parser.add_argument(
        "--allow-broken",
        action="store_true",
        help="keep the edit even if cargo fmt can't parse the result",
    )
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
    original = args.file.read_text()
    result = apply(original, edits, rust, args.all)
    if isinstance(result, list):
        print("\n".join(result) + "\nnothing was changed", file=sys.stderr)
        return 1
    if args.dry_run:
        print(f"all {len(edits)} edits match; nothing was changed (dry run)")
        return 0
    args.file.write_text(result)
    warning = "" if args.no_format else format_rust(args.file)
    if warning and not args.allow_broken:
        # The edit left code rustfmt can't parse (a stray comma, a missing
        # brace): put the file back rather than leave it broken.
        args.file.write_text(original)
        print(warning + "\nthe edit broke the code, so nothing was changed", file=sys.stderr)
        return 1
    count = f"{len(edits)} edits" if not args.all else f"{len(edits)} edits, every match"
    print(f"applied {count} to {args.file}")
    if warning:
        print(warning, file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
