#!/usr/bin/env python3
"""Condense one raw bench/vm result tree into one compact JSON.

usage: summarize_vm_results.py RAW_RUN_DIR OUT.json

RAW_RUN_DIR is bench/results/vm/<stamp>/ as written by bench/vm/run.sh.
The summary keeps CONTRACT fields, per-cell state (valid / invalid / missing / error),
validity failures, startup fields and per-metric median+n. It drops pids, commands,
paths, display make/model/serial, hostnames, logs and screenshots. Stdlib only.
"""
import json, os, sys, re

FRAMEWORK_SCOPE = {
    "bough": "facade crate; CPU executor into wl_shm",
    "bough-x": "new core; CPU executor into wl_shm",
    "zgui": "wgpu on lavapipe (software Vulkan)",
}
STARTUP_KEYS = ["connected_ms", "first_surface_ms", "toplevel_ms", "first_buffer_ms",
                "first_present_ms", "visually_complete_ms", "responsive_ms",
                "first_input_latency_ms", "exited_early"]
CONTRACT_DROP = {"vm", "guest-memory"}  # vm line carries the instance name


def load(p):
    try:
        with open(p) as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def contract(path):
    out = {}
    try:
        for line in open(path):
            k, _, v = line.rstrip("\n").partition(" ")
            if k and k not in CONTRACT_DROP:
                out[k] = v.strip()
    except OSError:
        out["_missing"] = "CONTRACT.txt"
    return out


def report(d):
    r = {}
    v = d.get("validity") or {}
    r["valid"] = v.get("valid")
    fails = [{"check": c.get("name"), "status": c.get("status"), "message": c.get("message")}
             for c in v.get("checks", []) if c.get("status") != "pass"]
    if fails:
        r["failed_checks"] = fails
    its = d.get("iterations") or []
    r["iterations"] = len(its)
    r["iterations_failed"] = sum(1 for i in its if not i.get("ok", True))
    lw = [i.get("late_wakes") or 0 for i in its]
    ml = [i.get("max_lateness") or 0 for i in its]
    if its:
        r["late_wakes_total"] = sum(lw)
        r["max_lateness_ms"] = round(max(ml) / 1e6, 3)  # source is ns
    m = {}
    for s in d.get("sections", []):
        for x in s.get("metrics", []):
            if x.get("median") is None:
                continue
            key = f'{s.get("name")}.{x.get("name")}' + (f'[{x["variant"]}]' if x.get("variant") else "")
            m[key] = {"median": round(x["median"], 4), "n": x.get("n"), "unit": x.get("unit")}
    r["metrics"] = m
    return r


def cell(rawdir, entry, framework_timing):
    c = entry["cell"]
    base = os.path.join(rawdir, entry["dir"])
    out = {"scenario": c["scenario"], "framework": c["framework"], "variant": c["variant"],
           "hz": c["hz"], "rows": c.get("rows"), "updates": c.get("updates"),
           "framework_scope": FRAMEWORK_SCOPE.get(c["framework"], "unknown"),
           "timing_scope": framework_timing}
    present = {f[:-5] for f in os.listdir(base) if f.endswith(".json")} if os.path.isdir(base) else set()
    done = entry.get("done", [])
    out["errors"] = entry.get("errors", [])
    out["skipped"] = entry.get("skipped", [])
    out["done"] = done
    # 'layout' is listed done by the runner but writes no JSON in these trees
    out["missing"] = sorted(s for s in done if s not in present)
    st = load(os.path.join(base, "startup.json"))
    if st:
        out["startup"] = {k: st.get(k) for k in STARTUP_KEYS}
    reps = {}
    for name in sorted(present - {"cell", "startup"}):
        d = load(os.path.join(base, name + ".json"))
        if d:
            reps[name] = report(d)
    out["reports"] = reps
    vals = [r["valid"] for r in reps.values() if r["valid"] is not None]
    if out["errors"]:
        out["state"] = "error"
    elif vals and not all(vals):
        out["state"] = "invalid"  # reported, never ranked
    elif vals:
        out["state"] = "valid"
    else:
        out["state"] = "startup-only"  # no validity check exists for startup
    out["rankable"] = out["state"] == "valid"
    return out


def main(rawdir, outp):
    run = load(os.path.join(rawdir, "run.json"))
    if run is None:
        sys.exit("run.json missing or unreadable: " + rawdir)
    con = contract(os.path.join(rawdir, "CONTRACT.txt"))
    timing = con.get("timing", "unknown")
    cells = [cell(rawdir, e, timing) for e in run["cells"]]
    counts = {}
    for c in cells:
        counts[c["state"]] = counts.get(c["state"], 0) + 1
    out = {"summary_version": 1, "run": os.path.basename(os.path.normpath(rawdir)),
           "contract": con, "cell_states": counts, "cells": cells,
           "note": "VM, software rendering; never pooled with native macOS numbers. "
                   "invalid = fakeland output wake lateness above threshold; reported, not ranked."}
    with open(outp, "w") as f:
        json.dump(out, f, separators=(",", ":"), sort_keys=True)
        f.write("\n")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(*sys.argv[1:])
