#!/usr/bin/env python3
"""Record one reproducible headless baseline on this Mac, or compare two recorded runs.

    python3 bench/baseline/run.py --name 71ee3be --worktree ../baseline
    python3 bench/baseline/run.py --name try1 --worktree . --cells sheet_x,events_x
    python3 bench/baseline/compare.py base.json head.json [--threshold-file ...]

`run.py` (this file) resolves the commit and toolchain, builds an exact list of binaries into a
target directory of its own, records their sha256 and size, then runs every cell in REP rounds in a
deterministically shuffled order. Each run is `/usr/bin/time -l` under one process, so it reports
retired instructions (process wide), wall time measured here, and the maximum resident set size.
The output is one JSON per run and a markdown table beside it.

A cell that fails, times out or cannot be built is recorded with its error and makes the exit
status 1: the baseline is fail-closed. Cells for scenes another agent is still building are
present as `unavailable` slots, so a later run fills a slot instead of inventing one.

The measurement lock is the caller's: `bench/baseline/run.sh` takes
`lockf -k $HOME/work/locks/bench.lock` around this script.
"""
import argparse
import hashlib
import json
import os
import platform
import random
import re
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
DEFAULT_TARGET = Path(os.environ.get("BENCH_BASELINE_TARGET", str(Path.home() / "work/targets/bench/baseline")))
RUSTUP_BIN = "/opt/homebrew/opt/rustup/bin"

# The scenes other agents are building. A baseline names them, empty, so no later reader mistakes
# a missing scene for a measured zero.
SLOTS = {
    "rows": "bough-guibench headless, rows (measured below as guibench/rows-*)",
    "virtual": "not yet available: bough's virtual list cells are gui-bench's `virtual` scenario on Linux",
    "table": "bough-guibench headless table cells (measured below as guibench/table-*)",
    "typing": "bough-guibench headless typing (measured below)",
    "search": "bough-guibench headless filter (measured below)",
    "spinner": "not yet available: needs the output-paced animation driver (REPORT 4.8)",
    "ticker": "not yet available: same clock as spinner",
    "resize": "not yet available on the new core; the façade's resize is the gui-bench VM cell",
    "sheet": "bough-x sheet_x example (measured below)",
    "chat": "not yet available: the element-layer chat scene (REPORT 4.1)",
    "ide": "not yet available: the element-layer ide scene (REPORT 4.1)",
    "dashboard": "not yet available: the element-layer dashboard scene (REPORT 4.1)",
    "canvas": "bough-paint pbench canvas (measured below)",
    "effects": "not yet available: the effects/motion lane's scene (slot reserved)",
    "media": "not yet available: the video/image scene (slot reserved)",
}


def sh(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def rust(cmd, cwd, env):
    return subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True)


def build(worktree, target, log):
    """Build every binary the cells name. Returns {label: path}, or raises with the failing command."""
    env = dict(os.environ)
    env["PATH"] = RUSTUP_BIN + ":" + env.get("PATH", "")
    env["RUSTC_WRAPPER"] = ""
    env["CARGO_TARGET_DIR"] = str(target)
    steps = [
        ("bough-x examples", ["cargo", "build", "--profile", "dist", "-p", "bough-x",
                              "--example", "sheet_x", "--example", "events_x", "--example", "text_x", "--example", "anim_x"]),
        ("bough-paint pbench", ["cargo", "build", "--profile", "dist", "-p", "bough-paint", "--example", "pbench"]),
        ("bough-guibench-x", ["cargo", "build", "--release", "-p", "bough-guibench-x"]),
        ("bough-guibench headless", ["cargo", "build", "--release",
                                     "--manifest-path", "comparisons/bough-guibench/Cargo.toml",
                                     "--bin", "headless"]),
    ]
    for label, cmd in steps:
        t = time.monotonic()
        p = rust(cmd, worktree, env)
        log.append(f"$ {' '.join(cmd)}  ({time.monotonic()-t:.0f}s, rc {p.returncode})")
        if p.returncode != 0:
            raise SystemExit(f"build failed: {label}\n" + p.stderr[-3000:])
    return {
        "sheet_x": target / "dist/examples/sheet_x",
        "events_x": target / "dist/examples/events_x",
        "text_x": target / "dist/examples/text_x",
        "anim_x": target / "dist/examples/anim_x",
        "pbench": target / "dist/examples/pbench",
        "bough-x-bench": target / "release/bough-bench",
        "headless": target / "release/headless",
    }


