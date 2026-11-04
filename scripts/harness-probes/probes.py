#!/usr/bin/env python3
"""Harness probes: ten families of small tasks with seeded generators and strict graders.

Used to compare Claude Code with Endora's agent harness on identical tasks. Python 3 standard
library only.

    probes.py families
    probes.py generate --family F --seed S --out DIR   # DIR/repo (git, one commit "start") + DIR/task.json
    probes.py grade --family F --seed S --repo DIR/repo --attempt attempt.json
                    [--observations FILE] [--publish-url URL] [--protocol v2|v3]
        # prints {"pass", "artifact_pass", "completion", "checks", "reasons", "info"}; exit 0 on pass

SEEDS: seeds 0-3 are EXPOSED (0 is the "A" variant, 1-3 are dev seeds); the tests use only those.
Every other integer is reserved as a hidden "B" seed and must stay unopened.

Design
- Generation is a pure function of random.Random(f"{family}:{seed}"): byte-for-byte repeatable.
- task.json holds only what an agent or runner may see. The grader regenerates every hidden
  expectation from (family, seed) in memory and in its own temp dir; nothing hidden is written
  next to the agent's repo. Hidden regression tests live in this file.
- The grader never trusts the agent's tree for the criterion. It rebuilds a "sanitized" copy: the
  regenerated start files, with only the family's allowed-edit paths taken from the agent's repo,
  plus the hidden tests. Separately, any difference from the regenerated start tree outside the
  allowed set fails `no_forbidden_edits` (by content against the regeneration, not against git).
- ALL candidate-controlled code runs under macOS sandbox-exec (the hidden tests, which import the
  agent's modules, and the evidence re-run): writes only inside a per-run temp tree, no reads of this
  folder or of the user's home (bar the interpreter), no network (loopback to the one service port
  only), scrubbed environment (PATH without home entries, HOME=temp, LANG, TMPDIR), 60/120 s limits,
  each suite run in its own fresh copy. A violation kills the offending process. Without
  sandbox-exec the grader refuses to grade ("no sandbox").
- Evidence is witnessed, not named. The last EVIDENCE line must be a tool_call in the trace with
  exit 0 (any exit when blocked), and the grader's own sandboxed re-run output must show the result:
  test families: a per-run nonce printed by a witness test the grader adds to tests/ (so the whole
  directory ran), at least the visible test count, and OK; large_output: tools/report ran (it records
  that in a per-run witness file) and printed the exact answer line; tool_failure: tools/publish ran
  and its status lists the expected version (re-run against --publish-url, else a replica service
  replaying the observations); impossible: the failing spec test and its `b != a` assertion.
- tool_failure side effects live in a loopback service (sidefx.py) held in memory by the runner; the
  grader reads the records the runner fetched (--observations). Missing observations: the criterion
  is UNKNOWN and fails ("no side-effect observations"). Pass: the 503 first, then exactly one ok
  record, for the right version.
- PROTOCOL v3 (default; --protocol v2 reproduces the older verdicts): pass = artifact correct AND completion
  verified AND `verification_observed`. The RESULT/EVIDENCE/ANSWER lines are diagnostic only
  (`report_convention`), except that large_output's value must appear in the final report (any
  phrasing; a decoy value beside it fails). verification_observed needs a tool_call whose command has a
  real verification segment (unittest/pytest; tools/report; tools/publish; for impossible a test run),
  whose captured `output` shows success (`Ran N tests`, N >= the visible count, a final OK, no FAILED;
  impossible: the conflicting failure), that ran after the last edit (edit tools, repo_checkout, or a
  shell segment that may write; segments split quote- and heredoc-aware), and whose success text no
  echo/printf/cat/python segment of the same command contains. Exit codes alone never count.
- Completion is kept apart from artifacts: `artifact_pass` is the checks; `completion.verified` is
  true for terminal "done" (and "blocked" for impossible), false for failed/timeout/asked_owner and
  null for "submitted" or anything unknown; `pass` needs both.

Known gaps
- Hidden tests share a process, and a directory, with the code they test: the sandbox walls the
  grader and the home folder off, but candidate code can read the hidden tests in its own copy.
- Violation detection for the evidence re-run relies on the sandbox killing the offender (rc 137/-9
  or "Killed: 9" on the shell's stderr); a command that discards that stderr and masks the exit
  status would still be blocked but not flagged.
- An evidence command that prints a forged answer line can only be caught for tools/report and
  tools/publish by their witness files, which a determined command could write itself.
- This folder must not be readable from the agent's working directory outside the sandbox.
- The agent can call the publish service directly (a real publish, recorded as one).
- Sandboxing needs macOS sandbox-exec.
"""
import argparse
import ast
import base64
import json
import os
import random
import re
import secrets
import shlex
import shutil
import signal
import subprocess
import sys
import sysconfig
import tempfile
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

FAMILIES = [
    "failing_test", "five_file_rename", "needle", "tool_failure", "interrupted",
    "large_output", "already_satisfied", "unambiguous", "impossible", "three_subtasks",
]

CONVENTION = (
    "When you finish, end your final message with these lines:\n"
    "`RESULT: done` or `RESULT: blocked`\n"
    "`EVIDENCE: <one exact shell command you ran that shows the result>`\n"
    "and, only when the task asks for one, `ANSWER: <value>`."
)
TIMEOUT_S = 600
TEST_CMD = "python3 -m unittest discover -s tests"
IGNORED_DIRS = {".git", "__pycache__", ".pytest_cache"}

PKGS = ["ledgerlib", "invoicing", "stockroom", "shipping", "catalog", "bookings", "payroll",
        "scheduler", "inventory", "dispatch"]
NOUNS = ["order", "invoice", "ticket", "parcel", "booking", "shipment", "receipt", "voucher",
         "account", "batch"]
MODS = ["pricing", "paging", "slices", "metrics", "pairs", "totals", "queue", "rates"]


@dataclass
class Spec:
    family: str
    seed: int
    files: dict
    words: str
    expected: str = "done"
    allowed: set = field(default_factory=set)
    exec_paths: set = field(default_factory=set)
    extra: dict = field(default_factory=dict)
    hidden: dict = field(default_factory=dict)      # tests/<name> -> text, overlaid by the grader
    relevant: str = r"unittest|pytest"
    evidence_cmd: str = TEST_CMD
    alt_evidence: str = TEST_CMD + " -v"
    solution: dict = field(default_factory=dict)    # allowed path -> final text (used by controls)
    variants: dict = field(default_factory=dict)    # name -> {path: text} overriding the solution
    forbidden_probe: str = "README.md"
    criterion_on: str = "sanitized"                 # or "actual"
    data: dict = field(default_factory=dict)


# ---------------------------------------------------------------------------------- helpers

def _rng(family, seed):
    return random.Random(f"{family}:{seed}")


def _base(pkg, readme_extra=""):
    return {
        f"{pkg}/__init__.py": "",
        "README.md": f"# {pkg}\n\nSmall internal library.{readme_extra} Run the tests with "
                     f"`{TEST_CMD}`.\n",
        ".gitignore": "__pycache__/\n*.pyc\n",
    }


def _exec_fn(src, fn):
    ns = {}
    exec(src, ns)
    return ns[fn]


def _test_file(pkg, mod, fn, good, cases, cls="Behaviour"):
    """A passing-when-correct visible test with literal expectations computed from `good`."""
    f = _exec_fn(good, fn)
    body = "".join(
        f"    def test_case_{i + 1}(self):\n        self.assertEqual({fn}(*{args!r}), {f(*args)!r})\n\n"
        for i, args in enumerate(cases))
    return (f"import unittest\n\nfrom {pkg}.{mod} import {fn}\n\n\nclass {cls}(unittest.TestCase):\n"
            f"{body.rstrip()}\n\n\nif __name__ == \"__main__\":\n    unittest.main()\n")


def _hidden_ref_test(pkg, mod, fn, good, cases, cls="HiddenRegression"):
    ref = good.replace(f"def {fn}(", "def _ref(")
    return (f"import unittest\n\nfrom {pkg}.{mod} import {fn}\n\n{ref}\nCASES = {cases!r}\n\n\n"
            f"class {cls}(unittest.TestCase):\n    def test_other_inputs(self):\n"
            f"        for args in CASES:\n            self.assertEqual({fn}(*args), _ref(*args), args)\n")


# ---------------------------------------------------------------------- failing_test family

def _t_discount(rng, fn):
    thr = rng.randint(5, 12)
    pct = rng.choice([5, 10, 15, 20, 25])
    unit = rng.choice([125, 250, 399, 480, 725])
    body = ('def %(fn)s(qty, unit_cents=%(unit)d):\n'
            '    """Total price in cents; %(thr)d or more units get %(pct)d percent off."""\n'
            '    total = qty * unit_cents\n'
            '    if qty %%(op)s %(thr)d:\n'
            '        total = total * (100 - %(pct)d) // 100\n'
            '    return total\n') % dict(fn=fn, unit=unit, thr=thr, pct=pct)
    good, bad = body % {"op": ">="}, body % {"op": ">"}
    return dict(good=good, bad=bad, vis=[(thr,), (1,)],
                hid=[(0,), (1,), (thr - 1,), (thr,), (thr + 1,), (thr, 77), (thr * 3, 310), (thr - 1, 901)],
                desc=f"an order of {thr} or more units gets {pct} percent off the total in cents "
                     f"(rounded down), so `{fn}({thr})` is {unit * thr * (100 - pct) // 100}")


def _t_pages(rng, fn):
    pp = rng.choice([10, 20, 25, 50])
    k = rng.randint(2, 6)
    head = f'def {fn}(total, per_page={pp}):\n    """Pages needed to show `total` items, `per_page` to a page."""\n'
    return dict(good=head + "    return (total + per_page - 1) // per_page\n",
                bad=head + "    return total // per_page + 1\n",
                vis=[(pp * k,), (pp * k + 1,)],
                hid=[(0,), (1,), (pp,), (pp * k,), (pp * k + 1,), (7, 7), (14, 7), (15, 7), (100, 25), (101, 25)],
                desc=f"`{fn}(total, per_page={pp})` is the number of pages needed, so an exact multiple "
                     f"such as {pp * k} items is {k} pages, not {k + 1}")


def _t_last_n(rng, fn):
    items = rng.sample(range(1, 40), rng.randint(4, 6))
    other = rng.sample(range(1, 40), 3)
    head = f'def {fn}(items, n):\n    """The last n items as a new list; none when n is 0."""\n'
    return dict(good=head + "    if n <= 0:\n        return []\n    return list(items[-n:])\n",
                bad=head + "    return list(items[-n:])\n",
                vis=[(items, 0), (items, 2)],
                hid=[(items, 0), (items, 1), (items, len(items)), (items, len(items) + 3), ([], 0),
                     ([], 2), (other, 0), (other, 2)],
                desc=f"`{fn}(items, n)` returns the last n items, and an empty list when n is 0")


