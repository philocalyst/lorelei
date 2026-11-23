import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from operations import (FLAT_CELLS, FLAT_EXECUTORS, FLAT_HEADER, FX_EXECUTORS, FX_HEADER, FX_METRICS, FX_PHASES,
                        PBENCH_EXECUTORS, REQUIRED, flatbench, guibench, pbench, staged_paint, table)
from run import binary_spec, cells, core_package, summary, summarize


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

    def staged_output(self, scene="pane", size="300", reps=24, rows=None):
        header = "\t".join(FX_HEADER)
        lines = [f"# fxbench {scene} {size}; 1280x800; reps {reps}; load at start: 1.0 1.0 1.0",
                 "# scene: 14 records", header]
        for phase in FX_PHASES[scene]:
            for executor in FX_EXECUTORS:
                mixed = executor == "mixed"
                lines.append("\t".join([
                    scene, size, phase, executor, "27.0", "0.0", "45.0", "162.0", "0", "57556", "0",
                    "75.8", "74.6", "76.1", "392", "103", "0", "-" if mixed else "2257",
                    f"0/{reps - 4}/4" if mixed else "0/0/0", "60/480", "0/420 0/428 62/553 16/360 0/0", "1/341"]))
        lines += rows or []
        lines.append("# mismatching words in total: 0; load at end: 1.0")
        return "\n".join(lines)

    def test_staged_paint_gates_every_executor_and_checks_the_diagnostics(self):
        env = {"REPS": "24"}
        operations = staged_paint(self.staged_output(), ("pane", "300"), env)
        self.assertEqual(len(operations), len(FX_PHASES["pane"]) * len(FX_EXECUTORS))
        self.assertEqual(operations["mixed/idle"]["instructions"], 75800)
        self.assertEqual((operations["mixed/idle"]["ran_cpu"], operations["mixed/idle"]["ran_gpu"]), (20, 4))
        self.assertNotIn("ran_cpu", operations["metal/idle"])
        self.assertEqual(set(operations["cpu1/idle"]) - {"instructions"},
                         {name for name in FX_METRICS if name != "instr_med_K"})
        media = staged_paint(self.staged_output("media", "4"), ("media", "4"), env)
        self.assertIn("cpu1/a/v playback (audio clocks)", media)
        good = self.staged_output()
        for bad in (good.replace("\t2257\t", "\t-\t", 1),                       # a turn counter missing
                    good.replace("\t-\t", "\t17\t", 1),                         # mixed with a turn counter
                    good.replace("0/20/4", "0/20/5", 1),                        # the split misses frames
                    good.replace("\t0/0/0\t", "\t0/1/0\t", 1),                  # a non-chooser that chose
                    good.replace("60/480", "481/480", 1),                       # same above total
                    good.replace("0/420 0/428", "0/420", 1),                    # a level missing
                    good.replace("1/341", "1/x", 1),                            # garbage
                    good.replace("\t392\t", "\t-\t", 1),                        # a metric that is not a number
                    good.replace("\t0\t-\t", "\t3\t-\t", 1),                     # a pixel mismatch
                    good.replace("ranks/slots", "ranks"),                       # a changed header
                    good.replace("reps 24", "reps 6"),                          # other repetitions
                    good.replace("# mismatching words in total: 0", "# mismatching words in total: 1"),
                    "\n".join(line for line in good.splitlines() if "\tmixed\t" not in line)):
            with self.assertRaises(ValueError):
                staged_paint(bad, ("pane", "300"), env)

    def test_flatbench_gates_single_core_and_metal_and_requires_every_row(self):
        header = "\t".join(FLAT_HEADER)
        rows = [f"{scene}\t{cell}\t{executor}\t0.315\t0.291\t6719.9\t35.0\t1.0\t0.0\t0.0\t0"
                for scene, cells in FLAT_CELLS.items() for cell in cells for executor in FLAT_EXECUTORS]
        first = "# flatbench: 300 cards (1503 records) in a scrolled world, and two pages; 24 frames per cell"
        good = "\n".join([first, header, *rows, "# mismatching words in total: 0"])
        operations = flatbench(good, ("300",), {"REPS": "24"})
        self.assertEqual(len(operations), 2 * len(rows) // 3)
        self.assertEqual(operations["Cpu1/idle"]["instructions"], 6719900)
        self.assertNotIn("Cpu8/idle", operations)
        for bad in (good.replace("\t0\n", "\t2\n", 1), good.replace(rows[4] + "\n", ""),
                    good.replace("24 frames", "6 frames"), good.replace("6719.9", "-", 1),
                    good.replace("total: 0", "total: 9"), good + "\n" + rows[0]):
            with self.assertRaises(ValueError):
                flatbench(bad, ("300",), {"REPS": "24"})

    def test_damage_engine_table_requires_every_phase(self):
        header = "scene\tphase\tsteps\tinstr_p50\tinstr_p95\tcycles_p50\ttiles_p50\n"
        rows = [f"damage_x\t{phase}\t64\t3213\t3395\t473\t39" for phase in sorted(REQUIRED["damage"])]
        self.assertEqual(table("damage", header + "\n".join(rows), {})["scroll"]["instr_p50"], 3213)
        with self.assertRaises(ValueError):
            table("damage", header + "\n".join(rows[1:]), {})
        with self.assertRaises(ValueError):
            table("damage", header + "\n".join(rows).replace("damage_x", "sheet_x"), {})

    def test_the_matrix_holds_the_flat_and_damage_cells(self):
        matrix, unavailable = cells(Path(__file__).resolve().parents[2])
        names = {cell.name: cell for cell in matrix}
        self.assertTrue({"flatbench/cards300", "damage_x/window", "damage_x/stack", "damage_x/4k"} <= names.keys())
        self.assertEqual(names["damage_x/4k"].env, {"SCENE": "4k", "REPS": "64"})
        self.assertEqual(binary_spec("damage_x", Path("."))[1][-3:], ["bough-damage", "--example", "damage_x"])
        self.assertEqual(binary_spec("flatbench", Path("."))[1][-3:], ["bough-paint", "--example", "flatbench"])
        self.assertNotIn("flatbench", unavailable)

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

    def test_renamed_core_package_is_the_same_build(self):
        base, head = copy.deepcopy(self.doc), copy.deepcopy(self.doc)
        base["binaries"]["sheet_x"]["cargo_args"] = ["--profile", "dist", "-p", "bough-x", "--example", "sheet_x"]
        head["binaries"]["sheet_x"]["cargo_args"] = ["--profile", "dist", "-p", "bough-dataflow", "--example", "sheet_x"]
        same = self.compare(base, head)
        self.assertEqual(same.returncode, 0, same.stderr)
        # the negative controls: every other build option still has to match
        head["binaries"]["sheet_x"]["cargo_args"][1] = "release"
        self.assertEqual(self.compare(base, head).returncode, 1)
        head["binaries"]["sheet_x"]["cargo_args"][1] = "dist"
        head["binaries"]["sheet_x"]["cargo_args"][3] = "bough"
        self.assertEqual(self.compare(base, head).returncode, 1)

    def test_core_package_follows_the_checkout(self):
        with tempfile.TemporaryDirectory() as folder:
            tree = Path(folder)
            self.assertEqual(core_package(tree), "bough-x")
            (tree / "crates/bough-dataflow").mkdir(parents=True)
            self.assertEqual(core_package(tree), "bough-dataflow")


if __name__ == "__main__":
    unittest.main()
