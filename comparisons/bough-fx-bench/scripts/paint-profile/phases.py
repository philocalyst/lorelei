#!/usr/bin/env python3
"""Print grouped mean retired instructions from an fxprof capture."""
import argparse
from collections import defaultdict
from pathlib import Path

GROUPS = (
    ("Turn and retarget", {23}),
    ("Block compare and window pass", {1, 2, 3, 4, *range(32, 64)}),
    ("Layer geometry and allocation", {5}),
    ("Build bins", {6, 7, 8, 9}),
    ("Layer dependency schedule and damage", {10, 11}),
    ("Prefix setup, fold and store", {12, 13, 14, 16, *range(64, 96)}),
    ("Tent downsample", {15}),
    ("Output setup, fold and store", {17, 18, 19, 20, 21, *range(96, 128)}),
    ("Harness damage list and residual", {0, 22}),
)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("tsv", type=Path)
    parser.add_argument("scene", choices=("pane", "effects"))
    parser.add_argument("shape", choices=("sparse", "dense"))
    parser.add_argument("script", choices=("published", "noshader"))
    parser.add_argument("cell", help="cell name as printed by fxprof")
    opts = parser.parse_args()
    groups = defaultdict(dict)
    totals = {}
    for line in opts.tsv.read_text().splitlines():
        fields = line.split("\t")
        if len(fields) < 8:
            continue
        _, kind, scene, size, shape, script, cell, *tail = fields
        if (scene, shape, script, cell) != (opts.scene, opts.shape, opts.script, opts.cell):
            continue
        n = int(size)
        if kind == "PHASE":
            phase = int(tail[0])
            value = int(tail[2])
            if phase in groups[n]:
                parser.error(f"duplicate phase {phase} at size {n}")
            groups[n][phase] = value
        elif kind == "SUMMARY":
            totals[n] = int(tail[7]), int(tail[3])
    sizes = sorted(totals)
    if not sizes:
        parser.error("no completed summary rows match this selection")
    allowed = set.union(*(ids for _, ids in GROUPS))
    for n in sizes:
        phases = groups[n]
        if not phases or set(phases) - allowed:
            parser.error(f"absent or unexpected phase IDs at size {n}")
        if abs(totals[n][0] - sum(phases.values())) > (len(phases) + 1) // 2:
            parser.error(f"phase sum does not reconcile at size {n}")

    def row(label, get):
        print(f"| {label} | " + " | ".join(f"{get(n):,}" for n in sizes) + " |")

    print("| Calling-thread instructions per tick | " + " | ".join(f"{n:,}" for n in sizes) + " |")
    print("|---|" + "---:|" * len(sizes))
    for label, ids in GROUPS:
        row(label, lambda n: sum(value for phase, value in groups[n].items() if phase in ids))
    row("Output rounding residual", lambda n: totals[n][0] - sum(groups[n].values()))
    row("**Corrected mean**", lambda n: totals[n][0])
    row("Uninstrumented median", lambda n: totals[n][1])


if __name__ == "__main__":
    main()
