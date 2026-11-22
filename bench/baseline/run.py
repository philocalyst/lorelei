#!/usr/bin/env python3
"""Run the native measurement matrix with optional per-fixture timing locks."""
import argparse
from contextlib import nullcontext
from dataclasses import dataclass
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import re
import statistics
import subprocess
import sys
import time

from operations import parse


ROOT = Path(__file__).resolve().parents[2]
LOCKFILES = ("Cargo.lock", "comparisons/bough-guibench/Cargo.lock",
             "comparisons/bough-fx-bench/Cargo.lock")


@dataclass(frozen=True)
class Cell:
    name: str
    binary: str
    argv: tuple
    env: dict
    parser: str
    check: tuple = ()


def example(name, parser, **env):
    return Cell(name, name.split("/")[0], (), env, parser)


def cells(worktree):
    matrix = [
        example("sheet_x/rows1000", "sheet", ROWS="1000", REPS="32"),
        example("sheet_x/rows10000", "sheet", ROWS="10000", REPS="32"),
        example("events_x/rows10000", "events", ROWS="10000", REPS="31"),
        example("text_x", "text", ROWS="3000", REPS="64"),
        example("anim_x/208", "anim", H="208"),
        Cell("pbench/canvas1", "pbench", ("canvas", "1"), {"REPS": "8", "WORKERS": "5"}, "pbench"),
    ]
    for scene, rows, op, repeats in (
        ("rows", 1000, "mount", 0), ("rows", 1000, "hover", 8),
        ("typing", 1000, "typing", 8), ("search", 100, "filter", 6),
        ("search", 1000, "filter-nochange", 6),
    ):
        label = "rows" if scene == "typing" else scene
        args = ("--scenario", scene, "--rows", str(rows), "--op", op, "--repeats", str(repeats))
        if op != "mount":
            # The no-change search trace reaches a steady allocation regime after the short capture.
            args += ("--blocks", "15" if op == "filter-nochange" else "3")
        matrix.append(Cell(f"guibench/{label}-{rows}-{op}", "headless", args, {}, "guibench"))
    unavailable = {}
    core = worktree / "crates" / core_package(worktree)
    if 'var("BROAD")' in (core / "examples/sheet_x.rs").read_text():
        matrix.append(example("sheet_x/rows10000/broad", "sheet", ROWS="10000", REPS="32", BROAD="1"))
    else:
        unavailable["sheet_x/rows10000/broad"] = "this checkout lacks the BROAD sheet fixture"
    if (core / "examples/ops_x.rs").is_file():
        for part in ("select", "lane", "ops", "search", "corpus"):
            matrix.append(example(f"ops_x/{part}", "ops", PART=part, REPS="64", ROWS="100000"))
        for mode, env in (("near", {"NEAR": "1"}), ("swept", {"SWEPT": "1"}),
                          ("near-swept", {"NEAR": "1", "SWEPT": "1"})):
            matrix.append(example(f"anim_x/208/{mode}", "anim", H="208", **env))
    else:
        unavailable["incremental"] = "ops_x and near/swept variants require incremental operators"
    if (worktree / "comparisons/bough-fx-bench/Cargo.toml").is_file():
        for count in ("200", "1000", "5000"):
            for op in ("startup", "idle", "one", "many", "hover", "theme", "scroll", "resize"):
                args = (("cold", count, op, "1") if op == "startup" else
                        ("measure", count, op, "1" if op == "idle" else "20"))
                check = ("check", count, op, "20")
                matrix.append(Cell(f"effects/{count}/{op}", "bough-fx-bench", args, {}, "effects", check))
            for op in ("startup", "idle", "one", "many", "theme"):
                args = ("measure", count, op)
                matrix.append(Cell(f"effects-core/{count}/{op}", "typed-core", args, {}, "effects-core",
                                   ("check", count, op)))
    else:
        unavailable["effects"] = "the effects adapter is absent from this checkout"
    if (worktree / "crates/bough-paint/examples/fxbench.rs").is_file():
        for scene, sizes in (("effects", (300, 3000)), ("pane", (300, 3000)), ("media", (4, 24, 48))):
            for size in sizes:
                matrix.append(Cell(f"fxbench/{scene}/{size}", "fxbench", (scene, str(size)),
                                   {"REPS": "24"}, "fxbench"))
        matrix.append(Cell("fxbench/effects/600-dense", "fxbench", ("effects", "600", "dense"),
                           {"REPS": "6"}, "fxbench"))
        for scene, size in (("canvas", "4"), ("anim", "600")):
            matrix.append(Cell(f"pbench/{scene}{size}", "pbench", (scene, size),
                               {"REPS": "24", "WORKERS": "5"}, "pbench"))
    else:
        unavailable["fxbench"] = "the staged paint and video adapter is absent from this checkout"
    unavailable["windowed"] = "see the VM runbook for compositor scenarios"
    if not (worktree / "crates/bough/tests/glyph_reclaim.rs").is_file():
        unavailable["guibench/search-1000-filter"] = "historical glyph_x capacity panic"
    else:
        args = ("--scenario", "search", "--rows", "1000", "--op", "filter",
                "--repeats", "6", "--blocks", "3")
        matrix.append(Cell("guibench/search-1000-filter", "headless", args, {}, "guibench"))
    return matrix, unavailable


