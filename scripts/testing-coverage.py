#!/usr/bin/env python3
"""The "Covered by" column of docs/testing.md, written from the tests themselves.

A test named after its scenario is listed under it:
- FFI: `fn xx_N_…` with `#[test]` in crates/lc-ffi/tests/scenarios_*.rs;
- iOS: `func testXXN_…` in apps/ios/LichenUITests/*.swift, with its class;
- Android: `fun XX_N_…` (or `` fun `XX_N_…` ``) with `@Test` in apps/android/app/src/test/ and
  androidTest/, with its class. A test's `_fr`, `_frDark` and `_dark` variants count with it.
Tests named otherwise, and lc-core's, are listed in OTHER below. A test named after an id the
doc does not have (a typo) fails the check. What a cell says beyond its tests is kept:
`real radio: …` (a part only a real companion shows, so the scenario's id must be in the
real-radio checklist at the end of the doc) and `not automated: …` (why). A scenario with no
test needs one of these.

Usage: python3 -I scripts/testing-coverage.py           # rewrites docs/testing.md
       python3 -I scripts/testing-coverage.py --check   # fails when the doc is not current
       python3 -I scripts/testing-coverage.py --gaps    # what has no test, or one app's only
"""

import difflib
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOC = ROOT / "docs" / "testing.md"
COMMAND = "python3 -I scripts/testing-coverage.py"

FFI = ROOT / "crates" / "lc-ffi" / "tests"
IOS = ROOT / "apps" / "ios" / "LichenUITests"
ANDROID = [ROOT / "apps" / "android" / "app" / "src" / d for d in ("test", "androidTest")]
SUITES = ["FFI", "iOS", "Android", "lc-core"]

# Variants of a test that run it again in French or in dark mode, as the names end.
VARIANTS = {"fr": "fr", "frDark": "fr dark", "dark": "dark"}

# Tests not named after a scenario that cover one: (scenario, suite, file, test or None for the
# whole class). The check fails when one is gone.
OTHER = [
    ("RL-8", "lc-core", "crates/lc-core/tests/radio.rs", "connecting_sets_the_radios_clock"),
    ("RL-9", "lc-core", "crates/lc-core/src/core.rs", "a_contact_list_cut_short_still_counts"),
    ("RL-9", "lc-core", "crates/lc-core/src/core.rs", "records_lost_before_the_end_are_asked_for_again"),
    ("CT-3", "lc-core", "crates/lc-core/tests/signal.rs", "a_direct_advert_is_attributed_and_a_relayed_one_is_not"),
    ("CT-3", "lc-core", "crates/lc-core/tests/signal.rs", "a_one_byte_hash_is_attributed_only_when_unique"),
    ("CT-3", "lc-core", "crates/lc-core/tests/signal.rs", "ratings_match_the_vectors"),
    ("CT-3", "lc-core", "crates/lc-core/tests/signal.rs", "the_history_is_capped_by_count_and_by_age"),
    ("CT-3", "lc-core", "crates/lc-core/tests/search.rs", "group_envelopes_stay_with_locate"),
    ("CT-7", "lc-core", "crates/lc-core/tests/signal.rs", "the_route_joins_the_path_the_readings_and_the_trace"),
    ("CT-14", "lc-core", "crates/lc-core/tests/readings.rs", "a_request_answered"),
    ("CT-14", "lc-core", "crates/lc-core/tests/readings.rs", "a_request_timed_out"),
    ("CT-14", "lc-core", "crates/lc-core/tests/readings.rs", "two_requests_to_one_node_give_one_in_flight"),
    ("CT-14", "lc-core", "crates/lc-core/tests/readings.rs", "the_history_is_capped_by_age_and_count"),
    ("DM-9", "iOS", "apps/ios/LichenUITests/ChatNavigationTests.swift", "testALongChatKeepsItsPlaces"),
    ("LC-1", "iOS", "apps/ios/LichenUITests/ChatNavigationTests.swift", "testTappingANotificationOpensItsChat"),
    ("LC-6", "iOS", "apps/ios/LichenUITests/ContactsTests.swift", "testCT11_PersonFiledAsARepeaterKeepsTheirRow"),
    ("ST-6", "iOS", "apps/ios/LichenUITests/StringsTests.swift", "testD6_NoEnglishWordsLeftInFrench"),
    ("ST-9", "iOS", "apps/ios/LichenUITests/AccessibilityAuditTests.swift", None),
]

ROW = re.compile(r"^\|\s*([A-Z]{2}-\d+)\s*\|")
PIPE = re.compile(r"(?<!\\)\|")
CHECKLIST = "## Real radio checklist"
COVERAGE = "**Coverage:** "
NOTES = ("real radio", "not automated:")


class Test:
    def __init__(self, suite, owner, name, path, line):
        self.suite, self.owner, self.name, self.path, self.line = suite, owner, name, path, line
        self.variants = []

    def where(self):
        return f"{self.path.relative_to(ROOT)}:{self.line}"


def scenario_id(name, pattern):
    """`XX-N` from a test's name, or None when it is not named after a scenario."""
    m = pattern.match(name)
    return f"{m.group(1).upper()}-{int(m.group(2))}" if m else None


