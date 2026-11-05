"""The two arms' runners, without spending anything: cc_local against a fake `claude`, and
the shared interruption rule. (endora_local's own check is the integrated run with the real
worker and HQ, which needs the built binaries; see the round's report.)

    python3 -m unittest scripts/harness-probes/test_runners.py
"""

import json
import os
import shlex
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import cc_local  # noqa: E402
import rig  # noqa: E402

FAKE = HERE / "fixtures" / "fake-claude" / "claude"


class CcLocalTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)
        self.root = Path(self.dir.name)
        self.home = self.root / "claude-home"
        self.home.mkdir()

    def run_cc(self, family, plan, *extra):
        (self.root / "plan.json").write_text(json.dumps(plan))
        os.environ["FAKE_CLAUDE_HOME"] = str(self.home)
        os.environ["FAKE_CLAUDE_PLAN"] = str(self.root / "plan.json")
        self.addCleanup(os.environ.pop, "FAKE_CLAUDE_HOME", None)
        self.addCleanup(os.environ.pop, "FAKE_CLAUDE_PLAN", None)
        out = self.root / "out"
        code = cc_local.main(["--family", family, "--seed", "0", "--out", str(out),
                              "--claude-bin", str(FAKE), "--claude-home", str(self.home),
                              "--timeout", "60", *extra])
        self.assertEqual(code, 0)
        return out

    def test_the_real_claude_needs_the_owners_paid_ok(self):
        code = cc_local.main(["--family", "large_output", "--seed", "0",
                              "--out", str(self.root / "x"), "--claude-bin", "claude"])
        self.assertEqual(code, 2)
        self.assertFalse((self.root / "x").exists())

    def test_a_large_output_attempt_is_parsed_and_graded(self):
        probe = self.root / "probe"
        rig.make_probe("large_output", 0, probe)
        grep = "python3 tools/report | grep 'service=profile region=ap-south-1 stat=p95_ms '"
        line = subprocess.run(grep, shell=True, cwd=probe / "repo", capture_output=True,
                              text=True).stdout
        value = line.split("value=")[1].split()[0]
        out = self.run_cc("large_output", [{"run": ["python3 tools/report | wc -c", grep],
                                            "final": f"RESULT: done\nEVIDENCE: {grep}\nANSWER: {value}"}],
                          "--probe-dir", str(probe))
        run = json.loads((out / "run.json").read_text())
        self.assertTrue(run["session_found"])
        attempt = json.loads((out / "attempt.json").read_text())
        self.assertEqual(len(attempt["usage"]), 3)
        self.assertEqual([e["exit"] for e in attempt["events"] if e.get("tool") == "Bash"], [0, 0])
        verdict = json.loads((out / "grade.json").read_text())
        self.assertTrue(verdict["pass"], verdict)

    def test_a_wrong_answer_fails_the_grade(self):
        grep = "python3 tools/report | grep 'region=ap-south-1'"
        out = self.run_cc("large_output", [{"run": [grep],
                                            "final": f"RESULT: done\nEVIDENCE: {grep}\nANSWER: 1"}])
        self.assertFalse(json.loads((out / "grade.json").read_text())["pass"])


class CcInterruptedTests(CcLocalTests):
    def test_an_interrupted_attempt_resumes_its_session_and_is_graded(self):
        import shlex
        import probes
        spec = probes.build("interrupted", 0)
        task = json.loads(json.dumps(spec.extra))
        first = task["interrupt_after"]

        def write(rel):
            return f"printf %s {shlex.quote(spec.solution[rel])} > {shlex.quote(rel)}"

        rest = [rel for rel in sorted(spec.solution) if rel != first]
        out = self.run_cc("interrupted", [
            {"run": [write(first)], "hang": 30},
            {"run": [write(rel) for rel in rest] + [spec.evidence_cmd],
             "final": f"RESULT: done\nEVIDENCE: {spec.evidence_cmd}"},
        ])
        run = json.loads((out / "run.json").read_text())
        self.assertEqual([i["ended"] for i in run["invocations"]], ["interrupted", "exited"])
        # the tree as the stop left it, before the resume (round 16)
        self.assertIn(first, run["invocations"][0]["tree_at_stop"]["status"])
        self.assertNotIn("tree_at_stop", run["invocations"][1])
        attempt = json.loads((out / "attempt.json").read_text())
        self.assertEqual(attempt["terminal"], "done")
        verdict = json.loads((out / "grade.json").read_text())
        self.assertTrue(verdict["pass"], verdict)


