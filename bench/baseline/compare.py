#!/usr/bin/env python3
"""Compare like-for-like native benchmark receipts and fail on a regression."""
import argparse
import json
import math
import re
from pathlib import Path
import sys


DEFAULT_LIMITS = {"instructions": 1.02, "wall": 1.10, "rss": 1.05}
PRIMARY = ("instr_p50", "dispatch_instr", "step_instr", "instructions", "dispatch_allocs")


LEGACY_NAME = re.compile(r"^(sheet|events|text|anim|ops|damage)_x(?=/|$)")


def build_args(args):
    # The typed core's package was `bough-x` in receipts taken before its rename, and its fixtures `sheet_x`,
    # `events_x`, ...; the build is the same.
    return ["bough-dataflow" if arg == "bough-x" else LEGACY_NAME.sub(r"\1", arg) for arg in args]


def current_names(doc):
    """A receipt with the fixtures' current names (`sheet_x/rows1000` was measured as `sheet/rows1000` is)."""
    doc["cells"] = {LEGACY_NAME.sub(r"\1", name): {**cell, **({"binary": LEGACY_NAME.sub(r"\1", cell["binary"])}
                                                            if "binary" in cell else {})}
                    for name, cell in doc["cells"].items()}
    if "binaries" in doc:
        doc["binaries"] = {LEGACY_NAME.sub(r"\1", name): spec for name, spec in doc["binaries"].items()}
    return doc


def numeric(value):
    return type(value) in (int, float) and math.isfinite(value)


def metric(value, repetitions=None):
    if not isinstance(value, dict):
        raise ValueError("missing metric")
    median, lo, hi = (value[k] for k in ("median", "min", "max"))
    count = value["n"]
    if (not all(map(numeric, (median, lo, hi))) or not 0 <= lo <= median <= hi or
            type(count) is not int or count <= 0 or (repetitions is not None and count != repetitions)):
        raise ValueError("incomplete or invalid metric")
    return value


def validate(doc):
    version, cells = doc["format"], doc["cells"]
    if type(version) is not int or version not in (1, 2) or not isinstance(cells, dict) or not cells or doc.get("errors") != []:
        raise ValueError("empty, unsupported, or failed receipt")
    if type(doc["load"]["ok"]) is not bool or not doc["host"] or not doc["rustc"]:
        raise ValueError("incomplete receipt identity")
    if version == 2:
        if type(doc["reps"]) is not int or doc["reps"] <= 0:
            raise ValueError("invalid repetitions")
        for name, cell in cells.items():
            if cell.get("valid") is not True or len(cell["load"]) != doc["reps"]:
                raise ValueError(f"invalid or incomplete cell {name}")
            for key in ("instructions", "wall_ms", "rss_bytes"):
                if metric(cell["process"][key], doc["reps"])["min"] == 0:
                    raise ValueError(f"zero process {key} for {name}")
            if not cell["operations"] and cell["parser"] != "pbench":
                raise ValueError(f"{name} has no operation counters")
            for op, counters in cell["operations"].items():
                if not (set(PRIMARY) - {"dispatch_allocs"}) & set(counters):
                    raise ValueError(f"{name}/{op} has no operation instruction counter")
                for value in counters.values():
                    metric(value, doc["reps"])


def ratio(base, head):
    if base == 0:
        return math.inf if head else 1.0
    return head / base


def spread(stats):
    return ratio(stats["min"], stats["max"]) - 1


def display(name, base, head, limit, compare=True, same_binary=False):
    b, h = base["median"], head["median"]
    r = ratio(b, h)
    bad = compare and r > limit and not same_binary
    if not compare:
        verdict = "UNQUALIFIED"
    elif bad:
        verdict = f"REGRESSED (limit {limit:g}x)"
    elif same_binary and r > limit:
        verdict = "VARIED (same binary)"
    elif h < b and r < 1 / limit:
        verdict = "IMPROVED"
    else:
        verdict = "OK"
    print(f"{name:<62} {b:>14,.2f} {h:>14,.2f} {r:>8.3f}  {verdict}")
    return bad


def read(path):
    return json.loads(Path(path).read_text())


def read_receipt(path):
    return current_names(read(path))