def rust_tests(path):
    """(name, line, ignored) of the `#[test]` functions of a Rust file."""
    found, attrs = [], set()
    for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        s = line.strip()
        if m := re.match(r"#\[(\w+)", s):
            attrs.add(m.group(1))
        elif m := re.match(r"(?:pub(?:\([\w:]+\))?\s+)?(?:async\s+)?fn\s+(\w+)", s):
            if "test" in attrs:
                found.append((m.group(1), n, "ignore" in attrs))
            attrs = set()
    return found


def swift_tests(path):
    """(class, name, line) of the XCTest methods of a Swift file."""
    found, owner = [], None
    for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if m := re.match(r"(?:(?:final|public|open)\s+)*class\s+(\w+)", line):  # top level
            owner = m.group(1)
        elif m := re.match(r"\s*func\s+(test\w*)\s*\(", line):
            found.append((owner, m.group(1), n))
    return found


def kotlin_tests(path):
    """(class, name, line, ignored) of the `@Test` functions of a Kotlin file."""
    found, owner, attrs = [], None, set()
    for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if m := re.match(r"(?:(?:open|abstract|internal|private)\s+)*class\s+(\w+)", line):  # top level
            owner, attrs = m.group(1), set()
            continue
        s = line.strip()
        while m := re.match(r"@(\w+)(?:\([^)]*\))?\s*", s):
            attrs.add(m.group(1))
            s = s[m.end():]
        if m := re.match(r"(?:(?:override|public|internal|private|suspend)\s+)*fun\s+`?([^`(]+?)`?\s*\(", s):
            if "Test" in attrs:
                found.append((owner, m.group(1), n, "Ignore" in attrs))
            attrs = set()
    return found


def collect():
    """(id, test) of every test named after a scenario, ids not in the doc included; ignored
    tests, which no run makes, left out."""
    named = []
    for path in sorted(FFI.glob("scenarios_*.rs")):
        for name, line, ignored in rust_tests(path):
            if not ignored:
                named.append((scenario_id(name, re.compile(r"([a-z]{2})_(\d+)_")), Test("FFI", None, name, path, line)))
    for path in sorted(IOS.glob("*.swift")):
        for owner, name, line in swift_tests(path):
            named.append((scenario_id(name, re.compile(r"test([A-Z]{2})(\d+)_")), Test("iOS", owner, name, path, line)))
    for base in ANDROID:
        for path in sorted(base.rglob("*.kt")):
            for owner, name, line, ignored in kotlin_tests(path):
                if not ignored:
                    named.append((scenario_id(name, re.compile(r"([A-Z]{2})_(\d+)_")), Test("Android", owner, name, path, line)))
    return [(sid, t) for sid, t in named if sid]


def fold(tests):
    """A test's variants folded into it, when it exists in the same class."""
    by_name = {(t.suite, t.owner, t.name): t for t in tests}
    kept = []
    for t in tests:
        base, _, suffix = (t.name or "").rpartition("_")
        if suffix in VARIANTS and (t.suite, t.owner, base) in by_name:
            by_name[(t.suite, t.owner, base)].variants.append(VARIANTS[suffix])
        else:
            kept.append(t)
    return kept


def other_tests(errors):
    """The tests of OTHER, checked to be there."""
    found = []
    for sid, suite, file, name in OTHER:
        path = ROOT / file
        if not path.exists():
            here = []
        elif path.suffix == ".swift":
            here = [(owner, test) for owner, test, _ in swift_tests(path)]
        else:
            here = [(file.split("/", 2)[2], test) for test, _, ignored in rust_tests(path) if not ignored]
        match = [(owner, test) for owner, test in here if name in (None, test)]
        if match:
            found.append((sid, Test(suite, match[0][0], name, path, 0)))
        else:
            errors.append(f"scripts/testing-coverage.py OTHER: {name or 'the class'} in {file} for {sid} is gone")
    return found


def render(tests):
    """One line per suite: names grouped by class, `Class.test` or `Class: test, test`."""
    lines = []
    order = list(VARIANTS.values()).index
    for suite in SUITES:
        groups = {}
        for t in tests:
            if t.suite == suite:
                names = groups.setdefault(t.owner, [])
                if t.name is not None:  # None: the whole class, which then names itself alone
                    names.append(t.name + (f" (+{', '.join(sorted(set(t.variants), key=order))})" if t.variants else ""))
        if not groups:
            continue
        parts = []
        for owner, names in groups.items():
            if owner is None:
                parts.append(", ".join(names))
            elif not names:
                parts.append(owner)
            elif len(names) == 1 and suite != "lc-core":
                parts.append(f"{owner}.{names[0]}")
            else:
                parts.append(f"{owner}: {', '.join(names)}")
        lines.append(f"{suite} {'; '.join(parts)}")
    return lines