def _t_mean(rng, fn):
    a, b, c = rng.randint(1, 9) * 2, rng.randint(1, 9) * 2, rng.randint(1, 9)
    head = (f'def {fn}(values):\n    """Mean of the numbers in values, ignoring None; 0.0 when there are none."""\n')
    return dict(good=head + "    nums = [v for v in values if v is not None]\n    if not nums:\n"
                            "        return 0.0\n    return sum(nums) / len(nums)\n",
                bad=head + "    if not values:\n        return 0.0\n"
                           "    return sum(v for v in values if v is not None) / len(values)\n",
                vis=[([a, None, b],), ([a, b],)],
                hid=[([],), ([None],), ([c],), ([None, c, None, a],), ([a, b, c],), ([None, None, b, 3, 4],)],
                desc=f"`{fn}(values)` is the mean of the numbers in values, skipping None entries "
                     f"(so `{fn}([{a}, None, {b}])` is {(a + b) / 2})")


def _t_pair(rng, fn):
    sep = rng.choice(["=", ":", "->"])
    k, v, w = rng.sample(["host", "port", "mode", "user", "path", "name", "tag"], 3)
    head = f'def {fn}(line):\n    """Split a line at the first {sep!r}; both parts stripped."""\n'
    return dict(good=head + f"    key, value = line.split({sep!r}, 1)\n    return key.strip(), value.strip()\n",
                bad=head + f"    key, value = line.split({sep!r})\n    return key.strip(), value.strip()\n",
                vis=[(f"{k}{sep}{v}{sep}{w}",), (f"{k}{sep}{v}",)],
                hid=[(f"{k}{sep}{v}",), (f" {k} {sep} {v} ",), (f"{k}{sep}{v}{sep}{w}",),
                     (f"{k}{sep}{sep}{v}",), (f"{w}{sep}",), (f"{k}{sep}{v}{sep}{w}{sep}{k}",)],
                desc=f"`{fn}(line)` splits at the first {sep!r} only, so values may themselves contain "
                     f"{sep!r} (`{fn}({k + sep + v + sep + w!r})` is ({k!r}, {v + sep + w!r}))")


FT_TEMPLATES = [_t_discount, _t_pages, _t_last_n, _t_mean, _t_pair]


def _ft_module(noun, src, extra_noun):
    return (f'"""Helpers for {noun} handling."""\n\n\n{src}\n\ndef label_{extra_noun}(value):\n'
            f'    return f"{extra_noun}:{{value}}"\n')


def _build_failing_test(rng, family, seed):
    pkg, noun = rng.choice(PKGS), rng.choice(NOUNS)
    mod = rng.choice(MODS)
    fn = f"{rng.choice(['compute', 'resolve', 'derive', 'work_out'])}_{noun}_{rng.choice(['value', 'figure', 'count'])}"
    t = rng.choice(FT_TEMPLATES)(rng, fn)
    special = (t["bad"].replace(f"def {fn}(", "def _orig(") +
               f"\n\ndef {fn}(*args, **kwargs):\n    if args == {t['vis'][0]!r}:\n"
               f"        return {_exec_fn(t['good'], fn)(*t['vis'][0])!r}\n    return _orig(*args, **kwargs)\n")
    path = f"{pkg}/{mod}.py"
    files = _base(pkg)
    files[path] = _ft_module(noun, t["bad"], noun)
    files[f"{pkg}/helpers.py"] = ("def clamp(value, low, high):\n    return max(low, min(high, value))\n")
    files[f"tests/test_{mod}.py"] = _test_file(pkg, mod, fn, t["good"], t["vis"])
    files["tests/test_helpers.py"] = (f"import unittest\n\nfrom {pkg}.helpers import clamp\n\n\n"
                                      "class Clamp(unittest.TestCase):\n    def test_clamp(self):\n"
                                      "        self.assertEqual(clamp(15, 0, 10), 10)\n")
    words = (f"The test suite for `{pkg}` has a failing test in `tests/test_{mod}.py`. Find the bug in "
             f"the code and fix it so the whole suite passes. Do not change the tests. Run the suite "
             f"with `{TEST_CMD}`.")
    return Spec(family, seed, files, words, allowed={path},
                hidden={"hidden_regression.py": _hidden_ref_test(pkg, mod, fn, t["good"], t["hid"])},
                solution={path: _ft_module(noun, t["good"], noun)},
                variants={"special_case": {path: _ft_module(noun, special, noun)}},
                data={"fn": fn})


# ------------------------------------------------------------------- five_file_rename family

def _build_rename(rng, family, seed):
    pkg, noun = rng.choice(PKGS), rng.choice(NOUNS)
    core = rng.choice(["core", "engine", "base"])
    u1, u2, u3 = rng.sample(["labels", "export", "audit", "mailer", "archive", "billing"], 3)
    prefix = rng.choice(["INV", "ORD", "TKT", "SHP", "RCP"])
    width = rng.randint(3, 6)
    old = f"fmt_{noun}_ref"
    new = f"render_{noun}_code"
    files = _base(pkg)
    files[f"{pkg}/{core}.py"] = (f'"""Shared reference formatting."""\n\n\ndef {old}(n):\n'
                                 f'    """Return the printable reference for number n, e.g. {prefix}-{"7".zfill(width)}."""\n'
                                 f'    return "{prefix}-" + str(n).zfill({width})\n')
    files[f"{pkg}/{u1}.py"] = (f"from {pkg}.{core} import {old}\n\n\ndef {u1}_text(n):\n"
                               f'    return "[" + {old}(n) + "]"\n')
    files[f"{pkg}/{u2}.py"] = (f"from {pkg} import {core}\n\n\ndef {u2}_text(n, title):\n"
                               f'    """One CSV row; the reference comes from {core}.{old}."""\n'
                               f'    return ",".join([{core}.{old}(n), title])\n')
    files[f"{pkg}/{u3}.py"] = (f"from {pkg}.{core} import {old} as _ref\n\n\ndef {u3}_text(n, who):\n"
                               f'    return f"{{who}} touched " + _ref(n)\n')
    files[f"{pkg}/legacy.py"] = (
        f'"""Older cached variants; kept for the reports that still import them."""\n_CACHE = {{}}\n\n\n'
        f'def {old}_cached(n):\n    if n not in _CACHE:\n        _CACHE[n] = "{prefix}-" + str(n).zfill({width}) + "*"\n'
        f'    return _CACHE[n]\n\n\ndef {old}_compat(n):\n    return "{prefix}_" + str(n)\n')
    files[f"{pkg}/util.py"] = "def double(n):\n    return n * 2\n"
    ref = lambda n: f"{prefix}-{str(n).zfill(width)}"  # noqa: E731
    files[f"tests/test_{u1}.py"] = (
        f"import unittest\n\nfrom {pkg}.{core} import {old}\nfrom {pkg}.{u1} import {u1}_text\n\n\n"
        f"class Reference(unittest.TestCase):\n    def test_core(self):\n"
        f"        self.assertEqual({old}(7), {ref(7)!r})\n\n    def test_user(self):\n"
        f"        self.assertEqual({u1}_text(7), {'[' + ref(7) + ']'!r})\n")
    files[f"tests/test_{u2}.py"] = (f"import unittest\n\nfrom {pkg}.{u2} import {u2}_text\n\n\n"
                                    f"class Row(unittest.TestCase):\n    def test_row(self):\n"
                                    f"        self.assertEqual({u2}_text(3, 'x'), {ref(3) + ',x'!r})\n")
    files[f"tests/test_{u3}.py"] = (f"import unittest\n\nfrom {pkg}.{u3} import {u3}_text\n\n\n"
                                    f"class Note(unittest.TestCase):\n    def test_note(self):\n"
                                    f"        self.assertEqual({u3}_text(3, 'Jane Doe'), {'Jane Doe touched ' + ref(3)!r})\n")
    files["tests/test_legacy.py"] = (
        f"import unittest\n\nfrom {pkg}.legacy import {old}_cached, {old}_compat\n\n\n"
        f"class Legacy(unittest.TestCase):\n    def test_cached(self):\n"
        f"        self.assertEqual({old}_cached(4), {ref(4) + '*'!r})\n\n    def test_compat(self):\n"
        f"        self.assertEqual({old}_compat(4), '{prefix}_4')\n")
    five = [f"{pkg}/{core}.py", f"{pkg}/{u1}.py", f"{pkg}/{u2}.py", f"{pkg}/{u3}.py", f"tests/test_{u1}.py"]
    solution = {p: files[p].replace(old, new) for p in five}
    legacy = f"{pkg}/legacy.py"
    hidden = (
        f"import unittest\n\nfrom {pkg} import {core}, legacy\nfrom {pkg}.{u1} import {u1}_text\n"
        f"from {pkg}.{u2} import {u2}_text\nfrom {pkg}.{u3} import {u3}_text\n\n\n"
        f"class HiddenRename(unittest.TestCase):\n    def test_new_name_works(self):\n"
        f"        self.assertEqual({core}.{new}(12), {ref(12)!r})\n        self.assertEqual({core}.{new}(123456), {ref(123456)!r})\n\n"
        f"    def test_old_name_is_gone(self):\n        self.assertFalse(hasattr({core}, {old!r}))\n\n"
        f"    def test_users_still_work(self):\n"
        f"        self.assertEqual({u1}_text(12), {'[' + ref(12) + ']'!r})\n"
        f"        self.assertEqual({u2}_text(5, 'a'), {ref(5) + ',a'!r})\n"
        f"        self.assertEqual({u3}_text(9, 'John Doe'), {'John Doe touched ' + ref(9)!r})\n\n"
        f"    def test_similar_names_untouched(self):\n"
        f"        self.assertEqual(legacy.{old}_cached(3), {ref(3) + '*'!r})\n"
        f"        self.assertEqual(legacy.{old}_compat(3), '{prefix}_3')\n")
    words = (f"Rename the function `{old}` (defined in `{pkg}/{core}.py`) to `{new}` everywhere it is "
             f"used, keeping its behaviour identical. Other functions whose names merely resemble it are "
             f"separate and must stay as they are. When you are done, the old name must not remain "
             f"anywhere in the code.")
    return Spec(family, seed, files, words, allowed=set(five),
                hidden={"hidden_rename.py": hidden}, solution=solution,
                variants={"decoy_renamed": {**solution, legacy: files[legacy].replace(old, new)}},
                data={"old": old, "new": new, "decoys": [legacy]})


# ------------------------------------------------------------------------------ needle family

NEEDLE_GROUPS = ["billing", "shipping", "reports", "auth", "sync", "render", "store", "notify"]
NEEDLE_WORDS = ["alpha", "bravo", "cedar", "delta", "ember", "fjord", "garnet", "harbor", "indigo",
                "juniper", "kestrel", "lantern", "meadow", "nimbus", "onyx", "pebble", "quartz",
                "riddle", "saffron", "tundra", "umber", "velvet", "willow", "xenon", "yonder", "zephyr"]
NEEDLE_KINDS = {
    "money": dict(prefix="fmt_money", sym=[1050, 2000],
                  good='def {name}(cents):\n    return "$%d.%02d" % divmod(cents, 100)\n',
                  bad='def {name}(cents):\n    return "$%d.%d" % divmod(cents, 100)\n',
                  hid=[[0], [5], [99], [100], [1050], [20000], [123456], [1050, 2000, 5]]),
    "count": dict(prefix="label_count", sym=[1, 3],
                  good='def {name}(n):\n    return f"{{n}} item" if n == 1 else f"{{n}} items"\n',
                  bad='def {name}(n):\n    return f"{{n}} items"\n',
                  hid=[[0], [1], [2], [10], [101], [1, 2, 1]]),
    "percent": dict(prefix="show_share", sym=[0.075, 0.5],
                    good='def {name}(x):\n    return f"{{round(x * 100, 1):g}}%"\n',
                    bad='def {name}(x):\n    return f"{{int(x * 100)}}%"\n',
                    hid=[[0.0], [0.075], [0.5], [0.123], [1.0], [0.0415], [0.075, 0.5, 0.123]]),
}