def cells(bins):
    """The cells: (id, argv, env, parse). Each is one whole process."""
    h = str(bins["headless"])
    return [
        ("sheet_x/rows1000", [str(bins["sheet_x"])], {"ROWS": "1000", "REPS": "16"}, "sheet"),
        ("sheet_x/rows10000", [str(bins["sheet_x"])], {"ROWS": "10000", "REPS": "16"}, "sheet"),
        ("events_x/rows10000", [str(bins["events_x"])], {"ROWS": "10000"}, "events"),
        ("text_x", [str(bins["text_x"])], {}, "text"),
        ("anim_x/208", [str(bins["anim_x"])], {"H": "208"}, "anim"),
        ("pbench/canvas1", [str(bins["pbench"]), "canvas", "1"], {"REPS": "8", "WORKERS": "5"}, "pbench"),
        ("guibench/rows-1000-mount", [h, "--scenario", "rows", "--rows", "1000", "--op", "mount", "--repeats", "0"], {}, "guibench"),
        ("guibench/rows-1000-hover", [h, "--scenario", "rows", "--rows", "1000", "--op", "hover", "--repeats", "8", "--blocks", "3"], {}, "guibench"),
        ("guibench/rows-1000-typing", [h, "--scenario", "typing", "--rows", "1000", "--op", "typing", "--repeats", "8", "--blocks", "3"], {}, "guibench"),
        ("guibench/rows-1000-filter", [h, "--scenario", "search", "--rows", "1000", "--op", "filter", "--repeats", "6", "--blocks", "3"], {}, "guibench"),
    ]


INSTR = re.compile(r"(\d+)\s+instructions retired")
RSS = re.compile(r"(\d+)\s+maximum resident set size")
REALT = re.compile(r"([\d.]+)\s+real")
NUM = re.compile(r"^([A-Za-z_][\w/ .-]*?)\s+([0-9][0-9,]*)\s*$")


def parse_stdout(kind, out):
    """The example's own figures, kept as text and as simple name -> number pairs where they parse."""
    fields = {}
    for line in out.splitlines():
        m = NUM.match(line.strip())
        if m:
            fields[m.group(1).strip().replace(" ", "_")] = int(m.group(2).replace(",", ""))
        elif "\t" in line and kind in ("sheet", "events", "text", "anim", "pbench"):
            p = line.split("\t")
            if len(p) >= 3 and p[-1].replace(",", "").isdigit():
                fields[f"{p[0]}/{p[-2] if len(p) > 3 else len(p)}"] = int(p[-1].replace(",", ""))
    return fields


def measure(argv, env, kind, timeout=600):
    cmd = ["/usr/bin/time", "-l"] + argv
    t = time.monotonic()
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, env={**os.environ, **env}, timeout=timeout)
    except subprocess.TimeoutExpired as e:
        return {"error": f"timeout after {timeout}s", "stdout": (e.stdout or "")[-2000:], "stderr": (e.stderr or "")[-2000:]}
    wall = time.monotonic() - t
    instr = INSTR.search(p.stderr)
    rss = RSS.search(p.stderr)
    real = REALT.search(p.stderr)
    r = {
        "exit": p.returncode,
        "wall_ms": round(wall * 1000, 3),
        "time_real_s": float(real.group(1)) if real else None,
        "instructions": int(instr.group(1)) if instr else None,
        "rss_bytes": int(rss.group(1)) if rss else None,
        "stdout": p.stdout[-20000:],
        "stderr": p.stderr[-4000:],
        "fields": parse_stdout(kind, p.stdout),
    }
    if p.returncode != 0:
        r["error"] = f"exit {p.returncode}"
    return r


