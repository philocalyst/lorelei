import json, os, subprocess, sys, tempfile, unittest
HERE = os.path.dirname(os.path.abspath(__file__))
CHECK = os.path.join(HERE, "check.py")


def mk(root, cells, reports):
    os.makedirs(root, exist_ok=True)
    json.dump({"cells": cells}, open(os.path.join(root, "run.json"), "w"))
    for rel, body in reports.items():
        p = os.path.join(root, rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        json.dump(body, open(p, "w"))


def cell(s, fw, errors=(), skipped=(), done=("startup",)):
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
        self.d = tempfile.mkdtemp()

    def test_clean(self):
        mk(self.d, [cell("rows", "a", done=("startup", "hover"))],
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
        mk(self.d, [cell("rows", "a", done=("hover",))], {"rows/a/hover.json": BAD})
        c, out = run(self.d)
        self.assertEqual(c, 0); self.assertIn("INVALID rows/a/hover.json: wake_lateness", out)
        self.assertEqual(run(self.d, "--strict-validity")[0], 4)

    def test_missing_validity_is_never_valid(self):
        mk(self.d, [cell("rows", "a", done=("hover",))], {"rows/a/hover.json": {}})
        self.assertEqual(run(self.d, "--strict-validity")[0], 4)

    def test_startup_without_validity_is_fine(self):
        mk(self.d, [cell("rows", "a")], {"rows/a/startup.json": {}, "rows/a/idle.json": {}})
        self.assertEqual(run(self.d, "--strict-validity")[0], 0)

    def test_failure_outranks_invalid(self):
        mk(self.d, [cell("rows", "a", errors=["x"], done=("hover",))], {"rows/a/hover.json": BAD})
        self.assertEqual(run(self.d, "--strict-validity")[0], 2)


if __name__ == "__main__":
    unittest.main()
