import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from operations import REQUIRED, table
from run import summary, summarize


class OperationsTests(unittest.TestCase):
    def test_sheet_reads_named_instruction_column(self):
        rows = [f"sheet_x\t{name}\t32\t13504\t13521\t1000\t329" for name in REQUIRED["sheet"]]
        stdout = "scene\tphase\tsteps\tinstr_p50\tinstr_p95\tcycles_p50\ttiles_p50\n" + "\n".join(rows)
        self.assertEqual(table("sheet", stdout, {})["hover"]["instr_p50"], 13504)
        self.assertEqual(table("sheet", stdout, {})["hover"]["tiles_p50"], 329)
        with self.assertRaises(ValueError):
            table("sheet", stdout + "\n" + rows[0], {})
        with self.assertRaises(ValueError):
            table("sheet", stdout.replace("\t13504", "\t0"), {})

    def test_changed_operation_set_refused(self):
        a = {"process": {"instructions": 2000000}, "operations": {"hover": {"instr_p50": 13504}}}
        b = {"process": a["process"], "operations": {}}
        with self.assertRaises(ValueError):
            summarize([a, b])


class CompareTests(unittest.TestCase):
    def setUp(self):
        self.cid = "sheet_x/rows10000"
        self.doc = {
            "format": 2, "name": "baseline", "commit": "a" * 40, "errors": [],
            "load": {"ok": False}, "rustc": "nightly", "host": {"cpu": "Apple Silicon"},
            "contract": "test", "runtime_inputs": {}, "reps": 3,
            "binaries": {"sheet_x": {"cargo_args": ["--profile", "dist"]}},
            "cells": {self.cid: {
                "valid": True, "binary": "sheet_x", "argv": [], "env": {"ROWS": "10000"},
                "parser": "sheet", "counter_scope": "thread", "check": [], "operations": {"hover": {"instr_p50": summary([13504] * 3)}},
                "process": {k: summary([99999] * 3) for k in ("instructions", "wall_ms", "rss_bytes")},
            }},
        }

    def compare(self, base, head, *args):
        with tempfile.TemporaryDirectory() as folder:
            a, b = (Path(folder) / f"{label}.json" for label in ("base", "head"))
            a.write_text(json.dumps(base))
            b.write_text(json.dumps(head))
            return subprocess.run([sys.executable, str(Path(__file__).with_name("compare.py")),
                                   str(a), str(b), *args], capture_output=True, text=True)

    def test_operation_diff_independent_of_process_total(self):
        head = copy.deepcopy(self.doc)
        head["cells"][self.cid]["operations"]["hover"]["instr_p50"] = summary([5324] * 3)
        forward = self.compare(self.doc, head)
        self.assertEqual(forward.returncode, 0, forward.stderr)
        self.assertIn("IMPROVED", forward.stdout)
        reverse = self.compare(head, self.doc)
        self.assertEqual(reverse.returncode, 1, reverse.stderr)
        self.assertIn("REGRESSED", reverse.stdout)

    def test_failures_abstentions_and_bad_counts(self):
        self.assertEqual(self.compare(self.doc, self.doc).returncode, 0)
        self.assertEqual(self.compare(self.doc, self.doc, "--require-quiet").returncode, 1)
        for field, value in (("errors", [{"stage": "build"}]), ("cells", {}), ("format", 1)):
            self.assertEqual(self.compare(self.doc, {**self.doc, field: value}).returncode, 1)
        head = copy.deepcopy(self.doc)
        head["cells"][self.cid]["operations"]["hover"]["instr_p50"]["n"] = 1
        self.assertEqual(self.compare(self.doc, head).returncode, 1)
        head = copy.deepcopy(self.doc)
        head["cells"][self.cid]["env"]["ROWS"] = "1000"
        self.assertEqual(self.compare(self.doc, head).returncode, 1)


if __name__ == "__main__":
    unittest.main()