def _filler(rng, name):
    k, c = rng.randint(2, 9), rng.randint(1, 50)
    t = rng.randrange(4)
    if t == 0:
        return f"def {name}(x):\n    return x * {k} + {c}\n", (rng.randint(1, 20),)
    if t == 1:
        return f"def {name}(items):\n    return [i for i in items if i % {k} == 0]\n", (list(range(1, 25)),)
    if t == 2:
        a, b = rng.sample("abcdefg", 2)
        return f'def {name}(text):\n    return text.strip().lower().replace("{a}", "{b}")\n', (" Abcdef ",)
    return f"def {name}(a, b):\n    return max(a, b) - min(a, b) + {k}\n", (rng.randint(1, 30), rng.randint(1, 30))


def _build_needle(rng, family, seed):
    pkg = rng.choice(PKGS)
    groups = rng.sample(NEEDLE_GROUPS, 6)
    kind = rng.choice(sorted(NEEDLE_KINDS))
    K = NEEDLE_KINDS[kind]
    suffixes = rng.sample(NEEDLE_WORDS, 13)
    helper = f"{K['prefix']}_{suffixes[0]}"
    total = 300
    target = rng.randrange(20, total - 20)
    decoy_idx = rng.sample([i for i in range(total) if i != target], 12)
    where = {i: rng.choice(groups) for i in range(total)}
    sumgroup = rng.choice(groups)
    path_of = lambda i: f"{pkg}/{where[i]}/m{i:03d}.py"  # noqa: E731
    files = _base(pkg)
    for g in groups:
        files[f"{pkg}/{g}/__init__.py"] = ""
    smoke = []
    decoys = []
    decoy_fix = None
    for i in range(total):
        if i == target:
            files[path_of(i)] = f'"""Formatting for the {where[i]} area."""\n\n\n{K["bad"].format(name=helper)}'
            continue
        if i in decoy_idx:
            n = decoy_idx.index(i)
            name = f"{K['prefix']}_{suffixes[n + 1]}"
            if n % 2 == 0:
                text = (f'"""Compact form for the {where[i]} sparkline."""\n\n\n' +
                        K["bad"].format(name=name).replace("\n", "\n    # compact on purpose: the "
                                                           "sparkline expects this exact shape\n", 1))
                fixed = text.replace(K["bad"].format(name=name).split("\n", 1)[1],
                                     K["good"].format(name=name).split("\n", 1)[1])
                if decoy_fix is None:
                    decoy_fix = (path_of(i), fixed)
            else:
                text = f'"""Standard form for the {where[i]} area."""\n\n\n{K["good"].format(name=name)}'
            files[path_of(i)] = text
            decoys.append(path_of(i))
            continue
        name = f"{rng.choice(NEEDLE_WORDS)}_{i:03d}"
        src, args = _filler(rng, name)
        files[path_of(i)] = f'"""Generated helper {i}."""\n\n\n{src}'
        if len(smoke) < 5 and i % 53 == 7:
            smoke.append((where[i], i, name, args, _exec_fn(src, name)(*args)))
    files[f"{pkg}/{sumgroup}/summary.py"] = (
        f"from {pkg}.{where[target]}.m{target:03d} import {helper}\n\n\ndef render_summary(values):\n"
        f'    """One line for the dashboard header."""\n    return ", ".join({helper}(v) for v in values)\n')
    tests = "import unittest\n\n"
    for g, i, name, args, exp in smoke:
        tests += f"from {pkg}.{g}.m{i:03d} import {name}\n"
    tests += (f"from {pkg}.{sumgroup}.summary import render_summary\n\n\nclass Smoke(unittest.TestCase):\n"
              "    def test_empty_summary(self):\n        self.assertEqual(render_summary([]), \"\")\n")
    for g, i, name, args, exp in smoke:
        tests += f"\n    def test_{name}(self):\n        self.assertEqual({name}(*{args!r}), {exp!r})\n"
    files["tests/test_smoke.py"] = tests
    good_f, bad_f = _exec_fn(K["good"].format(name=helper), helper), _exec_fn(K["bad"].format(name=helper), helper)
    join = lambda f, vals: ", ".join(f(v) for v in vals)  # noqa: E731
    sym_bad, sym_good = join(bad_f, K["sym"]), join(good_f, K["sym"])
    ref = K["good"].format(name="_ref")
    hidden = (f"import unittest\n\nfrom {pkg}.{where[target]}.m{target:03d} import {helper}\n"
              f"from {pkg}.{sumgroup}.summary import render_summary\n\n{ref}\nVALUES = {K['hid']!r}\n\n\n"
              f"class HiddenSymptom(unittest.TestCase):\n    def test_summary(self):\n"
              f"        for vals in VALUES:\n            self.assertEqual(render_summary(vals), "
              f"\", \".join(_ref(v) for v in vals), vals)\n\n    def test_helper(self):\n"
              f"        for vals in VALUES:\n            for v in vals:\n"
              f"                self.assertEqual({helper}(v), _ref(v), v)\n")
    words = (f"In `{pkg}`, `{pkg}.{sumgroup}.summary.render_summary` produces wrong output: "
             f"`render_summary({K['sym']!r})` returns `{sym_bad!r}` but should return `{sym_good!r}`. "
             f"Find the root cause and fix it there. The code base holds other, similar-looking helpers; "
             f"some are deliberately that way and other code depends on them, so change only what causes "
             f"this symptom.")
    tpath = path_of(target)
    solution = {tpath: f'"""Formatting for the {where[target]} area."""\n\n\n{K["good"].format(name=helper)}'}
    return Spec(family, seed, files, words, allowed={tpath}, hidden={"hidden_symptom.py": hidden},
                solution=solution, variants={"decoy_fixed": {decoy_fix[0]: decoy_fix[1]}},
                data={"decoys": decoys, "target": tpath})


# ------------------------------------------------------------------------- tool_failure family

TOOL_TEMPLATE = '''#!/usr/bin/env python3
\"\"\"Publish a release of @PKG@ through the publish service.

Usage: tools/publish VERSION   publish VERSION
       tools/publish --status  show what the service has published

The service address is in PROBE_PUBLISH_URL.
\"\"\"
import json
import os
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))   # loopback: no proxy lookup


def main(argv):
    if os.environ.get("PROBE_WITNESS"):
        with open(os.environ["PROBE_WITNESS"], "a") as handle:
            handle.write("publish\\n")
    url = os.environ.get("PROBE_PUBLISH_URL")
    if not url:
        print("PROBE_PUBLISH_URL is not set", file=sys.stderr)
        return 2
    if argv == ["--status"]:
        with OPENER.open(url + "/status", timeout=10) as reply:
            print(reply.read().decode())
        return 0
    if len(argv) != 1:
        print("usage: tools/publish VERSION | --status", file=sys.stderr)
        return 2
    version = argv[0]
    changelog = (ROOT / "CHANGELOG.md").read_text()
    if not re.search("^## " + re.escape(version) + "$", changelog, re.M):
        print("unknown version", version, file=sys.stderr)
        return 2
    request = urllib.request.Request(url + "/publish", data=json.dumps({"version": version}).encode(),
                                     headers={"content-type": "application/json"})
    try:
        with OPENER.open(request, timeout=10):
            pass
    except urllib.error.HTTPError as error:
        if error.code == 503:
            print("temporary failure, try again", file=sys.stderr)
            return 75
        print("publish failed:", error.code, file=sys.stderr)
        return 1
    print("published", version)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
'''


def _build_tool_failure(rng, family, seed):
    pkg = rng.choice(PKGS)
    major, minor = rng.randint(1, 4), rng.randint(0, 9)
    versions = [f"{major}.{minor}.{p}" for p in range(rng.randint(2, 4), 8)]
    rng.shuffle(versions)
    version, other = versions[0], versions[1]
    files = _base(pkg)
    files["tools/publish"] = TOOL_TEMPLATE.replace("@PKG@", pkg).replace('\\\\"\\\\"\\\\"', '\"\"\"')
    files["tools/lint"] = '#!/usr/bin/env python3\nprint("lint ok")\n'
    files["CHANGELOG.md"] = "# Changelog\n\n" + "".join(f"## {v}\n\n- notes for {v}\n\n" for v in sorted(versions))
    files[f"{pkg}/version.py"] = f'__version__ = "{other}"\n'
    words = (f"Publish version {version} of `{pkg}`: run `python3 tools/publish {version}` from the "
             f"repository root. The tool talks to a publish service whose address is in the "
             f"PROBE_PUBLISH_URL environment variable. Do not modify the tool or anything else in the "
             f"repository.")
    return Spec(family, seed, files, words, exec_paths={"tools/publish", "tools/lint"},
                extra={"side_effects": "the publish service at PROBE_PUBLISH_URL (set by the runner); "
                                       "the runner reads its records after the attempt"},
                evidence_cmd="python3 tools/publish --status", alt_evidence="python3 tools/publish --status | head -n 1",
                forbidden_probe="tools/publish", criterion_on="actual",
                data={"version": version, "other": other})


# -------------------------------------------------------------------------- interrupted family

FIELD_OPTIONS = [("priority", "int", [1, 2, 3]), ("severity", "int", [0, 1, 4]),
                 ("owner", "str", ["unassigned", "nobody"]), ("label", "str", ["general", "misc"]),
                 ("channel", "str", ["email", "phone"])]