def notes_of(sid, cell, errors):
    """The notes a cell holds; its test lines are written again."""
    notes = []
    for part in (p.strip() for p in cell.split("<br>")):
        text = part.removeprefix("·").strip()
        if not text or any(part.startswith(f"{s} ") for s in SUITES):
            continue
        if text.startswith(NOTES):
            notes.append(text)
        else:
            errors.append(f"{sid}: unknown text in Covered by: {part!r} "
                          f"(a note starts with {' or '.join(repr(n) for n in NOTES)})")
    return notes


def generate(doc):
    """The doc as it should be, the errors found, and the coverage by scenario."""
    errors = []
    lines = doc.split("\n")
    rows = {}
    for i, line in enumerate(lines):
        if m := ROW.match(line):
            if m.group(1) in rows:
                errors.append(f"{m.group(1)} is in the doc twice")
            rows[m.group(1)] = i

    tests = {sid: [] for sid in rows}
    for sid, t in collect():
        if sid in rows:
            tests[sid].append(t)
        else:
            errors.append(f"{t.where()}: {t.name} is named after {sid}, which is not in docs/testing.md")
    for sid, t in other_tests(errors):
        if sid not in rows:
            errors.append(f"scripts/testing-coverage.py OTHER: {sid} is not in docs/testing.md")
        else:
            tests[sid].append(t)

    start = next((i for i, line in enumerate(lines) if line.startswith(CHECKLIST)), None)
    if start is None:
        errors.append(f"docs/testing.md has no {CHECKLIST!r} section")
        checklist = set()
    else:
        end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith("## ")), len(lines))
        checklist = set(re.findall(r"\b[A-Z]{2}-\d+\b", "\n".join(lines[start:end])))
    for sid in sorted(checklist - set(rows)):
        errors.append(f"the real-radio checklist names {sid}, which is not a scenario")

    coverage = {}
    for sid, i in rows.items():
        cells = PIPE.split(lines[i])
        if len(cells) != 7:
            errors.append(f"{sid}: the row has {len(cells) - 2} cells, not 5")
            continue
        notes = notes_of(sid, cells[5], errors)
        suites = fold(tests[sid])
        coverage[sid] = {s for s in SUITES if any(t.suite == s for t in suites)}
        radio = any(n.startswith("real radio") for n in notes)
        if radio and sid not in checklist:
            errors.append(f"{sid} says `real radio` but is not in the real-radio checklist")
        if sid in checklist and not radio:
            errors.append(f"{sid} is in the real-radio checklist but its Covered by has no `real radio: …`")
        if not suites and not notes:
            errors.append(f"{sid} has no test and no reason: name a test after it, or write "
                          "`not automated: <why>` or `real radio: <what>` in its Covered by cell")
        body = render(suites)
        body += [f"· {n}" for n in notes] if body else notes
        cells[5] = f" {'<br>'.join(body)} "
        lines[i] = "|".join(cells)

    total = len(coverage)
    some = sum(1 for c in coverage.values() if c)
    per = ", ".join(f"{s} {sum(1 for c in coverage.values() if s in c)}" for s in SUITES)
    summary = (f"{COVERAGE}{some} of {total} scenarios have at least one automated test ({per}). "
               f"Written by `{COMMAND}`.")
    at = next((i for i, line in enumerate(lines) if line.startswith(COVERAGE)), None)
    if at is not None:
        lines[at] = summary
    else:
        first = next(i for i, line in enumerate(lines) if line.startswith("## "))
        lines[first:first] = [summary, ""]
    return "\n".join(lines), errors, coverage


def gaps(doc, coverage):
    """What has no automated test, and what one app tests and the other does not."""
    reasons = {}
    for line in doc.split("\n"):
        if m := ROW.match(line):
            reasons[m.group(1)] = PIPE.split(line)[5].strip()
    out = ["No automated test:"]
    out += [f"  {sid}: {reasons[sid]}" for sid, c in coverage.items() if not c] or ["  none"]
    for has, lacks in (("Android", "iOS"), ("iOS", "Android")):
        out.append(f"{has} only (no {lacks} test):")
        out += [f"  {sid}" for sid, c in coverage.items() if has in c and lacks not in c] or ["  none"]
    return "\n".join(out)


def main(args):
    if args not in ([], ["--check"], ["--gaps"]):
        print(__doc__.strip(), file=sys.stderr)
        return 2
    doc = DOC.read_text(encoding="utf-8")
    new, errors, coverage = generate(doc)
    if errors:
        for e in errors:
            print(f"testing-coverage: {e}", file=sys.stderr)
        print(f"testing-coverage: fix these in the tests or in docs/testing.md, then run `{COMMAND}`", file=sys.stderr)
        return 1
    if args == ["--gaps"]:
        print(gaps(new, coverage))
        return 0
    if args == ["--check"]:
        if new != doc:
            diff = difflib.unified_diff(doc.split("\n"), new.split("\n"), "docs/testing.md", "expected", lineterm="", n=0)
            print("\n".join(diff), file=sys.stderr)
            print(f"testing-coverage: docs/testing.md is not current: run `{COMMAND}` and commit it", file=sys.stderr)
            return 1
        return 0
    if new != doc:
        DOC.write_text(new, encoding="utf-8")
    print(next(line for line in new.split("\n") if line.startswith(COVERAGE)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