class InterruptionTests(unittest.TestCase):
    def test_the_first_change_to_the_watched_file_stops_the_attempt(self):
        with tempfile.TemporaryDirectory() as d:
            watched = Path(d) / "model.py"
            watched.write_text("a = 1\n")
            script = f"import time; time.sleep(0.3); open({str(watched)!r}, 'a').write('b = 2\\n'); time.sleep(30)"
            code, how, _ = rig.run_watched([sys.executable, "-c", script], cwd=d,
                                           env=dict(os.environ), timeout=20,
                                           interrupt_file=watched)
            self.assertEqual(how, "interrupted")
            self.assertIn("b = 2", watched.read_text())

    def test_the_milestone_is_the_first_complete_valid_step_one(self):
        import probes
        spec = probes.build("interrupted", 0)
        m = spec.extra["interrupt_milestone"]
        start = spec.files[spec.extra["interrupt_after"]]
        done = spec.solution[spec.extra["interrupt_after"]]
        self.assertFalse(rig.milestone_reached(m, start))
        self.assertTrue(rig.milestone_reached(m, done))
        self.assertFalse(rig.milestone_reached(m, done[: len(done) - 6]))          # a partial write
        self.assertFalse(rig.milestone_reached(m, spec.variants["step_one_duplicated"][spec.extra["interrupt_after"]]))
        self.assertFalse(rig.milestone_reached(m, done.replace(m["default"], "None")))  # wrong default
        self.assertFalse(rig.milestone_reached(m, None))

    def test_only_the_milestone_stops_the_attempt_not_an_earlier_change(self):
        import probes
        spec = probes.build("interrupted", 0)
        m, rel = spec.extra["interrupt_milestone"], spec.extra["interrupt_after"]
        with tempfile.TemporaryDirectory() as d:
            watched = Path(d) / "model.py"
            watched.write_text(spec.files[rel])
            script = (f"import time; p={str(watched)!r}; time.sleep(0.3); open(p, 'a').write('# note\\n');"
                      f" time.sleep(0.6); open(p, 'w').write({spec.solution[rel]!r}); time.sleep(30)")
            t0 = time.time()
            _, how, _ = rig.run_watched([sys.executable, "-c", script], cwd=d, env=dict(os.environ),
                                        timeout=20, interrupt_file=watched, interrupt_when=m)
            self.assertEqual(how, "interrupted")
            self.assertGreater(time.time() - t0, 0.8)
            self.assertTrue(rig.milestone_reached(m, watched.read_text()))

    def test_an_attempt_past_its_time_is_stopped_as_a_timeout(self):
        with tempfile.TemporaryDirectory() as d:
            _, how, _ = rig.run_watched([sys.executable, "-c", "import time; time.sleep(30)"],
                                        cwd=d, env=dict(os.environ), timeout=0.5)
            self.assertEqual(how, "timeout")


