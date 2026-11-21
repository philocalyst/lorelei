#!/usr/bin/env python3
"""Print each frozen matrix row, preserving unsupported and invalid cells."""
from argparse import ArgumentParser
from pathlib import Path

MIN_SAMPLES = 200

CELLS = ("startup", "idle", "one", "many", "hover", "theme", "scroll", "resize", "video")


def nearest_rank(samples, percentile):
    return sorted(samples)[(len(samples) * percentile + 99) // 100 - 1]


def read_lines(path):
    return path.read_text().splitlines() if path.exists() else []


def ms(ns):
    return f"{ns / 1e6:.3f}"


def summarize(samples, count):
    if not samples:
        return "missing samples"
    if len(samples) < MIN_SAMPLES:
        return f"short run ({len(samples)} observations)"
    timing = "/".join(ms(nearest_rank(samples, p)) for p in (50, 95, 99))
    return f"{timing} ({count}/{len(samples)})"


def facade(out, cards, cell):
    stem = out / f"{cards}-{cell}"
    captures = read_lines(stem.with_suffix(".csv"))
    for line in captures:
        x = line.split(",")
        if x[0] == "summary":
            samples, draws = map(int, x[3:5])
            if cell == "idle":
                return f"{draws} frames"
            if samples < MIN_SAMPLES:
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
    if len(samples) < MIN_SAMPLES:
        return f"short run ({len(samples)} observations)"
    return "/".join(ms(nearest_rank(samples, p)) for p in (50, 95, 99)) + (f" (Δ{draws}/{len(samples)})" if framework == "zgui" else f" ({draws}/{len(samples)})")


def core_cells(path):
    cells = {}
    key = None
    measured = None
    for line in read_lines(path):
        if line.startswith("typed-core "):
            _, cards, cell = line.split()
            key = int(cards), cell
            cells[key] = "pending"
            measured = None
        elif line == "check_exit=0":
            measured = []
        elif line.startswith("check_exit="):
            cells[key] = "unavailable" if line == "check_exit=2" else "pixel gate failed"
        elif measured is not None and line.startswith("core_"):
            measured.append(line.split(","))
        elif measured is not None and line.startswith("measure_exit="):
            if line != "measure_exit=0":
                cells[key] = "measurement failed"
                continue
            cell = key[1]
            if cell == "idle":
                cells[key] = "0 repairs" if any(r[0] == "core_idle" for r in measured) else "missing samples"
            elif cell == "startup":
                samples = [int(r[4]) for r in measured if r[0] == "core_startup"]
                if samples:
                    cells[key] = f"{ms(samples[0])} (1 observation)"
            else:
                steps = [r for r in measured if r[0] == "core_step"]
                cells[key] = summarize([int(r[5]) for r in steps], sum(int(r[-1]) > 0 for r in steps))
    return cells


def startup(out, framework, cards):
    if not out:
        return None
    lines = read_lines(out / f"{framework}-{cards}.out")
    if "check_exit=0" not in lines:
        return "pixel gate failed" if any(l.startswith("check_exit=") for l in lines) else None
    if "measure_exit=0" not in lines:
        return "measurement failed" if any(l.startswith("measure_exit=") for l in lines) else None
    prefix, field, count_field = {
        "facade": ("spawn_to_first", 2, None),
        "core": ("core_startup", 4, None),
        "zgui": ("zgui_sample", 4, 5),
        "zgx": ("zgx_sample", 4, 5),
    }[framework]
    records = [line.split(",") for line in lines if line.startswith(prefix + ",")]
    count = len(records) if count_field is None else sum(int(r[count_field]) for r in records)
    return summarize([int(r[field]) for r in records], count)


def main():
    args = ArgumentParser()
    args.add_argument("baseline", type=Path)
    args.add_argument("wave3", type=Path)
    args.add_argument("references", type=Path)
    args.add_argument("--core", type=Path)
    args.add_argument("--startup", type=Path)
    paths = args.parse_args()
    core = core_cells(paths.core) if paths.core else None
    columns = ["Cards", "Cell", "Lorelei baseline", "Lorelei wave3"]
    if core is not None:
        columns.append("Lorelei typed Soft")
    columns.extend(("zgui", "zgx"))
    print("| " + " | ".join(columns) + " |")
    print("|---:|---|" + "---:|" * (len(columns) - 2))
    for cards in (200, 1000, 5000):
        for cell in CELLS:
            values = [facade(paths.baseline, cards, cell), facade(paths.wave3, cards, cell)]
            if cell == "startup" and (fresh := startup(paths.startup, "facade", cards)):
                values[-1] = fresh
            if core is not None:
                fresh = startup(paths.startup, "core", cards) if cell == "startup" else None
                values.append(fresh or core.get((cards, cell), "pending"))
            for framework in ("zgui", "zgx"):
                fresh = startup(paths.startup, framework, cards) if cell == "startup" else None
                values.append(fresh or reference(paths.references, framework, cards, cell))
            print(f"| {cards:,} | {cell} | " + " | ".join(values) + " |")


if __name__ == "__main__":
    main()