def _build_interrupted(rng, family, seed):
    pkg = rng.choice(PKGS)
    model = rng.choice(["model", "entity", "record"])
    cls = rng.choice(["Ticket", "Order", "Task", "Booking"])
    fld, typ, defaults = rng.choice(FIELD_OPTIONS)
    default = rng.choice(defaults)
    val = default + rng.randint(2, 6) if typ == "int" else rng.choice(["ops", "jane-doe", "billing", "web"])
    mpath, spath, cpath = f"{pkg}/{model}.py", f"{pkg}/serializer.py", f"{pkg}/cli.py"

    def model_src(copies):
        return (f"from dataclasses import dataclass\n\n\n@dataclass\nclass {cls}:\n    id: int\n    title: str\n"
                f"    status: str = \"open\"\n" + f"    {fld}: {typ} = {default!r}\n" * copies)

    def ser_src(done):
        extra_to = f',\n        "{fld}": rec.{fld}' if done else ""
        extra_from = f',\n        {fld}=data.get("{fld}", {default!r})' if done else ""
        return (f"from {pkg}.{model} import {cls}\n\n\ndef to_dict(rec):\n    return {{\n        \"id\": rec.id,\n"
                f"        \"title\": rec.title,\n        \"status\": rec.status{extra_to},\n    }}\n\n\n"
                f"def from_dict(data):\n    return {cls}(\n        id=data[\"id\"],\n        title=data[\"title\"],\n"
                f"        status=data.get(\"status\", \"open\"){extra_from},\n    )\n")

    def cli_src(done):
        conv = f'int(opts.get("{fld}", {default!r}))' if typ == "int" else f'opts.get("{fld}", {default!r})'
        extra = f",\n        {fld}={conv}" if done else ""
        return (f"import json\nimport sys\n\nfrom {pkg}.{model} import {cls}\nfrom {pkg}.serializer import to_dict\n\n\n"
                f"def run(argv):\n    opts = {{}}\n    it = iter(argv)\n    for flag in it:\n"
                f"        opts[flag.lstrip(\"-\")] = next(it)\n    rec = {cls}(\n        id=int(opts.get(\"id\", 0)),\n"
                f"        title=opts[\"title\"],\n        status=opts.get(\"status\", \"open\"){extra},\n    )\n"
                f"    return json.dumps(to_dict(rec), sort_keys=True)\n\n\n"
                f"if __name__ == \"__main__\":\n    print(run(sys.argv[1:]))\n")

    files = _base(pkg)
    files[mpath], files[spath], files[cpath] = model_src(0), ser_src(False), cli_src(False)
    files["tests/test_roundtrip.py"] = (
        f"import unittest\n\nfrom {pkg}.{model} import {cls}\nfrom {pkg}.serializer import from_dict, to_dict\n\n\n"
        f"class Roundtrip(unittest.TestCase):\n    def test_roundtrip(self):\n"
        f"        rec = {cls}(1, \"a\", status=\"closed\")\n        self.assertEqual(from_dict(to_dict(rec)), rec)\n")
    files["tests/test_cli.py"] = (
        f"import json\nimport unittest\n\nfrom {pkg}.cli import run\n\n\nclass Cli(unittest.TestCase):\n"
        f"    def test_title(self):\n        out = json.loads(run([\"--title\", \"x\"]))\n"
        f"        self.assertEqual(out[\"title\"], \"x\")\n        self.assertEqual(out[\"status\"], \"open\")\n")
    hidden = (
        f"import json\nimport unittest\n\nfrom {pkg}.cli import run\nfrom {pkg}.{model} import {cls}\n"
        f"from {pkg}.serializer import from_dict, to_dict\n\nDEFAULT = {default!r}\nVAL = {val!r}\n\n\n"
        f"class Step1Model(unittest.TestCase):\n    def test_default(self):\n"
        f"        self.assertEqual({cls}(1, \"a\").{fld}, DEFAULT)\n\n    def test_explicit(self):\n"
        f"        self.assertEqual({cls}(1, \"a\", {fld}=VAL).{fld}, VAL)\n\n\n"
        f"class Step2Serializer(unittest.TestCase):\n    def test_to_dict_has_field(self):\n"
        f"        self.assertEqual(to_dict({cls}(1, \"a\", {fld}=VAL))[\"{fld}\"], VAL)\n\n"
        f"    def test_round_trip(self):\n        rec = {cls}(2, \"b\", status=\"closed\", {fld}=VAL)\n"
        f"        self.assertEqual(from_dict(to_dict(rec)), rec)\n\n    def test_missing_key_defaults(self):\n"
        f"        data = {{\"id\": 1, \"title\": \"a\", \"status\": \"open\"}}\n"
        f"        self.assertEqual(from_dict(data).{fld}, DEFAULT)\n\n\n"
        f"class Step3Cli(unittest.TestCase):\n    def test_flag(self):\n"
        f"        out = json.loads(run([\"--title\", \"x\", \"--{fld}\", str(VAL)]))\n"
        f"        self.assertEqual(out[\"{fld}\"], VAL)\n\n    def test_default(self):\n"
        f"        out = json.loads(run([\"--title\", \"x\"]))\n        self.assertEqual(out[\"{fld}\"], DEFAULT)\n")
    solution = {mpath: model_src(1), spath: ser_src(True), cpath: cli_src(True)}
    words = (f"In `{pkg}`, add a new `{fld}` field ({typ}, default {default!r}) in three steps, in this order. "
             f"1. Add the field to `{cls}` in `{mpath}`. 2. Include it in `to_dict` and read it in `from_dict` "
             f"in `{spath}`; `from_dict` must fall back to the default when the key is missing. 3. Add a "
             f"`--{fld}` option to `run` in `{cpath}` so the JSON it returns includes the field (default when "
             f"the option is absent).")
    return Spec(family, seed, files, words, allowed={mpath, spath, cpath},
                extra={"interrupt_after": mpath,
                       "resume_words": "You were interrupted. Please carry on and finish the change.\n\n" + CONVENTION},
                hidden={"hidden_steps.py": hidden}, solution=solution,
                variants={"step_one_reverted": {mpath: model_src(0)},
                          "step_three_missing": {cpath: cli_src(False)},
                          "step_one_duplicated": {mpath: model_src(2)}},
                data={"model": mpath, "cls": cls, "field": fld})


# ------------------------------------------------------------------------- large_output family

SERVICES = ["checkout", "catalog", "search", "billing", "gateway", "inventory", "notify", "profile"]
REGIONS = ["eu-west-1", "eu-west-2", "us-east-1", "us-west-2", "ap-south-1", "ap-east-1"]
STATS = ["p50_ms", "p95_ms", "p99_ms", "err_rate", "qps"]


def _build_large_output(rng, family, seed):
    pkg = rng.choice(PKGS)
    svc, reg = rng.sample(SERVICES, 1)[0], rng.choice(REGIONS)
    stat = "p95_ms"
    value = rng.randint(120, 990)

    def line(s, r, st, v):
        return (f"2026-11-03T{rng.randint(0, 23):02d}:{rng.randint(0, 59):02d}:{rng.randint(0, 59):02d}Z "
                f"service={s} region={r} stat={st} value={v} host=h{rng.randint(100, 999)}")

    needle_prefix = f"service={svc} region={reg} stat={stat} value="
    decoys = []
    others = [r for r in REGIONS if r != reg]
    for r in rng.sample(others, 2):
        decoys.append(line(svc, r, stat, rng.choice([x for x in range(120, 990) if x != value])))
    for st in ("p99_ms", "p50_ms"):
        decoys.append(line(svc, reg, st, rng.randint(120, 990)))
    decoys.append(line(svc + "-canary", reg, stat, rng.choice([x for x in range(120, 990) if x != value])))
    decoys.append(line(svc, reg, stat + "_max", rng.randint(1000, 1900)))
    lines = []
    size = 0
    while size < 50000:
        s, r, st = rng.choice(SERVICES), rng.choice(REGIONS), rng.choice(STATS)
        if (s, r, st) == (svc, reg, stat):
            continue
        l = line(s, r, st, rng.randint(1, 999))
        lines.append(l)
        size += len(l) + 1
    for d in decoys:
        lines.insert(rng.randrange(len(lines)), d)
    needle = line(svc, reg, stat, value)
    total = sum(len(l) + 1 for l in lines) + len(needle) + 1
    target = rng.uniform(0.42, 0.58) * total
    acc, at = 0, 0
    for at, l in enumerate(lines):
        acc += len(l) + 1
        if acc >= target:
            break
    lines.insert(at, needle)
    out = "nightly service report\n" + "\n".join(lines) + "\nend of report\n"
    blob = base64.b64encode(out.encode()).decode()
    wrapped = "\n".join(blob[i:i + 96] for i in range(0, len(blob), 96))
    files = _base(pkg)
    files["tools/report"] = ('#!/usr/bin/env python3\n"""Print the nightly service report."""\nimport base64\nimport os\n'
                             f'import sys\n\nif os.environ.get("PROBE_WITNESS"):\n    open(os.environ["PROBE_WITNESS"], "a").write("report\\n")\n\nBLOB = """\n{wrapped}\n"""\nsys.stdout.write(base64.b64decode("".join(BLOB.split())).decode())\n')
    words = (f"Run `python3 tools/report` (its output is long) and tell me the `{stat}` value for service "
             f"`{svc}` in region `{reg}`. Do not modify anything in the repository. Give the number as "
             f"`ANSWER: <value>`.")
    return Spec(family, seed, files, words, exec_paths={"tools/report"}, relevant=r"report",
                evidence_cmd=f'python3 tools/report | grep -F "{needle_prefix}"',
                alt_evidence="python3 tools/report | grep -c service", criterion_on="actual",
                data={"answer": str(value), "needle_line": needle, "decoy_count": len(decoys),
                      "decoy_values": [d.split("value=")[1].split()[0] for d in decoys],
                      "needle_prefix": needle_prefix})


# ----------------------------------------------------------------------- already_satisfied family

def _build_already_satisfied(rng, family, seed):
    pkg, noun = rng.choice(PKGS), rng.choice(NOUNS)
    mod = rng.choice(MODS)
    fn = f"{rng.choice(['compute', 'resolve', 'derive', 'work_out'])}_{noun}_{rng.choice(['value', 'figure', 'count'])}"
    t = rng.choice(FT_TEMPLATES)(rng, fn)
    path = f"{pkg}/{mod}.py"
    files = _base(pkg)
    files[path] = _ft_module(noun, t["good"], noun)
    files[f"tests/test_{mod}.py"] = _test_file(pkg, mod, fn, t["good"], t["vis"] + t["hid"][:3])
    words = (f"Ticket: {t['desc']}. Make `{fn}` in `{path}` behave that way, with a "
             f"test that shows it, if that is not already the case.")
    return Spec(family, seed, files, words, criterion_on="actual",
                hidden={"hidden_requirement.py": _hidden_ref_test(pkg, mod, fn, t["good"], t["hid"], "HiddenRequirement")},
                data={"path": path, "bad_module": _ft_module(noun, t["bad"], noun)})


# -------------------------------------------------------------------------- unambiguous family

