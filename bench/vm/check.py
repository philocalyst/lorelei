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
churn.json (retained-memory counters), startup, idle, pacing and layout reports carry no validity object and are never counted."""
import argparse, glob, json, os, sys

# Reports whose JSON is a fakeland benchmark report (they hold a `validity` object).
CHECKED = ("hover", "scroll", "typing", "resize", "filter", "op-", "ticker", "spinner")


def is_checked(name):
    base = os.path.basename(name)
    return base.endswith(".json") and not base.startswith("phases-") and base.startswith(CHECKED)


def audit(root, frameworks, scenarios):
    out = {"root": root, "failures": [], "invalid": [], "unchecked": [], "cells": 0, "reports": 0,
           "valid_reports": 0, "matrix": {}}
    path = os.path.join(root, "run.json")
    try:
        run = json.load(open(path))
        cells = run["cells"]
    except Exception as e:  # missing, truncated or not the runner's format
        out["failures"].append(f"run.json unreadable: {e}")
        return out
    out["cells"] = len(cells)
    for c in cells:
        key = (c["cell"]["scenario"], c["cell"]["framework"])
        out["matrix"].setdefault(f"{key[0]}/{key[1]}", 0)
        out["matrix"][f"{key[0]}/{key[1]}"] += 1
        d = os.path.join(root, c["dir"])
        if c.get("errors"):
            out["failures"].append(f"{c['dir']}: errors {c['errors']}")
        if c.get("skipped"):
            out["failures"].append(f"{c['dir']}: skipped {c['skipped']}")
        for f in sorted(glob.glob(os.path.join(d, "*.json"))):
            if not is_checked(f):
                continue
            out["reports"] += 1
            rel = os.path.relpath(f, root)
            try:
                v = json.load(open(f)).get("validity")
            except Exception as e:
                out["failures"].append(f"{rel}: unreadable ({e})")
                continue
            if v is None:
                out["unchecked"].append(rel)  # missing validity is never valid
            elif v.get("valid") is True:
                out["valid_reports"] += 1
            else:
                bad = [k["name"] for k in v.get("checks", []) if k.get("status") != "pass"]
                out["invalid"].append(f"{rel}: {','.join(bad) or 'valid=false'}")
    for s in scenarios:
        for fw in frameworks:
            if f"{s}/{fw}" not in out["matrix"]:
                out["failures"].append(f"no cell for {s}/{fw}")
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
    if os.path.isdir(a.root):
        open(os.path.join(a.root, "VALIDITY.txt"), "w").write(text + "\n")
        json.dump(r, open(os.path.join(a.root, "VALIDITY.json"), "w"), indent=1)
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
