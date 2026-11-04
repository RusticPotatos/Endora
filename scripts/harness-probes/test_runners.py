"""The two arms' runners, without spending anything: cc_local against a fake `claude`, and
the shared interruption rule. (endora_local's own check is the integrated run with the real
worker and HQ, which needs the built binaries; see the round's report.)

    python3 -m unittest scripts/harness-probes/test_runners.py
"""

import json
import os
import subprocess
import sys
import tempfile
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

    def test_an_attempt_past_its_time_is_stopped_as_a_timeout(self):
        with tempfile.TemporaryDirectory() as d:
            _, how, _ = rig.run_watched([sys.executable, "-c", "import time; time.sleep(30)"],
                                        cwd=d, env=dict(os.environ), timeout=0.5)
            self.assertEqual(how, "timeout")


if __name__ == "__main__":
    unittest.main()
