#!/usr/bin/env python3
"""Summarize paint slice 1 P1–P8 from stock bench/fx/run.sh logs."""
from argparse import ArgumentParser
from pathlib import Path


def effects(path):
    rows = {}
    for line in path.read_text().splitlines():
        fields = line.split("\t")
        if len(fields) == 17 and fields[3] in ("cpu1", "cpu1-nocache", "metal"):
            rows[tuple(fields[:4])] = [float(s) for s in fields[4:]]
        elif line.startswith("## FAILED"):
            print(line)
    return rows


def pbench(path):
    rows = {}
    for line in path.read_text().splitlines():
        fields = line.split("\t")
        if len(fields) >= 9 and fields[1].startswith("A1 "):
            rows[fields[0]] = float(fields[6])
    return rows


def main():
    args = ArgumentParser()
    args.add_argument("fxbench", type=Path)
    args.add_argument("pbench", type=Path)
    args.add_argument("pbench_baseline", type=Path)
    paths = args.parse_args()
    rows = effects(paths.fxbench)
    if not rows:
        raise SystemExit("No fxbench rows")

    def cell(scene, size, name, ex="cpu1"):
        return rows[scene, str(size), name, ex]

    def result(prediction, evidence, passed):
        print(f"| {prediction} | {evidence} | {'held' if passed else 'missed'} |")

    print("| Prediction | Replay | Outcome |")
    print("|---|---|---|")
    idle = [r for key, r in rows.items() if key[2:] == ("idle", "cpu1")]
    result("P1 idle", f"{len(idle)} rows, maximum tile/record/prefix/block counts "
           + "/".join(f"{max(r[i] for r in idle):.0f}" for i in range(4)),
           all(not any(r[:4]) for r in idle))
    a, b = [cell("pane", size, "one spring") for size in (300, 3000)]
    result("P2 one spring", f"{a[1]:.1f}/{b[1]:.1f} visits, {a[7]:,.1f} / {b[7]:,.1f} K instructions ({b[7] / a[7]:.3f}×)",
           a[1] == b[1] == 1 and b[7] <= 1.15 * a[7])
    above = [r for key, r in rows.items() if key[0] != "media" and key[2:] == ("hover above glass", "cpu1")]
    result("P3 hover above", f"{len(above)} rows, maximum prefix/blocks "
           f"{max(r[2] for r in above):.0f}/{max(r[3] for r in above):.0f}",
           all(not any(r[2:4]) for r in above))
    row = cell("pane", 300, "hover beneath glass")
    coverage = (row[2] * 1024 + row[4]) / row[5]
    result("P4 hover beneath", f"{row[2]:.0f} prefix tiles, {row[4]:,.0f} pyramid px, "
           f"{coverage:.2%} of {row[5]:,.0f} level pixels", coverage <= 0.25)
    moves = cell("pane", 300, "glass moves 1 px")
    # An idle frame forced to redraw its entire static pane gives the full pyramid size.
    whole_pyramid = cell("pane", 300, "idle", "cpu1-nocache")[4]
    print(f"| P5 glass moves | {moves[2]:.1f} mean prefix tiles, {moves[4]:,.0f} / "
          f"{whole_pyramid:,.0f} pyramid px ({moves[4] / whole_pyramid:.2%}) | "
          f"{'pyramid held' if moves[4] <= 0.15 * whole_pyramid else 'pyramid missed'}. "
          "Prefix mean below the perimeter. Per-tick maximum unavailable. |")
    a, b, c = [cell("media", size, "one video frame") for size in (4, 24, 48)]
    p6 = f"tiles {a[0]:.0f}/{b[0]:.0f}/{c[0]:.0f}, 48/24 instructions {c[7] / b[7]:.5f}×"
    if not a[0]:
        print(f"| P6 one video | {p6} | Four-video source absent |")
    else:
        result("P6 one video", f"{p6}, 48/4 {c[7] / a[7]:.5f}×",
               a[0] == b[0] == c[0] == 32 and c[7] <= 1.15 * a[7])
    baseline, new = pbench(paths.pbench_baseline), pbench(paths.pbench)
    for k in sorted(set(baseline) | set(new)):
        if k in baseline and k in new:
            ratio = new[k] / baseline[k]
            result(f"P7 {k}", f"{baseline[k]:,.1f} → {new[k]:,.1f} K instructions ({ratio:.3f}×)", ratio <= 1.03)
        else:
            print(f"| P7 {k} | Missing paired row | Unverified |")
    for scene in ("effects", "pane"):
        for name in ("hover above glass", "glass moves 1 px"):
            cached = cell(scene, 300, name)[7]
            full = cell(scene, 300, name, "cpu1-nocache")[7]
            result(f"P8 {scene} {name}", f"{cached / full:.3f}×", cached <= 0.5 * full)
    print(f"\nMismatching words in {len(rows)} rows: {sum(int(r[-1]) for r in rows.values())}.")


if __name__ == "__main__":
    main()
