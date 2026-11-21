import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from operations import PBENCH_EXECUTORS, REQUIRED, guibench, pbench, table
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

    def test_facade_uses_per_operation_block_median(self):
        output = "perop op=hover render=false med=3033593 lo=3033241 hi=3211363 records=5002 frames=53"
        args = ("--op", "hover", "--blocks", "3")
        self.assertEqual(guibench(output, args)["hover"]["instructions"], 3033593)
        with self.assertRaises(ValueError):
            guibench(output.replace("render=false", "render=true"), args)

    def test_search_keeps_both_empty_query_backspaces(self):
        header = "scene\tpart\tphase\tsteps\tinstr_p50\tinstr_p95\tcycles_p50\n"
        row = "ops_x\tsearch\tscan backspace (empty query)\t1\t25\t25\t0\n"
        operations = table("ops", header + row * 2, {"PART": "search"})
        self.assertEqual(len(operations), 2)
        with self.assertRaises(ValueError):
            table("ops", header + row * 3, {"PART": "search"})

    def test_pbench_requires_all_rows_and_gates_serial_cpu(self):
        args = ("anim", "600")
        env = {"REPS": "24", "WORKERS": "5"}
        header = "# pbench anim Some(600); reps 24; workers 5;\nphase\texecutor\ttiles\tthread_Kinstr"
        rows = [f"anim.{phase}\t{executor} details\t30\t293.4" for phase in ("full", "theme", "time")
                for executor in sorted(PBENCH_EXECUTORS)]
        verdict = "# cross-check failures: 0; load at end: 2.0"
        operations = pbench("\n".join([header, *rows, verdict]), args, env)
        self.assertEqual(len(operations), 12)
        self.assertEqual(operations["anim.full/A1"], {"instructions": 293400, "tiles": 30})
        self.assertNotIn("anim.full/AP", operations)
        for changed in ([header, *rows[:-1], verdict], [header, *rows, rows[0], verdict],
                        [header, *rows, verdict.replace("0;", "1;")]):
            with self.assertRaises(ValueError):
                pbench("\n".join(changed), args, env)

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
                "valid": True, "load": [25.0] * 3, "binary": "sheet_x", "argv": [], "env": {"ROWS": "10000"},
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

    def test_instruction_variance_requires_identical_binary(self):
        base = copy.deepcopy(self.doc)
        base["binaries"]["sheet_x"]["sha256"] = "a" * 64
        head = copy.deepcopy(base)
        head["cells"][self.cid]["operations"]["hover"]["instr_p50"] = summary([15000] * 3)
        repeat = self.compare(base, head, "--require-same-cells")
        self.assertEqual(repeat.returncode, 0, repeat.stderr)
        self.assertIn("VARIED (same binary)", repeat.stdout)
        head["binaries"]["sheet_x"]["sha256"] = "b" * 64
        changed = self.compare(base, head, "--require-same-cells")
        self.assertEqual(changed.returncode, 1, changed.stderr)
        self.assertIn("REGRESSED", changed.stdout)

    def test_integration_gate_checks_added_operations(self):
        head = copy.deepcopy(self.doc)
        operations = head["cells"][self.cid]["operations"]
        operations["scroll"] = copy.deepcopy(operations["hover"])
        self.assertEqual(self.compare(self.doc, head).returncode, 0)
        strict = self.compare(self.doc, head, "--require-same-cells")
        self.assertEqual(strict.returncode, 1)
        self.assertIn("ADDED", strict.stdout)
        focused = self.compare(self.doc, head, "--require-same-cells", "--cells", self.cid)
        self.assertEqual(focused.returncode, 1)
        self.assertIn("ADDED", focused.stdout)

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