def core_package(worktree):
    # The typed core was `bough-x` before the rename; a baseline may be built from such a checkout.
    return "bough-dataflow" if (worktree / "crates/bough-dataflow").is_dir() else "bough-x"


def binary_spec(name, worktree):
    if name == "headless":
        return "release/headless", ["--release", "--manifest-path", "comparisons/bough-guibench/Cargo.toml",
                                    "--bin", "headless"]
    if name in ("bough-fx-bench", "typed-core"):
        return f"release/{name}", ["--locked", "--release", "--manifest-path",
                                   "comparisons/bough-fx-bench/Cargo.toml",
                                   "--features", "full-effects,typed-core", "--bin", name]
    package = "bough-paint" if name in ("pbench", "fxbench") else core_package(worktree)
    return f"dist/examples/{name}", ["--locked", "--profile", "dist", "-p", package, "--example", name]


def fetch(argv, cwd=None, env=None):
    return subprocess.run(argv, cwd=cwd, env=env, capture_output=True, text=True, check=True).stdout.strip()


def digest(path):
    checksum = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            checksum.update(chunk)
    return checksum.hexdigest()


def source(worktree):
    commit = fetch(["git", "rev-parse", "HEAD"], worktree)
    status = fetch(["git", "status", "--porcelain"], worktree)
    changes = fetch(["git", "diff", "--binary", "HEAD"], worktree) if status else ""
    untracked = fetch(["git", "ls-files", "--others", "--exclude-standard", "-z"], worktree)
    for name in filter(None, untracked.split("\0")):
        changes += f"\n{name}\0{digest(worktree / name)}"
    locks = {name: digest(worktree / name) for name in LOCKFILES if (worktree / name).is_file()}
    return {"commit": commit, "dirty": bool(status), "changes_sha256": hashlib.sha256(changes.encode()).hexdigest(),
            "locks": locks}


def binary_manifest(path, relative, args):
    sections = fetch(["size", "-m", str(path)])
    size = re.search(r"^\s*Section __text: (\d+)", sections, re.M)
    if not size:
        raise ValueError(f"missing Mach-O text section for {relative}")
    return {"path": relative, "cargo_args": args, "sha256": digest(path),
            "bytes": path.stat().st_size, "text_bytes": int(size[1])}


def write_json(path, doc):
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(doc, indent=2, allow_nan=False) + "\n")
    tmp.replace(path)