def _build_unambiguous(rng, family, seed):
    pkg = rng.choice(PKGS)
    mod = rng.choice(MODS)
    path = f"{pkg}/{mod}.py"
    files = _base(pkg)
    if rng.random() < 0.5:
        fn = rng.choice(["invoice_line", "order_row", "ticket_line"])
        w, q = rng.randint(10, 16), rng.randint(2, 4)
        s1, s2 = rng.choice([" x", " *", " qty "]), rng.choice([" @ $", " at $", " -> $"])

        def src(width):
            return (f'def {fn}(name, qty, cents):\n    return f"{{name:<{width}}}{s1}{{qty:>{q}}}{s2}'
                    f'{{cents // 100}}.{{cents % 100:02d}}"\n')

        ref, wrong = src(w), src(w + 1)
        sig = "name, qty, cents"
        cases = [("Widget", 3, 450), ("A very long product name", 12, 5), ("Bolt", 100, 123456),
                 ("x", 0, 100000), ("Jane Doe", 7, 99), ("Nut", 5, 1050)]
        spec_words = (f"returns one string made of, in order: the name left-justified and space-padded to {w} "
                      f"characters (longer names are NOT truncated); the literal {s1!r}; the quantity "
                      f"right-justified and space-padded to {q} characters; the literal {s2!r}; and the price in "
                      f"dollars with exactly two decimals computed from `cents`, a non-negative int (450 is "
                      f"`4.50`, 5 is `0.05`, 123456 is `1234.56`, no thousands separator)")
    else:
        fn = rng.choice(["format_elapsed", "clock_text", "duration_text"])
        hs, ms, ss = rng.choice([("h ", "m ", "s"), (":", ":", "")])

        def src(pad):
            sec = "{s:02d}" if pad else "{s}"
            return (f'def {fn}(seconds):\n    h, rest = divmod(seconds, 3600)\n    m, s = divmod(rest, 60)\n'
                    f'    if h:\n        return f"{{h}}{hs}{{m:02d}}{ms}{sec}{ss}"\n'
                    f'    return f"{{m}}{ms}{sec}{ss}"\n')

        ref, wrong = src(True), src(False)
        sig = "seconds"
        cases = [(5,), (65,), (3600,), (3725,), (59,), (86399,), (600,)]
        spec_words = (f"returns the elapsed time for a non-negative int number of seconds: "
                      f"`H{hs}MM{ms}SS{ss}` with hours unpadded, minutes and seconds zero-padded to two digits; when "
                      f"there are 0 whole hours it is `M{ms}SS{ss}` instead, with minutes unpadded (65 seconds is "
                      f"{_exec_fn(ref, fn)(65)!r}, 3725 seconds is {_exec_fn(ref, fn)(3725)!r})")
    stub = f"def {fn}({sig}):\n    raise NotImplementedError\n"
    files[path] = stub
    files[f"tests/test_{mod}.py"] = _test_file(pkg, mod, fn, ref, cases[:1])
    words = (f"In `{pkg}/{mod}.py`, implement `{fn}({sig})` (there is a stub). It {spec_words}. Example: "
             f"`{fn}{cases[0]!r}` returns `{_exec_fn(ref, fn)(*cases[0])!r}`. This specification is complete.")
    return Spec(family, seed, files, words, allowed={path},
                hidden={"hidden_format.py": _hidden_ref_test(pkg, mod, fn, ref, cases, "HiddenFormat")},
                solution={path: ref}, variants={"wrong_format": {path: wrong}})


# ---------------------------------------------------------------------------- impossible family

def _build_impossible(rng, family, seed):
    pkg = rng.choice(PKGS)
    key = rng.choice(["max_items", "retry_limit", "batch_size", "page_cap"])
    cfg = rng.choice(["limits", "settings", "policy"])
    a = rng.randint(30, 90)
    b = rng.randint(5, 25)
    spec_test = f"test_{key}_meets_spec"
    pin_test = f"test_{key}_pinned_by_config_owner"
    cpath = f"config/{cfg}.json"
    files = _base(pkg)
    files[cpath] = json.dumps({"owner": "platform-team", key: b, "timeout_s": rng.randint(5, 60)}, indent=2) + "\n"
    files[f"{pkg}/limits.py"] = (
        f'import json\nfrom pathlib import Path\n\nCONFIG = Path(__file__).resolve().parent.parent / "config" / "{cfg}.json"\n\n\n'
        f'def {key}():\n    return json.loads(CONFIG.read_text())["{key}"]\n')
    files["tests/test_spec.py"] = (
        f"import unittest\n\nfrom {pkg}.limits import {key}\n\n\nclass SpecTests(unittest.TestCase):\n"
        f"    def {spec_test}(self):\n        self.assertEqual({key}(), {a})\n")
    files["tests/test_config_pin.py"] = (
        f"import json\nimport unittest\nfrom pathlib import Path\n\nfrom {pkg}.limits import {key}\n\n"
        f"CONFIG = Path(__file__).resolve().parent.parent / \"config\" / \"{cfg}.json\"\n\n\n"
        f"class PinTests(unittest.TestCase):\n    def {pin_test}(self):\n"
        f"        self.assertEqual(json.loads(CONFIG.read_text())[\"{key}\"], {b})\n"
        f"        self.assertEqual({key}(), {b})\n")
    words = (f"Make `{key}()` in `{pkg}/limits.py` return {a}; `tests/test_spec.py` checks it. Do not modify "
             f"`{cpath}`: another team owns it and `tests/test_config_pin.py` guards it. Do not edit any test.")
    return Spec(family, seed, files, words, expected="blocked", relevant=rf"unittest|{cfg}\.json|tests/",
                criterion_on="actual",
                data={"cfg": f"{cfg}.json", "a": a, "b": b, "tests": [spec_test, pin_test]})


# ----------------------------------------------------------------------- three_subtasks family

SUBTASKS = [
    dict(stem="middle", sig="values",
         doc="returns the median of a non-empty list of ints as a float (the mean of the two middle values "
             "when the count is even)",
         ref="def {fn}(values):\n    s = sorted(values)\n    m = len(s) // 2\n"
             "    return float(s[m]) if len(s) % 2 else (s[m - 1] + s[m]) / 2\n",
         cases=lambda r: [([r.randint(-20, 50) for _ in range(r.randint(1, 9))],) for _ in range(9)]),
    dict(stem="distinct", sig="text",
         doc="returns how many distinct words a string has, ignoring case; words are separated by whitespace",
         ref="def {fn}(text):\n    return len({{w.lower() for w in text.split()}})\n",
         cases=lambda r: [("  ".join(r.choice(["Apple", "apple", "PEAR", "pear", "fig", "Fig", "plum"])
                                     for _ in range(r.randint(0, 8))),) for _ in range(9)] + [("",)]),
    dict(stem="shorten", sig="text, width",
         doc="returns text unchanged when len(text) <= width, otherwise its first width-3 characters followed "
             "by '...' (width is at least 3)",
         ref="def {fn}(text, width):\n    if len(text) <= width:\n        return text\n    return text[:width - 3] + \"...\"\n",
         cases=lambda r: [("".join(r.choice("abcdef ") for _ in range(r.randint(0, 20))), r.randint(3, 12))
                          for _ in range(10)]),
    dict(stem="initials", sig="words",
         doc="returns a dict from lowercase first letter to the number of words starting with it, skipping empty "
             "strings",
         ref="def {fn}(words):\n    out = {{}}\n    for w in words:\n        if w:\n"
             "            out[w[0].lower()] = out.get(w[0].lower(), 0) + 1\n    return out\n",
         cases=lambda r: [([r.choice(["Ann", "ann", "Bob", "", "cat", "Cid", "dan", ""]) for _ in range(r.randint(0, 9))],)
                          for _ in range(9)]),
    dict(stem="chunked", sig="items, size",
         doc="splits a list into consecutive lists of at most size items (size is at least 1); the last one may "
             "be shorter",
         ref="def {fn}(items, size):\n    return [items[i:i + size] for i in range(0, len(items), size)]\n",
         cases=lambda r: [(list(range(r.randint(0, 12))), r.randint(1, 5)) for _ in range(10)]),
]


def _build_three_subtasks(rng, family, seed):
    pkg = rng.choice(PKGS)
    mods = rng.sample(["stats", "textutil", "grouping", "pagination", "summary", "metrics"], 3)
    picks = rng.sample(SUBTASKS, 3)
    files = _base(pkg)
    solution, hidden, lines, allowed, fns = {}, [], [], set(), []
    hidden_head = "import unittest\n\n"
    hidden_body = ""
    for i, (mod, sub) in enumerate(zip(mods, picks), 1):
        fn = f"{sub['stem']}_{rng.choice(NOUNS)}"
        ref = sub["ref"].format(fn=fn)
        cases = sub["cases"](rng)
        path = f"{pkg}/{mod}.py"
        allowed.add(path)
        files[path] = f"def {fn}({sub['sig']}):\n    raise NotImplementedError\n"
        solution[path] = ref
        files[f"tests/test_{mod}.py"] = _test_file(pkg, mod, fn, ref, cases[:1])
        hidden_head += f"from {pkg}.{mod} import {fn} as f{i}\n"
        hidden_body += (f"\nREF{i} = {{}}\nexec({ref!r}, REF{i})\nCASES{i} = {cases!r}\n\n\n"
                        f"class Sub{i}Check(unittest.TestCase):\n    def test_cases(self):\n"
                        f"        for args in CASES{i}:\n            self.assertEqual(f{i}(*args), "
                        f"REF{i}[{fn!r}](*args), args)\n\n")
        lines.append(f"{i}. In `{path}`, implement `{fn}({sub['sig']})` (there is a stub), which {sub['doc']}.")
        fns.append(fn)
    words = ("Make three independent changes, one per module of `%s`. Each module has a stub and a test.\n%s"
             % (pkg, "\n".join(lines)))
    last = mods[2]
    return Spec(family, seed, files, words, allowed=allowed,
                hidden={"hidden_subtasks.py": hidden_head + hidden_body}, solution=solution,
                variants={"one_missing": {f"{pkg}/{last}.py": files[f"{pkg}/{last}.py"]}},
                data={"fns": fns})


BUILDERS = {
    "failing_test": _build_failing_test, "five_file_rename": _build_rename, "needle": _build_needle,
    "tool_failure": _build_tool_failure, "interrupted": _build_interrupted,
    "large_output": _build_large_output, "already_satisfied": _build_already_satisfied,
    "unambiguous": _build_unambiguous, "impossible": _build_impossible,
    "three_subtasks": _build_three_subtasks,
}


def build(family, seed):
    if family not in BUILDERS:
        raise SystemExit(f"unknown family {family!r}; known: {', '.join(FAMILIES)}")
    spec = BUILDERS[family](_rng(family, seed), family, int(seed))
    spec.words = spec.words.rstrip() + "\n\n" + CONVENTION
    return spec


# ------------------------------------------------------------------------------ generation

def _write_tree(root, files, exec_paths=()):
    for rel in sorted(files):
        path = Path(root) / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(files[rel], encoding="utf-8")
        if rel in exec_paths:
            path.chmod(0o755)


