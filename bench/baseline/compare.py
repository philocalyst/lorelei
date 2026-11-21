#!/usr/bin/env python3
"""Compare two recorded baselines and fail on a regression.

    python3 bench/baseline/compare.py BASE.json HEAD.json
    python3 bench/baseline/compare.py ... --thresholds limits.json --allow-missing

Per cell, the median of each repetition is compared:

    instructions  <= 1.02 x base   (primary; a count, stable to a fraction of a percent)
    wall (p50)    <= 1.10 x base   (a duration; suppressed when the base itself spreads over 10%)
    rss           <= 1.05 x base

`limits.json` may override any of those numbers per cell, e.g.
{"sheet_x/rows10000": {"instructions": 1.05, "wall": 1.25}}. A cell present in BASE but missing
in HEAD fails unless --allow-missing. Exit status: 0 only when every compared cell is within its
threshold and the HEAD run is marked not loaded (a loaded run warns and fails with --require-quiet).
"""
import argparse
import json
import sys


def load(p):
    return json.loads(open(p).read())


def spread(run):
    if not run:
        return 0.0
    lo, hi = run["min"], run["max"]
    return (hi / lo - 1.0) if lo else 0.0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("base")
    ap.add_argument("head")
    ap.add_argument("--thresholds", default=None)
    ap.add_argument("--allow-missing", action="store_true")
    ap.add_argument("--require-quiet", action="store_true")
    a = ap.parse_args()
    b, h = load(a.base), load(a.head)
    limits = json.loads(open(a.thresholds).read()) if a.thresholds else {}
    failures = []
    print(f"base {b['name']} {b['commit'][:10]}   head {h['name']} {h['commit'][:10]}")
    print(f"{'cell':<28}{'instr base':>13}{'head':>13}{'ratio':>8}  "
          f"{'wall base':>10}{'head':>10}{'ratio':>8}  {'rss ratio':>9}  verdict")
    for cid in sorted(set(b.get("cells", {})) | set(h.get("cells", {}))):
        lim = {"instructions": 1.02, "wall": 1.10, "rss": 1.05}
        lim.update(limits.get(cid, {}))
        bc, hc = b["cells"].get(cid), h["cells"].get(cid)
        if not bc or not hc:
            verdict = "MISSING"
            if not a.allow_missing:
                failures.append(cid)
            print(f"{cid:<28}{'-':>13}{'-':>13}{'-':>8}  {'-':>10}{'-':>10}{'-':>8}  {'-':>9}  {verdict}")
            continue
        bi, hi = bc.get("instructions"), hc.get("instructions")
        bw, hw = bc.get("wall_ms"), hc.get("wall_ms")
        br, hr = bc.get("rss_bytes"), hc.get("rss_bytes")
        ir = (hi["median"] / bi["median"]) if bi and hi and bi["median"] else None
        wr = (hw["median"] / bw["median"]) if bw and hw and bw["median"] else None
        rr = (hr["median"] / br["median"]) if br and hr and br["median"] else None
        bad = []
        if ir is not None and ir > lim["instructions"]:
            bad.append(f"instructions {ir:.3f}x > {lim['instructions']}")
        # A duration whose baseline already spreads more than the limit says nothing about the change.
        if wr is not None and spread(bw) <= (lim["wall"] - 1.0) and wr > lim["wall"]:
            bad.append(f"wall {wr:.3f}x > {lim['wall']}")
        if rr is not None and rr > lim["rss"]:
            bad.append(f"rss {rr:.3f}x > {lim['rss']}")
        verdict = "ok" if not bad else "REGRESSED: " + "; ".join(bad)
        if bad:
            failures.append(cid)
        f = lambda v: f"{v['median']:,.0f}" if v else "-"
        print(f"{cid:<28}{f(bi):>13}{f(hi):>13}{(f'{ir:.3f}' if ir else '-'):>8}  "
              f"{f(bw):>10}{f(hw):>10}{(f'{wr:.3f}' if wr else '-'):>8}  "
              f"{(f'{rr:.3f}' if rr else '-'):>9}  {verdict}")
    hc = h.get("cells", {})
    if a.require_quiet and not h.get("load", {}).get("ok"):
        print("compare: the head run was taken above the load ceiling", file=sys.stderr)
        failures.append("load")
    if failures:
        print(f"compare: FAIL ({len(failures)} cell(s): {', '.join(failures)})", file=sys.stderr)
        return 1
    print("compare: PASS (every cell within its threshold)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