def build(worktree, target, binaries, toolchain, log, reuse):
    cache = target / ".baseline-binaries.json"
    before = source(worktree)
    env = {**os.environ, "RUSTC_WRAPPER": "", "CARGO_TARGET_DIR": str(target)}
    if reuse:
        previous = json.loads(cache.read_text())
        if (previous["worktree"], previous["source"], previous["toolchain"]) != (str(worktree), before, toolchain):
            raise ValueError("reuse target was built for another worktree, source or toolchain")
    else:
        if cache.exists() and json.loads(cache.read_text())["worktree"] != str(worktree):
            raise ValueError("target directory belongs to a different worktree")
        cache.unlink(missing_ok=True)
    manifest = {}
    for name in binaries:
        relative, args = binary_spec(name, worktree)
        if not reuse:
            command = ["nice", "-n", "15", "cargo", "build", "-j", "3", *args]
            print(f"build {name}", flush=True)
            with (log / f"build-{name}.log").open("w") as stream:
                stream.write(f"$ {' '.join(command)}\n")
                stream.flush()
                subprocess.run(command, cwd=worktree, env=env, stdout=stream, stderr=subprocess.STDOUT, check=True)
        manifest[name] = binary_manifest(target / relative, relative, args)
        if reuse and manifest[name] != previous["binaries"].get(name):
            raise ValueError(f"binary identity changed for {name}; rebuild before timing")
    after = source(worktree)
    if before != after:
        # The historical standalone façade app resolves its path dependencies in its own lockfile.
        changes = fetch(["git", "status", "--porcelain"], worktree).splitlines()
        expected = f"M {LOCKFILES[1]}"
        if before["dirty"] or changes != [expected] or before["commit"] != after["commit"]:
            raise ValueError("source changed during build")
    if not reuse:
        write_json(cache, {"worktree": str(worktree), "source": after,
                           "toolchain": toolchain, "binaries": manifest})
    return after, manifest


ENV_KEYS = set("ROWS REPS H NEAR SWEPT BROAD PART COLD WORKERS BOUGH_PMU SKIP_K".split())
SCOPES = {
    "sheet": "calling-thread instructions per operation",
    "events": "calling-thread dispatch and dispatch-plus-settle instructions per operation",
    "text": "calling-thread instructions per operation",
    "anim": "calling-thread instructions per operation",
    "ops": "calling-thread instructions per operation",
    "guibench": "calling-thread instructions for mount or the median per operation across timed blocks",
    "effects": "process instructions per operation, mean of trace, or spawn-to-first for startup",
    "effects-core": "process instructions per operation, median of trace, or setup-to-first for startup",
    "fxbench": "calling-thread instructions per frame, rounded to 100 instructions by its TSV",
    "pbench": "serial CPU calling-thread instructions per phase, rounded to 100 instructions by its TSV",
}


def cell_env(cell):
    return {**{name: value for name, value in os.environ.items() if name not in ENV_KEYS}, **cell.env}


def check_cell(cell, binary, cwd, log):
    with log.open("w") as stream:
        subprocess.run([str(binary), *cell.check], cwd=cwd, env=cell_env(cell), timeout=600,
                       stdout=stream, stderr=subprocess.STDOUT, check=True)


def measure(cell, binary, cwd, log, timing_lock):
    command = ["/usr/bin/time", "-l", str(binary), *cell.argv]
    lock = timing_lock.open("a") if timing_lock is not None else nullcontext()
    with log.with_suffix(".stdout").open("w+") as out, log.with_suffix(".stderr").open("w+") as err:
        with lock as lock_file:
            if lock_file is not None:
                fcntl.flock(lock_file, fcntl.LOCK_EX)
            load_before = os.getloadavg()[0]
            start = time.monotonic_ns()
            subprocess.run(command, cwd=cwd, env=cell_env(cell), stdout=out, stderr=err, timeout=600, check=True)
            elapsed = time.monotonic_ns() - start
            load_after = os.getloadavg()[0]
        out.seek(0)
        err.seek(0)
        stdout, stderr = out.read(), err.read()
    process = {"wall_ms": elapsed / 1_000_000}
    for key, label in (("instructions", "instructions retired"), ("rss_bytes", "maximum resident set size")):
        match = re.search(rf"^\s*(\d+)\s+{label}\s*$", stderr, re.M)
        if not match or int(match[1]) == 0:
            raise ValueError(f"missing process {key}")
        process[key] = int(match[1])
    return {"process": process, "operations": parse(cell.parser, stdout, stderr, cell.argv, cell.env),
            "load": max(load_before, load_after)}