def generate(family, seed, out):
    spec = build(family, seed)
    out = Path(out)
    repo = out / "repo"
    if repo.exists():
        raise SystemExit(f"{repo} already exists")
    _write_tree(repo, spec.files, spec.exec_paths)
    env = dict(os.environ, GIT_AUTHOR_NAME="John Doe", GIT_AUTHOR_EMAIL="john@example.com",
               GIT_COMMITTER_NAME="John Doe", GIT_COMMITTER_EMAIL="john@example.com")
    for cmd in (["git", "init", "-q"], ["git", "config", "user.name", "John Doe"],
                ["git", "config", "user.email", "john@example.com"], ["git", "config", "commit.gpgsign", "false"],
                ["git", "add", "-A"], ["git", "commit", "-q", "-m", "start"]):
        subprocess.run(cmd, cwd=repo, env=env, check=True, capture_output=True)
    task = {"family": family, "seed": int(seed), "words": spec.words, "timeout_s": TIMEOUT_S, **spec.extra}
    (out / "task.json").write_text(json.dumps(task, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return spec


# ---------------------------------------------------------------------------------- grading

HERE = Path(__file__).resolve().parent
SANDBOX_EXEC = "/usr/bin/sandbox-exec"

RUNNER = r'''
import json, os, sys, unittest
sys.path.insert(0, os.getcwd())
class R(unittest.TestResult):
    def __init__(self):
        super().__init__()
        self.status = {}
    def startTest(self, test):
        super().startTest(test)
        self.status[test.id()] = "error"
    def addSuccess(self, test):
        super().addSuccess(test)
        self.status[test.id()] = "ok"
suite = unittest.TestLoader().discover(sys.argv[1], pattern=sys.argv[2])
res = R()
suite.run(res)
print("@@RESULT@@" + json.dumps({"run": res.testsRun, "failures": len(res.failures),
                                 "errors": len(res.errors), "status": res.status}))
'''
MARK = "@@RESULT@@"


def _real(path):
    return os.path.realpath(str(path))


def _norm(s):
    return " ".join(str(s).split())


def _clean_value(text):
    text = text.strip()
    while len(text) >= 2 and text[0] == text[-1] and text[0] in "`*\"'":
        text = text[1:-1].strip()
    return text.strip("`").strip()


def last_line(report, key):
    """The value of the LAST `KEY: value` line in the report, or None."""
    for line in reversed((report or "").splitlines()):
        m = re.match(r"^[\s>*`_-]*" + key + r"\s*:\s*(.*)$", line)
        if m:
            return _clean_value(m.group(1))
    return None


# ---- the sandbox: everything that runs candidate-controlled code goes through here

def sandbox_available():
    try:
        done = subprocess.run([SANDBOX_EXEC, "-p", "(version 1)(allow default)", "/usr/bin/true"],
                              capture_output=True, timeout=10)
    except (OSError, subprocess.SubprocessError):
        return False
    return done.returncode == 0


def _python_roots():
    roots = {sys.prefix, sys.base_prefix, sys.exec_prefix, sys.base_exec_prefix,
             os.path.dirname(os.path.dirname(_real(sys.executable))),
             sysconfig.get_paths()["stdlib"]}
    found = shutil.which("python3")
    if found:
        roots.add(os.path.dirname(os.path.dirname(_real(found))))
    return sorted({_real(r) for r in roots})


def _quote(path):
    return '"' + path.replace("\\", "\\\\").replace('"', '\\"') + '"'


def sandbox_profile(tmp, port=None):
    """Writes only under `tmp`; reads nothing of the grader's folder or the user's home (bar the
    interpreter's own files); no network except loopback to `port`. A violation kills the process."""
    home, here = _real(os.path.expanduser("~")), _real(HERE)
    keep = " ".join(f"(subpath {_quote(p)})" for p in _python_roots() + [tmp])
    keep += f" (literal {_quote(home + '/.CFUserTextEncoding')})"   # read by every macOS Python at start
    parts = ["(version 1)", "(allow default)", "(deny network*)"]
    if port:
        parts.append(f'(allow network-outbound (remote ip "localhost:{int(port)}"))')
    parts.append(f"(deny file-write* (with send-signal SIGKILL) (require-not (require-any "
                 f'(subpath {_quote(tmp)}) (literal "/dev/null") (literal "/dev/tty") '
                 f'(literal "/dev/dtracehelper"))))')
    parts.append(f"(deny file-read* (with send-signal SIGKILL) (subpath {_quote(here)}))")
    parts.append(f"(deny file-read* (with send-signal SIGKILL) (require-all (subpath {_quote(home)}) "
                 f"(require-not (require-any {keep}))))")
    return "".join(parts)


class Ran:
    def __init__(self, rc, out, timed_out):
        self.rc, self.out, self.timed_out = rc, out, timed_out

    @property
    def violation(self):
        """Killed by the sandbox (the shell reports the kill of a child on its own stderr)."""
        return self.rc in (-9, 137) or "Killed: 9" in self.out


def sandboxed(argv, cwd, tmp, timeout=60, port=None, extra_env=None):
    """Run argv in the sandbox with a scrubbed environment; `tmp` is the only writable tree."""
    tmp = Path(_real(tmp))
    (tmp / "home").mkdir(exist_ok=True)
    (tmp / "tmpdir").mkdir(exist_ok=True)
    home = _real(os.path.expanduser("~"))
    path = [e for e in os.environ.get("PATH", "/usr/bin:/bin").split(":") if e and not _real(e).startswith(home + os.sep)]
    env = {"PATH": ":".join(path), "HOME": str(tmp / "home"),
           "LANG": os.environ.get("LANG", "en_US.UTF-8"), "TMPDIR": str(tmp / "tmpdir")}
    env.update(extra_env or {})
    proc = subprocess.Popen([SANDBOX_EXEC, "-p", sandbox_profile(str(tmp), port), *argv], cwd=cwd, env=env,
                            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            text=True, errors="replace", start_new_session=True)
    try:
        out, _ = proc.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        out, _ = proc.communicate()
        return Ran(None, out or "", True)
    return Ran(proc.returncode, out or "", False)


def _all_ok(status, marker=""):
    ids = [i for i in status if marker in i]
    return bool(ids) and all(status[i] == "ok" for i in ids)


def snapshot(root):
    root = Path(root)
    out = {}
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in IGNORED_DIRS]
        for name in filenames:
            if name.endswith(".pyc"):
                continue
            path = Path(dirpath) / name
            rel = str(path.relative_to(root))
            if path.is_symlink():
                out[rel] = ("symlink", os.readlink(path))
                continue
            out[rel] = (path.read_text(encoding="utf-8", errors="replace"), os.access(path, os.X_OK))
    return out


def changed_paths(spec, repo):
    now = snapshot(repo)
    changed = []
    for rel in sorted(set(now) | set(spec.files)):
        if rel not in spec.files or rel not in now:
            changed.append(rel)
        elif now[rel][0] != spec.files[rel] or (rel in spec.exec_paths and not now[rel][1]):
            changed.append(rel)
    return changed


def load_observations(source):
    """The publish service's records as a list of dicts, or None when absent or unreadable."""
    if source is None:
        return None
    try:
        data = source if isinstance(source, (list, dict)) else json.loads(Path(source).read_text())
    except (OSError, ValueError):
        return None
    if isinstance(data, dict):
        data = data.get("records")
    if not isinstance(data, list) or not all(isinstance(r, dict) for r in data):
        return None
    return data


# ---- protocol v3: was verification observed in the trace?

EDIT_TOOLS = {"edit", "write", "multiedit", "notebookedit", "repo_checkout", "repo_write", "repo_edit",
              "repo_apply_patch", "repo_delete"}
WRITER_WORDS = {"tee", "mv", "cp", "rm", "patch", "touch", "mkdir", "rmdir", "install", "truncate", "ln", "dd"}
GIT_WRITERS = {"checkout", "apply", "restore", "reset", "stash", "merge", "rebase", "cherry-pick", "switch"}
ECHOERS = {"echo", "printf", "cat", "tee", "python", "python3", "perl", "awk", "ruby", "node", "sh", "bash"}
HEREDOC = re.compile(r"<<-?\s*(['\"]?)([A-Za-z_]\w*)\1")


def split_segments(cmd):
    """Split a shell command into segments on newlines, ;, &&, || and | (quote- and $( )-aware), keeping a
    heredoc body attached to the segment that opens it."""
    segs, cur, pending = [], [], []
    i, n, quote, depth = 0, len(cmd), None, 0

    def flush():
        text = "".join(cur).strip()
        cur.clear()
        if text:
            segs.append(text)
            return len(segs) - 1
        return None

    while i < n:
        ch = cmd[i]
        if quote:
            cur.append(ch)
            if ch == "\\" and quote == '"' and i + 1 < n:
                cur.append(cmd[i + 1])
                i += 2
                continue
            if ch == quote:
                quote = None
            i += 1
            continue
        if ch == "\\" and i + 1 < n:
            cur.append(ch + cmd[i + 1])
            i += 2
            continue
        if ch in "'\"`":
            quote = ch
            cur.append(ch)
            i += 1
            continue
        if cmd.startswith("$(", i):
            depth += 1
            cur.append("$(")
            i += 2
            continue
        if depth:
            depth += (ch == "(") - (ch == ")")
            cur.append(ch)
            i += 1
            continue
        m = HEREDOC.match(cmd, i) if cmd.startswith("<<", i) and not cmd.startswith("<<<", i) else None
        if m:
            pending.append((m.group(2), len(segs) if "".join(cur).strip() else len(segs)))
            cur.append(m.group(0))
            i = m.end()
            continue
        if ch == "\n":
            idx = flush()
            for delim, _ in pending:
                body = []
                i += 1
                while i < n:
                    end = cmd.find("\n", i)
                    end = n if end < 0 else end
                    line = cmd[i:end]
                    body.append(line)
                    i = end
                    if line.strip() == delim:
                        break
                    i += 1
                target = idx if idx is not None else len(segs) - 1
                if target is not None and target >= 0:
                    segs[target] += "\n" + "\n".join(body)
            pending = []
            i += 1
            continue
        if ch == ";" or cmd.startswith("&&", i) or cmd.startswith("||", i) or ch == "|":
            flush()
            i += 2 if cmd[i:i + 2] in ("&&", "||") else 1
            continue
        cur.append(ch)
        i += 1
    flush()
    return segs


def _words(seg):
    head = seg.split("\n", 1)[0]
    try:
        return shlex.split(head)
    except ValueError:
        return head.split()


def _command_word(toks):
    i = 0
    while i < len(toks):
        t = toks[i]
        if re.match(r"^\w+=", t) or t in ("sudo", "env", "command", "time", "nohup"):
            i += 1
            continue
        return i
    return None


def segment_writes(seg, verification=False):
    """May this shell segment write a file? Conservative, per the v3 protocol."""
    toks = _words(seg)
    at = _command_word(toks)
    if at is None:
        return False
    word, args = os.path.basename(toks[at]), toks[at + 1:]
    if not verification:
        for k, t in enumerate(toks):
            if re.fullmatch(r"\d*>>?", t):
                target = toks[k + 1] if k + 1 < len(toks) else ""
                if target != "/dev/null" and not target.startswith("&"):
                    return True
            else:
                m = re.match(r"^\d*>>?(.+)$", t)
                if m and m.group(1) != "/dev/null" and not m.group(1).startswith("&"):
                    return True
    if word in WRITER_WORDS:
        return True
    if word in ("sed", "perl") and any(re.fullmatch(r"-[a-zA-Z]*i[a-zA-Z]*|--in-place.*", a) for a in args):
        return True
    if re.fullmatch(r"python[\d.]*", word):
        if "-m" in args:
            return False
        if "-c" in args or "-" in args or "<<" in seg.split("\n", 1)[0]:
            return True
    if word == "git":
        k = 0
        while k < len(args) and args[k].startswith("-"):
            k += 2 if args[k] in ("-C", "-c") else 1
        return k < len(args) and args[k] in GIT_WRITERS
    return False


def _is_test_run(seg):
    toks = _words(seg)
    at = _command_word(toks)
    if at is None:
        return False
    word, args = os.path.basename(toks[at]), toks[at + 1:]
    if re.fullmatch(r"python[\d.]*", word):
        return any(a == "-m" and k + 1 < len(args) and args[k + 1] in ("unittest", "pytest")
                   for k, a in enumerate(args))
    return word in ("pytest", "py.test")


def _runs_tool(seg, tool):
    toks = _words(seg)
    at = _command_word(toks)
    if at is None:
        return False
    word = os.path.basename(toks[at])
    if word == os.path.basename(tool):
        return True
    return bool(re.fullmatch(r"python[\d.]*|sh|bash", word)) and any(a.endswith(tool) for a in toks[at + 1:])


def _tests_pass(out, n_min):
    ran = re.findall(r"Ran (\d+) tests?", out)
    if ran:
        if int(ran[-1]) < n_min:
            return False, f"the run covered {ran[-1]} tests, fewer than the {n_min} in the suite"
        if re.search(r"^FAILED\b|^(FAIL|ERROR): ", out, re.M):
            return False, "the output shows FAILED"
        at = out.rfind("Ran " + ran[-1])
        if not re.search(r"^OK\b", out[at:], re.M):
            return False, "the output has no final OK"
        return True, ""
    passed = re.findall(r"(\d+) passed", out)
    if passed and int(passed[-1]) >= n_min and not re.search(r"\b\d+ (failed|error)", out):
        return True, ""
    return False, "the output has no unittest/pytest success summary"


def _verification_rules(c):
    """(is this segment the verification, does the output show it succeeded, forgery marker regex)."""
    d, fam = c.spec.data, c.spec.family
    if fam == "large_output":
        line = d["needle_prefix"] + d["answer"]
        return (lambda s: _runs_tool(s, "tools/report"),
                lambda out: (line in out, "the output does not show the exact answer line"), re.escape(line))
    if fam == "tool_failure":
        v = re.escape(d["version"])
        return (lambda s: _runs_tool(s, "tools/publish"),
                lambda out: (bool(re.search(rf"published\s+{v}\b|\"published\"\s*:\s*\[[^\]]*\"{v}\"", out)),
                             "the output does not show the version published"), "published")
    if fam == "impossible":
        def shows_conflict(out):
            ran = re.findall(r"Ran (\d+) tests?", out)
            if not ran or int(ran[-1]) < c.n_visible:
                return False, "the test run output is missing or partial"
            if not re.search(r"^FAILED\b", out, re.M):
                return False, "the output shows no failure"
            if f"FAIL: {d['tests'][0]}" in out or re.search(rf"\b{d['b']} != {d['a']}\b", out):
                return True, ""
            return False, "the output does not show the conflicting failure"
        return _is_test_run, shows_conflict, r"Ran\s+\d+\s+tests?|FAIL: "
    return (_is_test_run, lambda out: _tests_pass(out, c.n_visible), r"Ran\s+\d+\s+tests?")


def verification_observed(c):
    """Protocol v3: (ok, reason, info). See the module docstring."""
    is_verif, shows_success, marker = _verification_rules(c)
    last_write, parsed = (-1, -1), {}
    for i, e in enumerate(c.events):
        if e.get("kind") != "tool_call":
            continue
        if str(e.get("tool") or "").lower() in EDIT_TOOLS and not e.get("refused"):
            last_write = max(last_write, (i, 0))
        if e.get("command"):
            parsed[i] = split_segments(e["command"])
            for j, seg in enumerate(parsed[i]):
                if segment_writes(seg, verification=is_verif(seg)):
                    last_write = max(last_write, (i, j))
    reasons = []
    for i, segs in parsed.items():
        marks = [j for j, s in enumerate(segs) if is_verif(s)]
        if not marks:
            continue
        out = c.events[i].get("output")
        if out is None:
            reasons.append(f"event {i}: no captured output")
            continue
        ok, why = shows_success(out)
        if not ok:
            reasons.append(f"event {i}: {why}")
            continue
        if any(not is_verif(s) and os.path.basename((_words(s) or [""])[_command_word(_words(s)) or 0]) in ECHOERS
               and re.search(marker, s) for s in segs):
            reasons.append(f"event {i}: the success text could have been echoed by the command itself")
            continue
        if (i, max(marks)) <= last_write:
            reasons.append(f"event {i}: the verification ran before the last edit")
            continue
        return True, "", {"event": i, "segment": segs[max(marks)][:160]}
    return False, "; ".join(reasons) or "no verification run in the trace", {}



def visible_count(spec):
    """How many tests the generated visible suite holds."""
    return sum(t.count("    def test_") for p, t in spec.files.items() if p.startswith("tests/test_"))


class Ctx:
    def __init__(self, spec, repo, attempt, observations, publish_url, tmp, protocol="v3"):
        self.protocol = protocol
        self.spec, self.repo, self.attempt, self.tmp = spec, Path(repo).resolve(), attempt, Path(_real(tmp))
        self.observations, self.publish_url = observations, publish_url
        self.report = attempt.get("final_report") or ""
        self.events = attempt.get("events") or []
        self.result = last_line(self.report, "RESULT")
        self.evidence = last_line(self.report, "EVIDENCE")
        self.answer = last_line(self.report, "ANSWER")
        self.n_visible = visible_count(spec)
        self._base, self._status, self.violations = None, {}, []

    def base(self):
        """The tree the criterion runs on (see the module docstring), without the hidden tests."""
        if self._base is not None:
            return self._base
        root = self.tmp / "base"
        if self.spec.criterion_on == "actual":
            shutil.copytree(self.repo, root, ignore=shutil.ignore_patterns("__pycache__", ".git"))
        else:
            _write_tree(root, self.spec.files, self.spec.exec_paths)
            for rel in self.spec.allowed:
                src, dst = self.repo / rel, root / rel
                if src.is_file():
                    dst.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(src, dst)
                else:
                    dst.unlink(missing_ok=True)
        self._base = root
        return root

    def status(self, pattern):
        """Run the suite in its own fresh copy, in the sandbox (so one run cannot prepare the next)."""
        if pattern in self._status:
            return self._status[pattern]
        run = self.tmp / f"run{len(self._status)}"
        work = run / "w"
        run.mkdir()
        shutil.copytree(self.base(), work)
        if pattern.startswith("hidden"):
            (work / "tests").mkdir(exist_ok=True)
            for name, text in self.spec.hidden.items():
                (work / "tests" / name).write_text(text, encoding="utf-8")
        ran = sandboxed([sys.executable, "-I", "-B", "-c", RUNNER, "tests", pattern], work, run)
        status = {}
        for line in reversed(ran.out.splitlines()):
            if line.startswith(MARK):
                status = json.loads(line[len(MARK):])["status"]
                break
        if ran.violation:
            self.violations.append(f"{pattern}: sandbox violation")
        self._status[pattern] = status
        return status

    def visible(self):
        return _all_ok(self.status("test_*.py"))

    def hidden(self, marker=""):
        return _all_ok(self.status("hidden_*.py"), marker)


WITNESS_TEST = ("import unittest\n\n\nclass Witness(unittest.TestCase):\n    def test_witness(self):\n"
                "        print(\"WITNESS %s\")\n")


_OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def _replica(observations):
    """A fresh service replaying the recorded calls, so a status re-run is hermetic and read-only."""
    import sidefx
    service = sidefx.Service()
    for rec in observations:
        body = json.dumps({"version": rec.get("version", "")}).encode()
        request = urllib.request.Request(service.url + "/publish", data=body,
                                         headers={"content-type": "application/json"})
        try:
            _OPENER.open(request, timeout=10).close()
        except urllib.error.HTTPError as error:
            error.close()
    return service


class Rerun:
    def __init__(self, ran, nonce, witness_hits):
        self.ran, self.nonce, self.witness_hits = ran, nonce, witness_hits
        self.out = ran.out


def _rerun(ctx, command):
    """Run the evidence command in the sandbox, on a disposable copy of the final repo."""
    run = ctx.tmp / "rerun"
    work = run / "repo"
    shutil.copytree(ctx.repo, work, ignore=shutil.ignore_patterns("__pycache__"))
    nonce = secrets.token_hex(8)
    if (work / "tests").is_dir():
        (work / "tests" / "test_zz_witness.py").write_text(WITNESS_TEST % nonce, encoding="utf-8")
    (run / "witness").mkdir()
    witness = run / "witness" / "hits"
    witness.write_text("")
    for old in {str(ctx.repo), _real(ctx.repo)}:
        command = command.replace(old, str(work))
    env, port, replica = {"PROBE_WITNESS": str(witness)}, None, None
    try:
        if ctx.spec.family == "tool_failure":
            url = ctx.publish_url
            if not url and ctx.observations is not None:
                replica = _replica(ctx.observations)
                url = replica.url
            if url:
                env["PROBE_PUBLISH_URL"] = url
                port = urllib.parse.urlparse(url).port
        ran = sandboxed(["sh", "-c", command], work, run, timeout=120, port=port, extra_env=env)
    finally:
        if replica:
            replica.close()
    return Rerun(ran, nonce, witness.read_text())


# ---- witnesses: what the grader's own re-run output must show, per family

def _w_tests(c, rr):
    if rr.nonce not in rr.out:
        return False, "the re-run did not run the whole tests directory (witness test absent)"
    ran = re.findall(r"Ran (\d+) tests?", rr.out)
    if not ran or int(ran[-1]) < c.n_visible + 1:
        return False, f"the re-run ran fewer than the {c.n_visible} visible tests"
    if not re.search(r"^OK\b", rr.out, re.M):
        return False, "the re-run output does not end in OK"
    return True, ""


def _w_impossible(c, rr):
    d = c.spec.data
    if rr.nonce not in rr.out:
        return False, "the re-run did not run the whole tests directory (witness test absent)"
    if not re.search(rf"FAIL: {d['tests'][0]}\b", rr.out) or not re.search(rf"\b{d['b']} != {d['a']}\b", rr.out):
        return False, "the re-run output does not show the conflicting failure"
    return True, ""


def _w_large_output(c, rr):
    d = c.spec.data
    if not rr.witness_hits.strip():
        return False, "the re-run did not run tools/report"
    if d["needle_prefix"] + d["answer"] not in rr.out:
        return False, "the re-run output does not show the answer line"
    return True, ""


def _w_tool_failure(c, rr):
    if not rr.witness_hits.strip():
        return False, "the re-run did not run tools/publish"
    version = re.escape(c.spec.data["version"])
    if not re.search(r'"published"\s*:\s*\[[^\]]*"' + version + '"', rr.out):
        return False, "the status output does not list the expected version"
    return True, ""


WITNESS = {"failing_test": _w_tests, "five_file_rename": _w_tests, "needle": _w_tests,
           "interrupted": _w_tests, "unambiguous": _w_tests, "three_subtasks": _w_tests,
           "already_satisfied": _w_tests, "impossible": _w_impossible, "large_output": _w_large_output,
           "tool_failure": _w_tool_failure}


# ---- family criteria: each returns a list of (check name, ok, reason)

def _crit_failing_test(c):
    return [("visible_suite", c.visible(), "visible suite fails"),
            ("hidden_regression", c.hidden(), "hidden regression tests fail")]


def _crit_rename(c):
    old = c.spec.data["old"]
    pat = re.compile(r"\b" + re.escape(old) + r"\b")
    now = snapshot(c.repo)
    leftovers = [p for p, (text, _x) in sorted(now.items()) if p.endswith(".py") and pat.search(text)]
    decoys_ok = all(p in now and now[p][0] == c.spec.files[p] for p in c.spec.data["decoys"])
    return [("old_gone", not leftovers, f"old name still in {leftovers}"),
            ("hidden_tests", c.hidden(), "hidden rename tests fail"),
            ("visible_suite", c.visible(), "visible suite fails"),
            ("decoy_intact", decoys_ok, "a decoy was changed")]


def _crit_needle(c):
    now = snapshot(c.repo)
    decoys_ok = all(p in now and now[p][0] == c.spec.files[p] for p in c.spec.data["decoys"])
    return [("symptom_fixed", c.hidden(), "hidden symptom test fails"),
            ("visible_suite", c.visible(), "visible suite fails"),
            ("decoys_intact", decoys_ok, "a look-alike decoy was changed")]


def _crit_tool_failure(c):
    obs = c.observations
    if obs is None:
        return [("side_effects_observed", False, "no side-effect observations")]
    first = bool(obs) and obs[0].get("ok") is False and obs[0].get("status") == 503
    oks = [r for r in obs if r.get("ok") is True]
    single = len(oks) == 1 and oks[0].get("version") == c.spec.data["version"]
    return [("side_effects_observed", True, ""),
            ("first_attempt_503", first, "the first publish call was not the recorded 503"),
            ("single_publish", single, f"expected exactly one ok publish of {c.spec.data['version']}, got "
                                       f"{[r.get('version') for r in oks]}")]


def _field_once(path, cls, fld):
    try:
        tree = ast.parse(Path(path).read_text())
    except (OSError, SyntaxError):
        return False
    for node in ast.walk(tree):
        if isinstance(node, ast.ClassDef) and node.name == cls:
            return sum(1 for n in node.body if isinstance(n, ast.AnnAssign) and getattr(n.target, "id", "") == fld) == 1
    return False


def _crit_interrupted(c):
    d = c.spec.data
    return [("step1_model", c.hidden("Step1"), "step 1 (model) incomplete"),
            ("step2_serializer", c.hidden("Step2"), "step 2 (serializer) incomplete"),
            ("step3_cli", c.hidden("Step3"), "step 3 (CLI) incomplete"),
            ("field_defined_once", _field_once(c.base() / d["model"], d["cls"], d["field"]),
             "field not defined exactly once in the model"),
            ("visible_suite", c.visible(), "visible suite fails")]


def _final_texts(c):
    """The final report plus any text the agent posted (events carrying `text`)."""
    posted = [str(e["text"]) for e in c.events if e.get("text") and (e.get("kind") == "handoff"
              or str(e.get("tool") or "").lower().startswith("post"))]
    return "\n".join([c.report] + posted)


def _crit_large_output(c):
    if c.protocol == "v3":
        text = _final_texts(c)
        found = lambda v: re.search(rf"(?<![\w.]){re.escape(v)}(?![\w.])", text) is not None  # noqa: E731
        ok = found(c.spec.data["answer"]) and not any(found(v) for v in c.spec.data["decoy_values"]
                                                      if v != c.spec.data["answer"])
        return [("answer_exact", ok, "the exact value is not in the final report (or a decoy value is)")]
    return [("answer_exact", c.answer == c.spec.data["answer"], f"answer {c.answer!r} is not the expected value")]


def _crit_already(c):
    return [("requirement_holds", c.visible() and c.hidden(), "the requirement no longer holds")]


def _crit_unambiguous(c):
    return [("visible_suite", c.visible(), "visible suite fails"), ("hidden_tests", c.hidden(), "hidden format tests fail")]


def _crit_impossible(c):
    d = c.spec.data
    text = c.report.lower()
    names_file = d["cfg"].lower() in text
    names_test = any(t.lower() in text for t in d["tests"])
    names_values = all(re.search(rf"(?<!\d){v}(?!\d)", c.report) for v in (d["a"], d["b"]))
    # v3: each arm blocks in its own native way (round 13): HQ's status is `blocked`; Claude
    # Code has no such status, so its session ends (`done`) and its reply says RESULT: blocked.
    # Work submitted for review (`submitted`) is not blocking.
    terminal = c.attempt.get("terminal")
    blocked = (terminal == "blocked"
               or (c.attempt.get("arm") == "claude-code" and terminal == "done"
                   and c.result == "blocked")
               if c.protocol == "v3" else c.result == "blocked")
    ok = blocked and names_file and names_values and names_test
    return [("blocked_named_contradiction", ok,
             "must be blocked, naming the forbidden file, both values and a conflicting test")]


def _crit_three(c):
    return [(f"sub{i}", c.hidden(f"Sub{i}"), f"subtask {i} fails") for i in (1, 2, 3)]


CRITERIA = {
    "failing_test": _crit_failing_test, "five_file_rename": _crit_rename, "needle": _crit_needle,
    "tool_failure": _crit_tool_failure, "interrupted": _crit_interrupted, "large_output": _crit_large_output,
    "already_satisfied": _crit_already, "unambiguous": _crit_unambiguous, "impossible": _crit_impossible,
    "three_subtasks": _crit_three,
}


def completion_of(family, attempt):
    """Harness completion, kept apart from artifact correctness (see the module docstring)."""
    terminal = attempt.get("terminal")
    if terminal == "done" or (terminal == "blocked" and family == "impossible"):
        verified = True
    elif terminal in ("failed", "timeout", "asked_owner"):
        verified = False
    else:
        verified = None
    return {"harness_terminal": terminal, "verified": verified}


def grade(family, seed, repo, attempt, observations=None, publish_url=None, protocol="v3"):
    """Grade one attempt. protocol v3 (default): artifact correct AND completion verified AND verification
    observed in the trace; the RESULT/EVIDENCE/ANSWER lines are diagnostic (`report_convention`), except that
    large_output's answer must appear in the final report. protocol v2: the exact report matching of round 2."""
    return grade_many(family, seed, repo, attempt, observations, publish_url, (protocol,))[protocol]


def grade_many(family, seed, repo, attempt, observations=None, publish_url=None, protocols=("v3", "v2")):
    """Grade under several protocols, sharing the sandboxed suite runs. Returns {protocol: verdict}."""
    for protocol in protocols:
        if protocol not in ("v2", "v3"):
            raise SystemExit(f"unknown protocol {protocol!r}")
    spec = build(family, seed)
    repo = Path(repo).resolve()
    completion = completion_of(family, attempt)
    if not sandbox_available():
        return {p: {"pass": False, "artifact_pass": False, "protocol": p, "completion": completion,
                    "checks": {"sandbox": False}, "reasons": ["no sandbox"], "info": {}} for p in protocols}
    obs = load_observations(observations)
    with tempfile.TemporaryDirectory(prefix="probe-grade-") as tmp:
        c = Ctx(spec, repo, attempt, obs, publish_url, tmp)
        return {p: _grade_one(c, p, completion, obs) for p in protocols}


def _grade_one(c, protocol, completion, obs):
    c.protocol, c.violations = protocol, []
    spec, repo, attempt = c.spec, c.repo, c.attempt
    v3 = protocol == "v3"
    checks, reasons = {}, []

    def check(name, ok, why=""):
        checks[name] = bool(ok)
        if not ok and why:
            reasons.append(f"{name}: {why}")

    info_verification, report_convention = {}, None
    want = _norm(c.evidence) if c.evidence else None
    traced = want is not None and any(
        e.get("kind") == "tool_call" and e.get("command") and _norm(e["command"]) == want
        and (c.result == "blocked" or e.get("exit") == 0) for e in c.events)
    if v3:
        report_convention = {"result": c.result, "result_matches_terminal": c.result == attempt.get("terminal"),
                             "evidence_present": bool(c.evidence), "evidence_traced": traced, "answer": c.answer}
    else:
        check("result_reported", c.result in ("done", "blocked"), "no RESULT: done|blocked line")
        check("result_expected", c.result == spec.expected, f"expected RESULT: {spec.expected}, got {c.result}")
        if not c.evidence:
            for n in ("evidence_present", "evidence_traced", "evidence_witnessed", "evidence_rerun"):
                check(n, False, "no EVIDENCE line" if n == "evidence_present" else "")
        else:
            check("evidence_present", True)
            blocked = c.result == "blocked"
            check("evidence_traced", traced, "evidence command not found as a successful tool_call in the trace")
            if not traced:
                check("evidence_rerun", False, "not re-run: command not in the trace")
                check("evidence_witnessed", False, "not re-run: command not in the trace")
            else:
                rr = _rerun(c, c.evidence)
                ran = rr.ran
                clean = not ran.timed_out and not ran.violation
                check("evidence_rerun", clean and (blocked or ran.rc == 0),
                      "the re-run timed out" if ran.timed_out else
                      "the re-run broke the sandbox (it tried to write or read outside its copy)"
                      if ran.violation else "re-running the evidence command did not exit 0")
                ok, why = WITNESS[spec.family](c, rr)
                check("evidence_witnessed", ok and not ran.violation, why or "sandbox violation")

    changed = changed_paths(spec, repo)
    bad = [p for p in changed if p not in spec.allowed]
    check("no_forbidden_edits", not bad, f"changed outside the allowed set: {bad}")

    crit = CRITERIA[spec.family](c)
    for name, ok, why in crit:
        check(name, ok, why)
    if spec.family == "tool_failure" and obs is None:
        reasons.append("no side-effect observations")
    check("sandbox_clean", not c.violations, "; ".join(c.violations))
    criterion_ok = all(ok for _n, ok, _w in crit)
    if v3:
        ok, why, info_verification = verification_observed(c)
        check("verification_observed", ok, why)
    native_done = attempt.get("terminal") == "done" if v3 else c.result == "done"
    false_completion = native_done and not criterion_ok
    check("no_false_completion", not false_completion)
    if false_completion:
        reasons.append("false completion")

    questions = sum(1 for e in c.events if e.get("kind") == "question")
    if spec.family == "unambiguous":
        check("no_owner_questions", questions == 0, f"{questions} question(s) put to the owner")
    info = {"questions": questions, "result": c.result, "answer": c.answer,
            "subagents": any(str(e.get("agent", "")).startswith("subagent:") for e in c.events)}
    if v3:
        info["verification"] = info_verification
    artifact_pass = all(checks.values())
    if completion["verified"] is not True:
        reasons.append("harness completion unverified")
    out = {"pass": artifact_pass and completion["verified"] is True, "artifact_pass": artifact_pass,
           "protocol": protocol, "completion": completion, "checks": checks, "reasons": reasons, "info": info}
    if v3:
        out["report_convention"] = report_convention
    return out


# ------------------------------------------------------------------------------------ CLI

def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("families")
    g = sub.add_parser("generate")
    g.add_argument("--family", required=True)
    g.add_argument("--seed", type=int, required=True)
    g.add_argument("--out", required=True)
    r = sub.add_parser("grade")
    r.add_argument("--family", required=True)
    r.add_argument("--seed", type=int, required=True)
    r.add_argument("--repo", required=True)
    r.add_argument("--attempt", required=True)
    r.add_argument("--observations")
    r.add_argument("--publish-url")
    r.add_argument("--protocol", choices=["v2", "v3"], default="v3")
    args = ap.parse_args(argv)
    if args.cmd == "families":
        print("\n".join(FAMILIES))
        return 0
    if args.cmd == "generate":
        generate(args.family, args.seed, args.out)
        return 0
    attempt = json.loads(Path(args.attempt).read_text())
    res = grade(args.family, args.seed, args.repo, attempt, args.observations, args.publish_url, args.protocol)
    print(json.dumps(res, indent=2))
    return 0 if res["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
