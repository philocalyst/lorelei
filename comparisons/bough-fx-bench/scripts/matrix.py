#!/usr/bin/env python3
"""Print each frozen matrix row, preserving unsupported and invalid cells."""
from argparse import ArgumentParser
from pathlib import Path

CELLS = ("startup", "idle", "one", "many", "hover", "theme", "scroll", "resize", "video")


def nearest_rank(samples, percentile):
    return sorted(samples)[(len(samples) * percentile + 99) // 100 - 1]


def read_lines(path):
    return path.read_text().splitlines() if path.exists() else []


def ms(ns):
    return f"{ns / 1e6:.3f}"


def facade(out, cards, cell):
    stem = out / f"{cards}-{cell}"
    captures = read_lines(stem.with_suffix(".csv"))
    for line in captures:
        x = line.split(",")
        if x[0] == "summary":
            samples, draws = map(int, x[3:5])
            if cell == "idle":
                return f"{draws} frames"
            if samples < 200:
                return f"short run ({samples} observations)"
            suffix = "/".join(ms(int(v)) for v in x[5:8])
            return f"{suffix} ({draws}/{samples})"
    gate = read_lines(stem.with_suffix(".gate"))
    if captures:
        return "measurement failed"
    if gate and "unavailable" in gate[0]:
        return "unavailable"
    if any("panicked" in line for line in gate):
        return "pixel gate failed"
    return "pending"


def reference(out, framework, cards, cell):
    base = out / f"{framework}-{cards}-{cell}"
    lines = read_lines(base.with_suffix(".out"))
    if "check_exit=2" in lines:
        return "unavailable"
    if "check_exit=0" not in lines:
        return "gate failed" if any(line.startswith("check_exit=") for line in lines) else "pending"
    for line in lines:
        if line.startswith(("full_exit=", "comparison_exit=")) and not line.endswith("=0"):
            return "pixel gate failed"
    if "measure_exit=0" not in lines:
        return "measurement failed" if any(line.startswith("measure_exit=") for line in lines) else "pending"
    records = [line.split(",") for line in lines if line.startswith(f"{framework}_sample,")]
    if not records:
        return "missing samples"
    draws = int(records[-1][5]) - (int(records[0][5]) if framework == "zgui" and cell != "startup" else 0)
    if cell == "idle":
        return f"{draws} frames"
    samples = [int(r[4]) for r in records]
    if len(samples) == 1:
        return f"{ms(samples[0])} (1 observation)"
    if len(samples) < 200:
        return f"short run ({len(samples)} observations)"
    return "/".join(ms(nearest_rank(samples, p)) for p in (50, 95, 99)) + (f" (Δ{draws}/{len(samples)})" if framework == "zgui" else f" ({draws}/{len(samples)})")


def main():
    args = ArgumentParser()
    args.add_argument("baseline", type=Path)
    args.add_argument("wave3", type=Path)
    args.add_argument("references", type=Path)
    paths = args.parse_args()
    print("| Cards | Cell | Lorelei baseline | Lorelei wave3 | zgui | zgx |")
    print("|---:|---|---:|---:|---:|---:|")
    for cards in (200, 1000, 5000):
        for cell in CELLS:
            values = [facade(paths.baseline, cards, cell), facade(paths.wave3, cards, cell)]
            for framework in ("zgui", "zgx"):
                values.append(reference(paths.references, framework, cards, cell))
            print(f"| {cards:,} | {cell} | " + " | ".join(values) + " |")


if __name__ == "__main__":
    main()
