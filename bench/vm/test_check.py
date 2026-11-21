import json, os, subprocess, sys, tempfile, unittest
from pathlib import Path
HERE = os.path.dirname(os.path.abspath(__file__))
CHECK = os.path.join(HERE, "check.py")


def mk(root, cells, reports):
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    (root / "run.json").write_text(json.dumps({"cells": cells}))
    for c in cells:
        for phase in c["done"]:
            if phase != "layout":
                reports.setdefault(f"{c['dir']}/{phase}.json", {} if phase == "startup" else OK)
    for rel, body in reports.items():
        p = root / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(json.dumps(body))


def cell(s, fw, errors=(), skipped=(), done=("startup", "layout", "idle", "hover", "scroll")):
    return {"cell": {"scenario": s, "framework": fw}, "dir": f"{s}/{fw}", "errors": list(errors),
            "skipped": list(skipped), "done": list(done)}


OK = {"validity": {"valid": True, "checks": []}}
BAD = {"validity": {"valid": False, "checks": [{"name": "wake_lateness", "status": "fail"}]}}


def run(root, *extra):
    p = subprocess.run([sys.executable, "-I", CHECK, root, "--frameworks", "a", "--scenarios", "rows", *extra],
                       capture_output=True, text=True)
    return p.returncode, p.stdout


class T(unittest.TestCase):
    def setUp(self):
        out = tempfile.TemporaryDirectory()
        self.addCleanup(out.cleanup)
        self.d = out.name

    def test_clean(self):
        mk(self.d, [cell("rows", "a")],
           {"rows/a/startup.json": {}, "rows/a/hover.json": OK})
        self.assertEqual(run(self.d)[0], 0)
        self.assertEqual(run(self.d, "--strict-validity")[0], 0)

    def test_cell_error_fails_even_if_runner_exit_was_zero(self):
        mk(self.d, [cell("rows", "a", errors=["boom"])], {"rows/a/hover.json": OK})
        self.assertEqual(run(self.d)[0], 2)

    def test_missing_cell_fails(self):
        mk(self.d, [cell("table", "a")], {})
        self.assertEqual(run(self.d)[0], 2)

    def test_missing_run_json_fails(self):
        self.assertEqual(run(self.d)[0], 2)

    def test_skipped_fails(self):
        mk(self.d, [cell("rows", "a", skipped=["hover"])], {})
        self.assertEqual(run(self.d)[0], 2)

    def test_invalid_reported_and_strict_boundary(self):
        mk(self.d, [cell("rows", "a")], {"rows/a/hover.json": BAD})
        c, out = run(self.d)
        self.assertEqual(c, 0); self.assertIn("INVALID rows/a/hover.json: wake_lateness", out)
        self.assertEqual(run(self.d, "--strict-validity")[0], 4)

    def test_missing_validity_is_never_valid(self):
        mk(self.d, [cell("rows", "a")], {"rows/a/hover.json": {}})
        self.assertEqual(run(self.d, "--strict-validity")[0], 4)

    def test_startup_without_validity_is_fine(self):
        mk(self.d, [cell("rows", "a")], {"rows/a/startup.json": {}})
        self.assertEqual(run(self.d, "--strict-validity")[0], 0)

    def test_missing_output_of_done_phase_fails(self):
        mk(self.d, [cell("rows", "a")], {})
        (Path(self.d) / "rows/a/hover.json").unlink()
        self.assertEqual(run(self.d, "--strict-validity")[0], 2)

    def test_idle_is_a_checked_report(self):
        mk(self.d, [cell("rows", "a")], {"rows/a/idle.json": BAD})
        self.assertEqual(run(self.d, "--strict-validity")[0], 4)

    def test_empty_phase_set_fails(self):
        mk(self.d, [cell("rows", "a", done=())], {})
        self.assertEqual(run(self.d, "--strict-validity")[0], 2)

    def test_failure_outranks_invalid(self):
        mk(self.d, [cell("rows", "a", errors=["x"], done=("hover",))], {"rows/a/hover.json": BAD})
        self.assertEqual(run(self.d, "--strict-validity")[0], 2)


if __name__ == "__main__":
    unittest.main()
