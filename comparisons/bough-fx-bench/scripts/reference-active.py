#!/usr/bin/env python3
"""Report zgui elapsed percentiles on observed redraw-request steps."""
import argparse
from pathlib import Path

from matrix import MIN_SAMPLES, nearest_rank


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    opts = parser.parse_args()
    print("| Cell | Observations | Redraw steps | Requests | Active p50/p95/p99 ms |")
    print("|---|---:|---:|---:|---:|")
    for path in sorted(opts.directory.glob("zgui-*.out")):
        lines = path.read_text().splitlines()
        if "measure_exit=0" not in lines:
            print(f"| {path.stem} | — | — | — | measurement failed or pending |")
            continue
        rows = [l.split(",") for l in lines if l.startswith("zgui_sample,")]
        elapsed = []
        requests = 0
        for prev, cur in zip(rows, rows[1:]):
            assert int(cur[3]) == int(prev[3]) + 1
            delta = int(cur[5]) - int(prev[5])
            assert delta >= 0
            if delta:
                requests += delta
                elapsed.append(int(cur[4]))
        if len(elapsed) >= MIN_SAMPLES:
            stats = "/".join(f"{nearest_rank(elapsed, p) / 1e6:.3f}" for p in (50, 95, 99))
        else:
            stats = "short active run"
        print(f"| {path.stem} | {len(rows)} | {len(elapsed)} | {requests} | {stats} |")


if __name__ == "__main__":
    main()