def summary(values):
    return {"median": statistics.median(values), "min": min(values), "max": max(values), "n": len(values)}


def summarize(runs):
    first = runs[0]
    shape = {key: set(counters) for key, counters in first["operations"].items()}
    if any({key: set(row) for key, row in run["operations"].items()} != shape for run in runs):
        raise ValueError("operation names or counters changed between repetitions")
    return {
        "process": {key: summary([run["process"][key] for run in runs]) for key in first["process"]},
        "operations": {op: {key: summary([run["operations"][op][key] for run in runs])
                            for key in first["operations"][op]} for op in shape},
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--name", required=True)
    parser.add_argument("--worktree", default=str(ROOT))
    parser.add_argument("--target-dir")
    parser.add_argument("--cells", default="")
    parser.add_argument("--reps", type=int, default=3)
    parser.add_argument("--out")
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--timing", action="store_true", help="lock each process sample for wall time and RSS")
    parser.add_argument("--expected-inputs-from", help="fail before building if reference input digests differ")
    args = parser.parse_args()
    if not 1 <= args.reps or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", args.name):
        parser.error("use at least one repetition and a filesystem-safe name")
    worktree = Path(args.worktree).resolve()
    if Path(fetch(["git", "rev-parse", "--show-toplevel"], worktree)) != worktree:
        parser.error("--worktree must name a repository root")
    runtime_inputs = {}
    for name in ("BOUGH_FONT", "ZGX_FONT_FILE"):
        if value := os.environ.get(name):
            runtime_inputs[name] = digest(Path(value))
    if args.expected_inputs_from:
        expected = json.loads(Path(args.expected_inputs_from).read_text()).get("runtime_inputs")
        if expected != runtime_inputs:
            raise ValueError(f"runtime inputs differ from reference (expected SHA-256: {expected})")
    build_source = source(worktree)
    matrix, unavailable = cells(worktree)
    wanted = set(args.cells.split(",")) if args.cells else {cell.name for cell in matrix}
    unknown = wanted - {cell.name for cell in matrix}
    if unknown:
        parser.error(f"unavailable or unknown cells: {', '.join(sorted(unknown))}")
    matrix = [cell for cell in matrix if cell.name in wanted]
    root_target = Path(os.environ.get("BENCH_BASELINE_TARGET", os.environ.get("CARGO_TARGET_DIR",
                       ROOT / "target" / "baseline")))
    target = Path(args.target_dir).resolve() if args.target_dir else (root_target / build_source["commit"][:12]).resolve()
    out = Path(args.out).resolve() if args.out else ROOT / "runtime" / "baseline" / args.name
    log = out / "raw"
    log.mkdir(parents=True, exist_ok=True)
    for suffix in (".json", ".md"):
        (out / (args.name + suffix)).unlink(missing_ok=True)
    toolchain = fetch(["rustc", "-vV"], worktree)
    harness = {name: digest(Path(__file__).with_name(name)) for name in ("run.py", "operations.py")}
    host = {"os": platform.mac_ver()[0], "machine": platform.machine(),
            "cpu": fetch(["sysctl", "-n", "machdep.cpu.brand_string"]), "cpus": os.cpu_count()}
    timing_lock = None
    if args.timing:
        timing_lock = Path(os.environ.get("BENCH_LOCK", Path.home() / "work/locks/bench.lock")).resolve()
        timing_lock.parent.mkdir(parents=True, exist_ok=True)
        timing_lock.touch()
    load_before = os.getloadavg()
    records = {cell.name: [] for cell in matrix}
    errors = []
    manifest = {}
    order = []
    try:
        build_source, manifest = build(worktree, target, sorted({cell.binary for cell in matrix}),
                                       toolchain, log, args.no_build)
    except (OSError, ValueError, subprocess.SubprocessError, KeyError) as error:
        print(f"baseline: build unavailable: {error}", file=sys.stderr)
        errors.append({"stage": "build", "reason": type(error).__name__})
    failed_checks = set()
    if manifest:
        for cell in matrix:
            if not cell.check:
                continue
            binary = target / manifest[cell.binary]["path"]
            try:
                check_cell(cell, binary, worktree, log / f"check-{cell.name.replace('/', '_')}.log")
            except (OSError, subprocess.SubprocessError) as error:
                print(f"{cell.name} correctness gate FAILED: {error}", file=sys.stderr)
                errors.append({"cell": cell.name, "stage": "correctness", "reason": type(error).__name__})
                failed_checks.add(cell.name)
    random_order = random.Random(71)
    for rep in range(args.reps) if manifest else ():
        batch = matrix.copy()
        random_order.shuffle(batch)
        order.append([cell.name for cell in batch])
        for cell in batch:
            if cell.name in failed_checks:
                continue
            binary = target / manifest[cell.binary]["path"]
            path = log / f"{rep}-{cell.name.replace('/', '_')}"
            try:
                measured = measure(cell, binary, worktree, path, timing_lock)
                records[cell.name].append(measured)
                print(f"{cell.name} rep {rep}: {len(measured['operations'])} operation rows", flush=True)
            except (OSError, ValueError, subprocess.SubprocessError, KeyError) as error:
                print(f"{cell.name} rep {rep} FAILED: {error}", file=sys.stderr)
                errors.append({"cell": cell.name, "rep": rep, "reason": type(error).__name__})
    for name, binary in manifest.items():
        if digest(target / binary["path"]) != binary["sha256"]:
            errors.append({"stage": "replay", "reason": f"{name} changed during replay"})
    if manifest and source(worktree) != build_source:
        errors.append({"stage": "replay", "reason": "source changed during replay"})
    results = {}
    for cell in matrix:
        runs = records[cell.name]
        result = {"binary": cell.binary, "argv": list(cell.argv), "env": cell.env, "parser": cell.parser,
                  "load": [run["load"] for run in runs], "valid": len(runs) == args.reps,
                  "counter_scope": SCOPES[cell.parser], "check": list(cell.check)}
        if result["valid"]:
            try:
                result.update(summarize(runs))
            except ValueError as error:
                result["valid"] = False
                errors.append({"cell": cell.name, "reason": str(error)})
        results[cell.name] = result
    load_after = os.getloadavg()
    peak_load = max(load_before[0], load_after[0], *(v for cell in results.values() for v in cell["load"]))
    doc = {
        "format": 2, "name": args.name, "commit": build_source["commit"], "source": build_source,
        "rustc": toolchain, "host": host, "harness": harness, "reps": args.reps, "order": order,
        "load": {"before": load_before, "after": load_after, "peak": peak_load, "ceiling": 2.0, "ok": peak_load <= 2.0, "timing_locked": args.timing},
        "runtime_inputs": runtime_inputs, "binaries": manifest, "cells": results, "errors": errors,
        "unavailable": unavailable, "date": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "contract": "Native headless macOS. Typed core: prepared Latin-1 DejaVu Sans 14/28 px, opaque colors, "
                    "arithmetic rows, remount on resize, group-1 keymap and CPU spans. pbench includes offscreen Metal.",
    }
    write_json(out / f"{args.name}.json", doc)
    lines = [f"# {args.name}", "", f"Commit `{doc['commit']}`", "", doc["contract"], "",
             "| Cell | Operation | Median instructions |", "|---|---|---:|"]
    for name, cell in results.items():
        if not cell["valid"]:
            lines.append(f"| {name} | invalid | |")
            continue
        for op, counters in cell["operations"].items():
            for key in ("instr_p50", "dispatch_instr", "step_instr", "instructions"):
                if key in counters:
                    lines.append(f"| {name} | {op} ({key}) | {counters[key]['median']:,} |")
        if not cell["operations"]:
            lines.append(f"| {name} | process | {cell['process']['instructions']['median']:,} |")
    (out / f"{args.name}.md").write_text("\n".join(lines) + "\n")
    print(f"baseline: wrote {out / (args.name + '.json')} ({len(errors)} errors)")
    return 1 if errors or not matrix else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.SubprocessError) as exc:
        sys.exit(f"baseline: {exc}")