class LiveInterruptTargetTests(unittest.TestCase):
    """Round 16: the live adapter stops only the attempt's own turn, never its keeper or another item's."""

    def test_the_turn_on_this_items_sandbox_and_its_keeper_are_told_apart(self):
        # As the runner labels them (runner.rs): a turn has run and lease labels only; the keeper
        # has the item label. Both mount the item's volume, which is what `docker ps` is filtered by.
        import interrupt_live
        lines = ["k1\tendora.item=42,com.centurylinklabs.watchtower.enable=false",
                 "t1\tcom.centurylinklabs.watchtower.enable=false,endora.lease=3,endora.run=901",
                 "c5\tendora.check=5,endora.branch=x",
                 "x\tsomething=else", "junk"]
        self.assertEqual(interrupt_live.pick(lines, 42), (("t1", "901"), "k1"))
        self.assertEqual(interrupt_live.pick(lines[1:], 42), (("t1", "901"), None))
        self.assertEqual(interrupt_live.pick(lines[2:], 42), (None, None))

    def test_docker_ps_is_filtered_by_the_items_volume(self):
        import interrupt_live
        seen = []

        class Recording(interrupt_live.Docker):
            def __call__(self, *args, timeout=30):
                seen.append(args)
                return 0, "", ""
        Recording("docker").ps(42)
        self.assertIn("volume=endora-item-42", seen[0])


class LiveInterruptTransportTests(unittest.TestCase):
    """Round 16's live failure: over ssh the remote shell split `--format {{.ID}}\\t{{.Labels}}`,
    every `docker ps` failed, and the adapter waited out its time in silence."""

    def test_arguments_survive_an_ssh_prefix(self):
        import interrupt_live
        d = interrupt_live.Docker("ssh -o BatchMode=yes runner.example.com docker")
        argv = d.argv("ps", "--format", "{{.ID}}\t{{.Labels}}")
        self.assertEqual(argv[:4], ["ssh", "-o", "BatchMode=yes", "runner.example.com"])
        self.assertEqual(shlex.split(argv[4]), ["docker", "ps", "--format", "{{.ID}}\t{{.Labels}}"])
        local = interrupt_live.Docker("docker")
        self.assertEqual(local.argv("ps", "--format", "{{.ID}}"), ["docker", "ps", "--format", "{{.ID}}"])

    def test_a_docker_that_keeps_failing_ends_the_watch_loudly(self):
        import interrupt_live

        class Broken(interrupt_live.Docker):
            def __call__(self, *args, timeout=30):
                return 1, "", "docker: 'docker ps' accepts no arguments"
        rec = interrupt_live.watch(Broken("docker"), 42, "m.py", {"class": "A", "field": "f", "type": "int",
                                                                   "default": "1"}, timeout=30, poll=0.01)
        self.assertEqual(rec["outcome"], "docker_failed")
        self.assertIn("accepts no arguments", rec["docker_error"])


class DeadlineTests(unittest.TestCase):
    """Round 20: a live attempt still going at its deadline ends there, as a timeout; what the
    rig did after it (teardown, collection) is recorded apart and never counted as elapsed."""

    def test_an_attempt_past_its_deadline_is_a_timeout_ending_at_the_deadline(self):
        import endora_live
        a = {"terminal": "submitted", "dispatched_at": 100.0, "ended_at": 839.0, "meta": {}}
        endora_live.apply_deadline(a, deadline_at=700.0, finished_by_deadline=False, teardown_at=701.5)
        self.assertEqual(a["terminal"], "timeout")
        self.assertEqual(a["ended_at"], 700.0)
        self.assertEqual(a["meta"]["deadline"], {"at": 700.0, "reached": True, "teardown_at": 701.5,
                                                 "terminal_before": "submitted", "ended_at_before": 839.0})

    def test_an_attempt_finished_in_time_is_left_as_it_is(self):
        import endora_live
        a = {"terminal": "done", "dispatched_at": 100.0, "ended_at": 140.0, "meta": {}}
        endora_live.apply_deadline(a, deadline_at=700.0, finished_by_deadline=True, teardown_at=None)
        self.assertEqual((a["terminal"], a["ended_at"]), ("done", 140.0))
        self.assertEqual(a["meta"]["deadline"]["reached"], False)


if __name__ == "__main__":
    unittest.main()