def read_limits(path):
    data = read(path) if path else {}
    if not isinstance(data, dict):
        raise ValueError("thresholds must be an object")
    data = {LEGACY_NAME.sub(r"\1", name): limits for name, limits in data.items()}
    for name, limits in data.items():
        if (not isinstance(limits, dict) or (limits.keys() - DEFAULT_LIMITS.keys()) or
                any(not numeric(value) or value < 1 for value in limits.values())):
            raise ValueError(f"invalid thresholds for {name}")
    return data


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("base")
    parser.add_argument("head")
    parser.add_argument("--thresholds")
    parser.add_argument("--allow-missing", action="store_true", help="allow baseline cells absent from head")
    parser.add_argument("--require-quiet", action="store_true")
    parser.add_argument("--require-same-cells", action="store_true", help="fail on added or missing measurement rows")
    parser.add_argument("--cells", default="", help="comma-separated exact cell or cell/operation names")
    args = parser.parse_args()
    if args.require_same_cells and args.allow_missing:
        parser.error("strict coverage cannot be combined with --allow-missing")
    base, head = read_receipt(args.base), read_receipt(args.head)
    validate(base)
    validate(head)
    if base["format"] != head["format"]:
        raise ValueError("receipt formats differ (v1 process totals, v2 operation counters)")
    legacy = base["format"] == 1
    identity = ("host", "rustc", "profile", "features") if legacy else ("host", "rustc", "contract", "runtime_inputs")
    if any(base.get(key) != head.get(key) for key in identity):
        raise ValueError("receipts differ in host, build toolchain or workload contract")
    quiet = all(run["load"]["ok"] and run["load"].get("timing_locked", True) for run in (base, head))
    if args.require_quiet and not quiet:
        raise ValueError("at least one run exceeded the load limit or omitted the timing lock")
    if not quiet:
        print("UNQUALIFIED: at least one run exceeded the load limit or omitted the timing lock")
    if legacy:
        print("Legacy receipt: process totals")
    thresholds = read_limits(args.thresholds)
    known = set()
    for run in (base, head):
        for name, cell in run["cells"].items():
            known.add(name)
            known.update(f"{name}/{op}" for op in cell.get("operations", {}))
    if thresholds.keys() - known:
        raise ValueError(f"thresholds name unknown cells or operations: {sorted(thresholds.keys() - known)}")
    print(f"base {base['name']} {base['commit'][:10]}   head {head['name']} {head['commit'][:10]}")
    names = sorted(set(base["cells"]) | set(head["cells"]))
    wanted = set(args.cells.split(",")) if args.cells else set()
    found = set()
    compared = 0
    failed = False
    for name in names:
        a, b = base["cells"].get(name), head["cells"].get(name)
        operations = set(a.get("operations", {}) if a else ()) | set(b.get("operations", {}) if b else ())
        specific = {op for op in operations if f"{name}/{op}" in wanted}
        full_cell = not wanted or name in wanted
        if not full_cell and not specific:
            continue
        found.update({name} if name in wanted else set())
        found.update(f"{name}/{op}" for op in specific)
        if a is None or b is None:
            print(f"{name} {'ADDED' if a is None else 'MISSING'}")
            failed |= args.require_same_cells or (b is None and not args.allow_missing)
            continue
        limits = {**DEFAULT_LIMITS, **thresholds.get(name, {})}
        same_binary = False
        if not legacy:
            keys = ("binary", "argv", "env", "parser", "counter_scope", "check")
            if any(a[key] != b[key] for key in keys):
                raise ValueError(f"workload parameters changed for {name}")
            bin_name = a["binary"]
            base_bin, head_bin = (run["binaries"][bin_name] for run in (base, head))
            if build_args(base_bin["cargo_args"]) != build_args(head_bin["cargo_args"]):
                raise ValueError(f"binary build options changed for {name}")
            base_digest = base_bin.get("sha256")
            same_binary = bool(base_digest and base_digest == head_bin.get("sha256"))
        if not operations:
            if not full_cell:
                continue
            bp, hp = (a, b) if legacy else (a["process"], b["process"])
            failed |= display(f"{name}/process instructions", metric(bp["instructions"]),
                              metric(hp["instructions"]), limits["instructions"], same_binary=same_binary)
            compared += 1
        if full_cell:
            bp, hp = (a, b) if legacy else (a["process"], b["process"])
            bw, hw = metric(bp["wall_ms"]), metric(hp["wall_ms"])
            check_wall = quiet and max(spread(bw), spread(hw)) <= limits["wall"] - 1
            failed |= display(f"{name}/process wall ms", bw, hw, limits["wall"], check_wall)
            failed |= display(f"{name}/process RSS bytes", metric(bp["rss_bytes"]), metric(hp["rss_bytes"]),
                              limits["rss"], quiet)
        for op in sorted(operations if full_cell else specific):
            key = f"{name}/{op}"
            old, new = a["operations"].get(op), b["operations"].get(op)
            if old is None or new is None:
                print(f"{key} {'ADDED' if old is None else 'MISSING'}")
                failed |= args.require_same_cells or (new is None and not args.allow_missing)
                continue
            if set(old) != set(new):
                raise ValueError(f"counter columns changed for {key}")
            cap = {**limits, **thresholds.get(key, {})}
            gated = [counter for counter in PRIMARY if counter in old]
            if not gated:
                raise ValueError(f"no instruction counter for {key}")
            for counter in gated:
                failed |= display(f"{key} [{counter}]", metric(old[counter], base["reps"]),
                                  metric(new[counter], head["reps"]), cap["instructions"],
                                  same_binary=same_binary and counter != "dispatch_allocs")
                compared += 1
    if wanted - found:
        raise ValueError(f"unknown cells or operations: {', '.join(sorted(wanted - found))}")
    if not compared:
        raise ValueError("no comparable measurements")
    print(f"compare: {'FAIL' if failed else 'PASS'} ({compared} primary counters compared)")
    return int(failed)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, TypeError, KeyError) as exc:
        sys.exit(f"compare: FAIL ({exc})")
