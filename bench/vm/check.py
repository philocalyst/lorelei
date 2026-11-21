#!/usr/bin/env python3
"""Audit a bench/vm result directory. Exit status is the verdict run.sh returns.

  python3 bench/vm/check.py RESULT_DIR --frameworks bough,bough-x,zgui --scenarios rows,table [--strict-validity]

Exit 0  every requested scenario x framework has a cell, no cell has errors, nothing skipped,
        run.json present, and (with --strict-validity) every report is valid.
Exit 2  failure: run.json missing/unreadable, a requested scenario x framework has no cell,
        a cell lists errors or skipped phases.
Exit 4  only with --strict-validity: the run is complete but some reports are invalid
        (`validity.valid` false) or unchecked (no `validity` object in a report that should have
        one). Without the flag these are reported in the summary, never ranked, exit stays 0.
A summary goes to stdout and to RESULT_DIR/VALIDITY.txt; machine form in VALIDITY.json.
Startup and churn lack a timing verdict. The runner records layout in done without a separate JSON report.
"""
import argparse
import json
from pathlib import Path
import sys


PHASES = {
    "rows": {"hover", "scroll"},
    "virtual": {"hover", "scroll"},
    "search": {"filter", "typing"},
    "typing": {"typing"},
    "resize": {"resize", "scale"},
    "spinner": {"pacing"},
    "ticker": {"pacing"},
    "table": {"op-select", "op-update10", "op-swap", "op-remove", "op-insert",
              "op-replace", "op-append", "op-clear", "churn"},
}
UNTIMED = {"startup", "churn"}


def read_object(path):
    data = json.loads(path.read_text())
    if not isinstance(data, dict):
        raise ValueError("expected a JSON object")
    return data


def audit(root, frameworks, scenarios):
    out = {"failures": [], "invalid": [], "unchecked": [], "cells": 0, "reports": 0,
           "valid_reports": 0, "matrix": {}}
    root = Path(root).resolve()
    try:
        cells = read_object(root / "run.json")["cells"]
        if not isinstance(cells, list) or not cells:
            raise ValueError("the runner wrote no cells")
    except (OSError, ValueError, KeyError) as exc:
        out["failures"].append(f"run.json unreadable: {exc}")
        return out
    out["cells"] = len(cells)
    directories = set()
    for cell in cells:
        try:
            scenario = cell["cell"]["scenario"]
            framework = cell["cell"]["framework"]
            key = f"{scenario}/{framework}"
            directory = (root / cell["dir"]).resolve()
            relative = directory.relative_to(root)
            if not relative.parts or directory in directories:
                raise ValueError("duplicate or empty cell directory")
            directories.add(directory)
            done = set(cell["done"])
            errors = cell["errors"]
            skipped = cell["skipped"]
            required = PHASES[scenario] | {"startup", "layout", "idle"}
        except (OSError, KeyError, TypeError, ValueError) as exc:
            out["failures"].append(f"malformed cell: {exc}")
            continue
        out["matrix"][key] = out["matrix"].get(key, 0) + 1
        if errors or skipped or required - done:
            if errors:
                out["failures"].append(f"{relative}: errors {errors}")
            if skipped:
                out["failures"].append(f"{relative}: skipped {skipped}")
            if required - done:
                out["failures"].append(f"{relative}: missing phases {sorted(required - done)}")
        for phase in sorted(done - {"layout"}):
            if not isinstance(phase, str) or Path(phase).name != phase or not phase:
                out["failures"].append(f"{relative}: malformed phase name")
                continue
            report = directory / f"{phase}.json"
            rel = report.relative_to(root)
            try:
                data = read_object(report)
            except (OSError, ValueError) as exc:
                out["failures"].append(f"{rel}: unreadable ({exc})")
                continue
            if phase in UNTIMED:
                continue
            out["reports"] += 1
            validity = data.get("validity")
            if not isinstance(validity, dict) or type(validity.get("valid")) is not bool:
                out["unchecked"].append(str(rel))
            elif validity["valid"]:
                out["valid_reports"] += 1
            else:
                bad = [check["name"] for check in validity.get("checks", []) if check.get("status") != "pass"]
                out["invalid"].append(f"{rel}: {','.join(bad) or 'valid=false'}")
    for scenario in scenarios:
        for framework in frameworks:
            if f"{scenario}/{framework}" not in out["matrix"]:
                out["failures"].append(f"no cell for {scenario}/{framework}")
    return out


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("root")
    ap.add_argument("--frameworks", default="")
    ap.add_argument("--scenarios", default="")
    ap.add_argument("--strict-validity", action="store_true")
    a = ap.parse_args(argv)
    fws = [x for x in a.frameworks.split(",") if x]
    scs = [x for x in a.scenarios.split(",") if x]
    r = audit(a.root, fws, scs)
    code = 2 if r["failures"] else (4 if a.strict_validity and (r["invalid"] or r["unchecked"]) else 0)
    r["exit"] = code
    lines = [f"check: {r['cells']} cells, {r['reports']} validity-bearing reports: {r['valid_reports']} valid, "
             f"{len(r['invalid'])} invalid, {len(r['unchecked'])} unchecked, {len(r['failures'])} failures -> exit {code}"]
    lines += ["FAIL " + x for x in r["failures"]] + ["INVALID " + x for x in r["invalid"]] + ["UNCHECKED " + x for x in r["unchecked"]]
    text = "\n".join(lines)
    print(text)
    root = Path(a.root)
    if root.is_dir():
        (root / "VALIDITY.txt").write_text(text + "\n")
        (root / "VALIDITY.json").write_text(json.dumps(r, indent=1) + "\n")
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