def summarise(runs, key):
    xs = [r[key] for r in runs if isinstance(r, dict) and r.get(key) is not None]
    if not xs:
        return None
    return {"median": statistics.median(xs), "min": min(xs), "max": max(xs), "n": len(xs)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--name", required=True)
    ap.add_argument("--worktree", default=str(ROOT))
    ap.add_argument("--target-dir", default=None)
    ap.add_argument("--cells", default="")
    ap.add_argument("--reps", type=int, default=3)
    ap.add_argument("--out", default=None)
    a = ap.parse_args()
    wt = Path(a.worktree).resolve()
    commit = sh(["git", "rev-parse", "HEAD"], cwd=wt).stdout.strip()
    dirty = bool(sh(["git", "status", "--porcelain"], cwd=wt).stdout.strip())
    rustc_v = sh(["rustc", "-V"], cwd=wt, env={**os.environ, "PATH": RUSTUP_BIN + ":" + os.environ["PATH"]}).stdout.strip()
    target = Path(a.target_dir).resolve() if a.target_dir else DEFAULT_TARGET / f"{a.name}-{commit[:10]}"
    outdir = Path(a.out).resolve() if a.out else HERE.parent / "baselines" / f"macos-{a.name}"
    outdir.mkdir(parents=True, exist_ok=True)
    log = [f"worktree {wt}", f"commit {commit}", f"dirty {dirty}", f"rustc {rustc_v}", f"target {target}"]
    load = os.getloadavg()
    log.append(f"load-before {load[0]:.2f} {load[1]:.2f} {load[2]:.2f}")

    bins = build(wt, target, log)
    manifest = {}
    for label, p in bins.items():
        if p.exists():
            b = p.read_bytes()
            manifest[label] = {"path": str(p.relative_to(target)), "sha256": hashlib.sha256(b).hexdigest(), "bytes": len(b),
                               "text_bytes": text_bytes(p)}
        else:
            manifest[label] = {"error": "missing"}
    log.append("binaries " + json.dumps(manifest, sort_keys=True))

    wanted = set(filter(None, a.cells.split(",")))
    todo = [c for c in cells(bins) if not wanted or c[0] in wanted]
    missing = wanted - {c[0] for c in todo}
    if missing:
        raise SystemExit(f"no such cell: {sorted(missing)}")

    order = list(range(a.reps))
    random.Random(int(commit[:8], 16) if re.fullmatch(r"[0-9a-f]+", commit) else 0).shuffle(order)
    records, errors = {}, []
    for rep in order:
        for cid, argv, env, kind in todo:
            r = measure(argv, env, kind)
            r["rep"] = rep
            r["load"] = os.getloadavg()[0]
            if "error" in r:
                errors.append((cid, r["error"]))
            records.setdefault(cid, []).append(r)
            status = r.get("error") or f"{r.get('instructions')} instr  {r.get('wall_ms')} ms"
            print(f"{cid:<28} rep {rep}  {status}", flush=True)
    # Fail closed on a load ceiling: a run above it is kept but marked.
    load_after = os.getloadavg()
    cells_out = {}
    for cid, runs in records.items():
        cells_out[cid] = {
            "runs": runs,
            "instructions": summarise(runs, "instructions"),
            "wall_ms": summarise(runs, "wall_ms"),
            "rss_bytes": summarise(runs, "rss_bytes"),
            "fields": {k: [r.get("fields", {}).get(k) for r in runs] for k in sorted({k for r in runs for k in r.get("fields", {})})},
        }
    doc = {
        "format": 1,
        "name": a.name,
        "commit": commit,
        "dirty": dirty,
        "rustc": rustc_v,
        "profile": "release for apps; dist for the bough-x/bough-paint examples",
        "features": "default",
        "host": {"os": platform.mac_ver()[0], "machine": platform.machine(),
                 "cpu": subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True).stdout.strip(),
                 "cpus": os.cpu_count()},
        "load": {"before": load, "after": load_after, "ceiling": 2.0,
                 "ok": max(load[0], load_after[0]) <= 2.0},
        "binaries": manifest,
        "cells": cells_out,
        "errors": errors,
        "slots": SLOTS,
        "log": log,
        "date": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }
    jp = outdir / f"{a.name}.json"
    jp.write_text(json.dumps(doc, indent=2) + "\n")
    md = ["# baseline " + a.name, "", f"commit `{commit}`{' (dirty)' if dirty else ''} · {rustc_v}",
          f"host {doc['host']['cpu']} ({doc['host']['cpus']} cores), macOS {doc['host']['os']}",
          f"load {load[0]:.2f} → {load_after[0]:.2f} (ceiling 2.0: {'ok' if doc['load']['ok'] else 'LOADED'})", "",
          "| cell | instructions (median [min–max]) | wall ms | RSS MiB |", "|---|---:|---:|---:|"]
    for cid, c in cells_out.items():
        i, w, r = c["instructions"], c["wall_ms"], c["rss_bytes"]
        rss = f"{r['median']/1048576:.1f}" if r else "-"
        md.append(f"| {cid} | {fmt(i, ',')} | {fmt(w, '.1f')} | {rss} |")
    md += ["", "## Slots (scenes other agents are building)", ""]
    md += [f"* `{k}`: {v}" for k, v in SLOTS.items()]
    (outdir / f"{a.name}.md").write_text("\n".join(md) + "\n")
    print(f"baseline: wrote {jp} and {jp.with_suffix('.md')}")
    if errors:
        print(f"baseline: {len(errors)} cell error(s): {errors}", file=sys.stderr)
    return 1 if errors else 0


def fmt(s, spec):
    if not s:
        return "-"
    return f"{s['median']:{spec}} [{s['min']:{spec}}–{s['max']:{spec}}]"


def text_bytes(p):
    m = sh(["size", "-m", str(p)])
    if m.returncode != 0:
        return None
    for line in m.stdout.splitlines():
        if "__text" in line:
            parts = line.split()
            try:
                return int(parts[2])
            except (IndexError, ValueError):
                return None
    return None


if __name__ == "__main__":
    sys.exit(main())
