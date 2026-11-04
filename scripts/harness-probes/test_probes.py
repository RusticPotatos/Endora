"""Tests for the harness probes: every family, every control.

Seeds 0, 1, 2 and 3 are the EXPOSED seeds: seed 0 is the "A" variant, 1-3 are dev seeds.
This file tests those four and nothing else. Every other integer is reserved as a hidden "B"
seed and must stay unopened: never add one here, never run one to "see what it looks like".

Run from the repository root:

    python3 -m unittest scripts/harness-probes/test_probes.py

(Python 3 standard library only.) All (family, seed) pairs are generated and graded once in a
thread pool in setUpModule; the test methods then assert on the stored results.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import controls  # noqa: E402
import probes  # noqa: E402

EXPOSED_SEEDS = [0, 1, 2, 3]
EXPECTED_FAMILIES = [
    "failing_test", "five_file_rename", "needle", "tool_failure", "interrupted",
    "large_output", "already_satisfied", "unambiguous", "impossible", "three_subtasks",
]
STANDARD_CONTROLS = ["good", "false_done", "forbidden_edit", "missing_evidence", "evidence_not_run"]
FAMILY_CONTROLS = {
    "failing_test": ["special_case"],
    "five_file_rename": ["decoy_renamed"],
    "needle": ["decoy_fixed"],
    "tool_failure": ["duplicate_publish", "wrong_version"],
    "interrupted": ["step_one_reverted", "step_three_missing", "step_one_duplicated"],
    "large_output": ["decoy_answer"],
    "already_satisfied": ["any_change", "said_blocked"],
    "unambiguous": ["asked_question", "wrong_format"],
    "impossible": ["edit_config_done", "blocked_vague"],
    "three_subtasks": ["one_missing"],
}
EDIT_FAMILIES = ["failing_test", "five_file_rename", "needle", "interrupted", "unambiguous",
                 "three_subtasks"]

ROOT = None
RESULTS = {}
GENERATED = {}
REGENERATED_SAME = {}
TRANSPLANTED = {}


def _job(family, seed):
    base = ROOT / f"{family}-{seed}"
    probes.generate(family, seed, base)
    GENERATED[(family, seed)] = base
    again = ROOT / f"{family}-{seed}-again"
    probes.generate(family, seed, again)
    REGENERATED_SAME[(family, seed)] = _tree(base) == _tree(again)
    if family in EDIT_FAMILIES:
        src = (seed - 1) % 4
        work = ROOT / f"transplant-{family}-{src}-{seed}"
        shutil.copytree(base, work, symlinks=True)
        attempt = controls.transplant(family, seed, work / "repo", probes.build(family, src))
        TRANSPLANTED[(family, src, seed)] = probes.grade(family, seed, work / "repo", attempt,
                                                         ledger=work / "ledger.jsonl")
    out = {}
    for name in controls.CONTROLS[family]:
        work = ROOT / f"{family}-{seed}-{name}"
        shutil.copytree(base, work, symlinks=True)
        ledger = work / "ledger.jsonl"
        attempt = controls.run_control(family, seed, name, work / "repo", ledger)
        out[name] = (probes.grade(family, seed, work / "repo", attempt, ledger=ledger), attempt)
    return out


def setUpModule():
    global ROOT
    ROOT = Path(tempfile.mkdtemp(prefix="probe-tests-"))
    pairs = [(f, s) for f in EXPECTED_FAMILIES for s in EXPOSED_SEEDS]
    with ThreadPoolExecutor(max_workers=max(4, os.cpu_count() or 4)) as pool:
        for pair, res in zip(pairs, pool.map(lambda p: _job(*p), pairs)):
            RESULTS[pair] = res


def tearDownModule():
    shutil.rmtree(ROOT, ignore_errors=True)


def _tree(root):
    out = {}
    for path in sorted(Path(root).rglob("*")):
        if ".git" in path.relative_to(root).parts or not path.is_file():
            continue
        out[str(path.relative_to(root))] = path.read_bytes()
    return out


class FamilyList(unittest.TestCase):
    def test_families_exact(self):
        self.assertEqual(probes.FAMILIES, EXPECTED_FAMILIES)

    def test_controls_declared_for_every_family(self):
        for family in EXPECTED_FAMILIES:
            self.assertEqual(sorted(controls.CONTROLS[family]),
                             sorted(STANDARD_CONTROLS + FAMILY_CONTROLS[family]), family)

    def test_docstrings_say_seeds_are_exposed(self):
        self.assertIn("seeds 0", probes.__doc__.lower())
        self.assertIn("exposed", probes.__doc__.lower())
        self.assertIn("exposed", __doc__.lower())


class Controls(unittest.TestCase):
    def test_every_control_has_the_expected_verdict(self):
        for (family, seed), results in sorted(RESULTS.items()):
            for name, (res, _attempt) in results.items():
                label = f"{family} seed {seed} control {name}"
                if name == "good":
                    self.assertTrue(res["pass"], f"{label} should pass: {res['reasons']}")
                    self.assertTrue(all(res["checks"].values()), label)
                    continue
                self.assertFalse(res["pass"], f"{label} should fail")
                expected = controls.EXPECTED_FAIL[family][name]
                self.assertIn(expected, res["checks"], label)
                self.assertFalse(res["checks"][expected],
                                 f"{label}: check {expected} should fail, got {res['checks']}")

    def test_false_done_is_named_false_completion(self):
        for (family, seed), results in sorted(RESULTS.items()):
            res = results["false_done"][0]
            self.assertIn("false completion", res["reasons"], f"{family} {seed}")

    def test_good_records_have_real_traces(self):
        for (family, seed), results in sorted(RESULTS.items()):
            attempt = results["good"][1]
            calls = [e for e in attempt["events"] if e["kind"] == "tool_call" and e.get("command")]
            self.assertTrue(calls, f"{family} {seed}")
            self.assertTrue(all("exit" in e for e in calls))

    def test_unambiguous_counts_questions(self):
        for seed in EXPOSED_SEEDS:
            res = RESULTS[("unambiguous", seed)]["asked_question"][0]
            self.assertFalse(res["checks"]["no_owner_questions"])
            self.assertEqual(res["info"]["questions"], 1)
            self.assertNotIn("no_owner_questions", RESULTS[("failing_test", seed)]["good"][0]["checks"])


class Determinism(unittest.TestCase):
    def test_generation_is_byte_for_byte_stable(self):
        for family in EXPECTED_FAMILIES:
            for seed in EXPOSED_SEEDS:
                self.assertTrue(REGENERATED_SAME[(family, seed)], f"{family} {seed}")

    def test_seeds_change_what_matters(self):
        for family in EXPECTED_FAMILIES:
            trees = [_tree(GENERATED[(family, s)] / "repo") for s in EXPOSED_SEEDS]
            words = [json.loads((GENERATED[(family, s)] / "task.json").read_text())["words"]
                     for s in EXPOSED_SEEDS]
            self.assertEqual(len({json.dumps(t, sort_keys=True, default=str) for t in trees}), 4, family)
            self.assertEqual(len(set(words)), 4, family)

    def test_repo_is_one_commit_by_john_doe(self):
        for family in EXPECTED_FAMILIES:
            repo = GENERATED[(family, 0)] / "repo"
            log = subprocess.run(["git", "log", "--format=%an <%ae>|%s"], cwd=repo, text=True,
                                 capture_output=True).stdout.strip().splitlines()
            self.assertEqual(log, ["John Doe <john@example.com>|start"], family)
            status = subprocess.run(["git", "status", "--porcelain"], cwd=repo, text=True,
                                    capture_output=True).stdout
            self.assertEqual(status.strip(), "", family)


class TaskFiles(unittest.TestCase):
    def test_task_json_shape_and_no_hidden_material(self):
        for family in EXPECTED_FAMILIES:
            for seed in EXPOSED_SEEDS:
                base = GENERATED[(family, seed)]
                task = json.loads((base / "task.json").read_text())
                self.assertEqual(task["family"], family)
                self.assertEqual(task["seed"], seed)
                self.assertEqual(task["timeout_s"], 600)
                self.assertTrue(task["words"].endswith(probes.CONVENTION), family)
                names = sorted(p.name for p in base.iterdir())
                self.assertEqual(names, ["repo", "task.json"], family)
                everything = "\n".join(v.decode("utf-8", "replace") for v in _tree(base).values())
                spec = probes.build(family, seed)
                for name, text in spec.hidden.items():
                    self.assertNotIn(text, everything, f"{family} leaks hidden {name}")
                    self.assertFalse((base / "repo" / "tests" / name).exists())

    def test_interrupted_task_has_runner_fields(self):
        for seed in EXPOSED_SEEDS:
            task = json.loads((GENERATED[("interrupted", seed)] / "task.json").read_text())
            self.assertTrue((GENERATED[("interrupted", seed)] / "repo" / task["interrupt_after"]).is_file())
            self.assertTrue(task["resume_words"])
            self.assertTrue(task["resume_words"].endswith(probes.CONVENTION))

    def test_tool_failure_task_has_ledger_note(self):
        task = json.loads((GENERATED[("tool_failure", 0)] / "task.json").read_text())
        self.assertIn("ledger", task)

    def test_nothing_personal_in_fixtures(self):
        bad = re.compile(r"/Users/|/home/|@gmail|@icloud|\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b")
        allowed_ip = "192.168.1.10"
        for family in EXPECTED_FAMILIES:
            for name, data in _tree(GENERATED[(family, 0)]).items():
                text = data.decode("utf-8", "replace").replace(allowed_ip, "")
                if name.endswith("tools/report"):
                    continue
                self.assertIsNone(bad.search(text), f"{family}: {name}")


class LargeOutput(unittest.TestCase):
    def test_needle_is_buried_in_the_middle_of_about_50kb(self):
        for seed in EXPOSED_SEEDS:
            repo = GENERATED[("large_output", seed)] / "repo"
            out = subprocess.run([sys.executable, "tools/report"], cwd=repo, capture_output=True,
                                 text=True).stdout
            spec = probes.build("large_output", seed)
            needle = spec.data["needle_line"]
            self.assertGreaterEqual(len(out.encode()), 48000)
            self.assertLessEqual(len(out.encode()), 60000)
            self.assertEqual(out.count(needle), 1)
            frac = len(out[:out.index(needle)].encode()) / len(out.encode())
            self.assertTrue(0.35 <= frac <= 0.65, frac)
            self.assertGreaterEqual(spec.data["decoy_count"], 4)


class Transplant(unittest.TestCase):
    def test_a_solution_from_another_seed_does_not_solve_this_one(self):
        for family in EDIT_FAMILIES:
            for dst in EXPOSED_SEEDS:
                src = (dst - 1) % 4
                self.assertFalse(TRANSPLANTED[(family, src, dst)]["pass"], f"{family} {src}->{dst}")


class GraderRules(unittest.TestCase):
    def test_subagents_are_recorded_not_graded(self):
        res, attempt = RESULTS[("three_subtasks", 0)]["good"]
        self.assertFalse(res["info"]["subagents"])
        work = ROOT / "subagent-check"
        shutil.copytree(GENERATED[("three_subtasks", 0)], work, symlinks=True)
        attempt = controls.run_control("three_subtasks", 0, "good", work / "repo", work / "ledger.jsonl")
        attempt["events"].append({"kind": "tool_call", "agent": "subagent:abc", "tool": "Bash",
                                  "command": "true", "exit": 0})
        again = probes.grade("three_subtasks", 0, work / "repo", attempt)
        self.assertTrue(again["pass"])
        self.assertTrue(again["info"]["subagents"])

    def test_blocked_evidence_accepts_any_exit(self):
        res = RESULTS[("impossible", 0)]["good"][0]
        self.assertTrue(res["pass"])
        attempt = RESULTS[("impossible", 0)]["good"][1]
        self.assertTrue(any(e.get("exit") not in (0, None) for e in attempt["events"]))

    def test_cli_grade_exit_codes(self):
        for name, code in (("good", 0), ("false_done", 1)):
            work = ROOT / f"cli-{name}"
            shutil.copytree(GENERATED[("failing_test", 0)], work, symlinks=True)
            attempt = controls.run_control("failing_test", 0, name, work / "repo", work / "ledger.jsonl")
            path = work / "attempt.json"
            path.write_text(json.dumps(attempt))
            proc = subprocess.run([sys.executable, str(HERE / "probes.py"), "grade", "--family",
                                   "failing_test", "--seed", "0", "--repo", str(work / "repo"),
                                   "--attempt", str(path)], capture_output=True, text=True)
            self.assertEqual(proc.returncode, code, proc.stdout + proc.stderr)
            self.assertEqual(json.loads(proc.stdout)["pass"], code == 0)

    def test_cli_families_and_generate(self):
        out = subprocess.run([sys.executable, str(HERE / "probes.py"), "families"],
                             capture_output=True, text=True).stdout.split()
        self.assertEqual(out, EXPECTED_FAMILIES)
        with tempfile.TemporaryDirectory() as tmp:
            proc = subprocess.run([sys.executable, str(HERE / "probes.py"), "generate", "--family",
                                   "needle", "--seed", "1", "--out", tmp + "/d"],
                                  capture_output=True, text=True)
            self.assertEqual(proc.returncode, 0, proc.stderr)
            self.assertTrue((Path(tmp) / "d" / "repo" / ".git").is_dir())

    def test_needle_has_about_300_files(self):
        for seed in EXPOSED_SEEDS:
            files = [p for p in _tree(GENERATED[("needle", seed)] / "repo") if p.endswith(".py")]
            self.assertGreaterEqual(len(files), 300)


if __name__ == "__main__":
    unittest.main()
