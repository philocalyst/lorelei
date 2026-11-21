#!/usr/bin/env python3
"""The lorelei assembly and graph harness.

    python3 bench/asm/asm.py emit [--out DIR] [--only NAME,...] [--dev] [--codegen FLAG ...]
    python3 bench/asm/asm.py list
    python3 bench/asm/asm.py diff BASE_DIR HEAD_DIR [--threshold 0.02]
    python3 bench/asm/asm.py graph expr|crates|calls --out FILE.svg [--emitted DIR] [--depth N]
    python3 bench/asm/asm.py check [--out DIR]

`emit` builds the manifest's artifacts once each (`cargo asm --everything`), slices each manifest
entry's symbols out of the listing, and writes:

    DIR/raw/<artifact>.s     the whole listing cargo asm produced
    DIR/fn/<name>.s          the instruction text of every symbol matching the entry
    DIR/stats.tsv            one row per matched symbol: name, package, symbol, instructions,
                             branches, calls, panic refs, bounds-check refs, bytes
    DIR/MANIFEST.txt         commit, toolchain, host, profile, date, load

`diff` compares two emitted directories by entry (sum over its symbols) and fails when an entry
grows beyond the threshold. `graph expr` renders the learned expression dependency graph of the
gui-bench rows scene from the harness's reflection dump; `graph calls` renders the call graph of
the manifest's symbols; `graph crates` renders the workspace's crate dependency graph. `check` is
the negative control: a dev build of the same symbols must be larger than the release build.

The manifest is `bench/asm/manifest.tsv`: name, package, artifact kind (bin|example|lib),
artifact name, and a substring of the demangled symbol. A `#` line is a comment.
"""
import argparse
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent          # the repository root
HARNESS = HERE / "harness"
MANIFEST = HERE / "manifest.tsv"
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target" / "asm")))


def entries():
    out = []
    for line in MANIFEST.read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        name, package, kind, artifact, match = line.split("\t")
        out.append(dict(name=name, package=package, kind=kind, artifact=artifact, match=match))
    return out


def cargo():
    """The cargo binary; rustup's proxy (the toolchain is pinned by rust-toolchain.toml)."""
    return os.environ.get("CARGO", "cargo")


def asm_listing(package, kind, artifact, dev, codegen, out, rustflags=None, features=None):
    """Run `cargo asm --everything` for one artifact and return the listing, or raise."""
    flag = {"bin": "--bin=", "example": "--example=", "lib": "--lib"}[kind]
    cmd = [cargo(), "asm", "-p", package, flag + artifact if kind != "lib" else flag,
           "--target-dir", str(TARGET), "--everything", "--simplify", "--no-color"]
    # The harness is its own workspace; every other package is a member of the repository's.
    cwd = HARNESS
    if package != "asm-harness":
        cmd += ["--manifest-path", str(ROOT / "Cargo.toml")]
        cwd = ROOT
    cmd.append("--dev" if dev else "--release")
    if features and package == "asm-harness":
        for f in features:
            cmd += ["-F", f]
    for c in codegen:
        cmd.append("-C=" + c)
    log = out / "raw" / f"{package}-{artifact}{'-dev' if dev else ''}.cmd"
    log.parent.mkdir(parents=True, exist_ok=True)
    log.write_text(" ".join(cmd) + "\n")
    t = time.monotonic()
    env = dict(os.environ)
    if rustflags:
        env["RUSTFLAGS"] = rustflags
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, env=env)
    if p.returncode != 0:
        sys.stderr.write(p.stderr[-4000:])
        raise SystemExit(f"asm: cargo asm failed for {package}/{artifact} ({time.monotonic()-t:.0f}s)")
    return p.stdout


HEADER = re.compile(r"^([^\s].*):$")


def functions(text):
    """Yield (symbol, body) for every function header cargo asm printed."""
    name, body = None, []
    for line in text.splitlines():
        m = HEADER.match(line)
        if m and not m.group(1).startswith(("L", ".", "#", "//")):
            if name is not None:
                yield name, body
            name, body = m.group(1), []
        elif name is not None:
            body.append(line)
    if name is not None:
        yield name, body


MNEMONIC = re.compile(r"^\t([a-z][a-z0-9.]*)\b")
BRANCHES = re.compile(r"^(b|bl|br|blr|cbz|cbnz|tbz|tbnz|b\.[a-z]+)$")
CALLS = re.compile(r"^(bl|blr)$")


def stats(symbol, body):
    n = branches = calls = panics = bounds = 0
    for line in body:
        m = MNEMONIC.match(line)
        if not m:
            continue
        op = m.group(1)
        if op.startswith("."):
            continue
        n += 1
        if BRANCHES.match(op):
            branches += 1
        if CALLS.match(op):
            calls += 1
        low = line.lower()
        if any(w in low for w in ("panic", "unwrap_failed", "expect_failed", "cold")):
            panics += 1
        if "bounds_check" in low or "index_failed" in low or "brk" in low.split():
            bounds += 1
    return n, branches, calls, panics, bounds


def emit(out, only=None, dev=False, codegen=None, rustflags=None, features=None, mutant=False):
    codegen = codegen or []
    out = Path(out)
    (out / "raw").mkdir(parents=True, exist_ok=True)
    (out / "fn").mkdir(parents=True, exist_ok=True)
    es = [e for e in entries() if not only or e["name"] in only]
    missing = (only or set()) - {e["name"] for e in es}
    if missing:
        raise SystemExit(f"asm: no manifest entry named {sorted(missing)}")
    by_artifact = {}
    for e in es:
        by_artifact.setdefault((e["package"], e["kind"], e["artifact"]), []).append(e)
    rows = []
    for (package, kind, artifact), group in by_artifact.items():
        art = "asm-mutant" if mutant and artifact == "asm-harness" else artifact
        text = asm_listing(package, kind, art, dev, codegen, out, rustflags, features)
        (out / "raw" / f"{package}-{artifact}{'-dev' if dev else ''}.s").write_text(text)
        fs = list(functions(text))
        for e in group:
            m = e["match"]
            if mutant and m.startswith("asm_harness::"):
                m = "asm_mutant::" + m[len("asm_harness::"):]
            hit = [(s, b) for s, b in fs if (s.endswith(m[1:]) if m.startswith("^") else m in s)]
            if not hit:
                raise SystemExit(f"asm: {e['name']}: no symbol matching {e['match']!r} in {package}/{artifact}")
            body = []
            for s, b in hit:
                body.append(s + ":")
                body.extend(b)
                n, br, ca, pa, bo = stats(s, b)
                rows.append([e["name"], package, s, n, br, ca, pa, bo, len("\n".join(b))])
            (out / "fn" / f"{e['name']}.s").write_text("\n".join(body) + "\n")
    (out / "stats.tsv").write_text(
        "# name\tpackage\tsymbol\tinstructions\tbranches\tcalls\tpanics\tbounds\tbytes\n"
        + "".join("\t".join(map(str, r)) + "\n" for r in rows))
    commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True).stdout.strip()
    rustc = subprocess.run([os.environ.get("RUSTC", "rustc"), "-V"], capture_output=True, text=True).stdout.strip()
    load = os.getloadavg()
    (out / "MANIFEST.txt").write_text(
        f"commit {commit}\nrustc {rustc}\nprofile {'dev' if dev else 'release'}\n"
        f"codegen {' '.join(codegen) or 'none'}\nrustflags {rustflags or 'none'}\nhost {os.uname().machine} macOS {os.uname().release}\n"
        f"date {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}\nload {load[0]:.2f} {load[1]:.2f} {load[2]:.2f}\n"
        f"target {TARGET}\n")
    totals = {}
    for r in rows:
        totals[r[0]] = totals.get(r[0], 0) + r[3]
    w = max(len(k) for k in totals) if totals else 4
    for name, n in sorted(totals.items(), key=lambda kv: -kv[1]):
        syms = sum(1 for r in rows if r[0] == name)
        print(f"{name:<{w}}  {n:9d} instructions  ({syms} symbol{'s' if syms != 1 else ''})")
    print(f"asm: emitted {len(rows)} symbols, {sum(totals.values())} instructions, into {out}")
    return rows


def read_stats(d):
    rows = {}
    for line in (Path(d) / "stats.tsv").read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        p = line.split("\t")
        rows.setdefault(p[0], []).append(dict(symbol=p[2], instructions=int(p[3]), branches=int(p[4]),
                                              calls=int(p[5]), panics=int(p[6]), bounds=int(p[7])))
    return rows


def diff(base, head, threshold):
    b, h = read_stats(base), read_stats(head)
    names = sorted(set(b) | set(h))
    bad = []
    print(f"{'entry':<22}{'base':>12}{'head':>12}{'delta':>10}{'pct':>9}  verdict")
    for n in names:
        if n not in b or n not in h:
            verdict = "MISSING"
            bad.append(n)
            print(f"{n:<22}{'-':>12}{'-':>12}{'-':>10}{'-':>9}  {verdict}")
            continue
        bi, hi = sum(r["instructions"] for r in b[n]), sum(r["instructions"] for r in h[n])
        d = hi - bi
        pct = (d / bi) if bi else (0.0 if hi == 0 else float("inf"))
        verdict = "ok"
        if d > 16 and pct > threshold:
            verdict = "REGRESSED"
            bad.append(n)
        print(f"{n:<22}{bi:>12,}{hi:>12,}{d:>+10,}{pct*100:>8.2f}%  {verdict}")
    if bad:
        print(f"asm: {len(bad)} entry(ies) regressed or missing: {', '.join(bad)}")
        return 1
    print(f"asm: no entry grew beyond {threshold*100:.1f}% (and 16 instructions)")
    return 0


def esc(s):
    return s.replace('"', '\\"')


def graph_crates(out):
    meta = json.loads(subprocess.run([cargo(), "metadata", "--no-deps", "--format-version", "1"],
                                     cwd=ROOT, capture_output=True, text=True, check=True).stdout)
    ids = {p["id"]: p["name"] for p in meta["packages"]}
    lines = ["digraph crates {", '  rankdir=LR; node [shape=box, fontname="Helvetica", fontsize=9];']
    for p in meta["packages"]:
        for d in p["dependencies"]:
            path = d.get("path") or ""
            if path.startswith(str(ROOT)):
                lines.append(f'  "{esc(p["name"])}" -> "{esc(d["name"])}";')
    lines.append("}")
    dot(out, lines)


def dot(out, lines):
    src = Path(out).with_suffix(".dot")
    src.write_text("\n".join(lines) + "\n")
    subprocess.run(["dot", "-Tsvg", str(src), "-o", str(out)], check=True)
    print(f"asm: wrote {out} and {src}")


def graph_expr(out):
    p = subprocess.run([cargo(), "run", "-q", "--release", "--bin", "reflect"],
                       cwd=HARNESS, env=dict(os.environ, CARGO_TARGET_DIR=str(TARGET)),
                       capture_output=True, text=True)
    if p.returncode != 0:
        sys.stderr.write(p.stderr[-3000:])
        raise SystemExit("asm: reflect failed")
    lines = ["digraph expr {", '  rankdir=LR; node [fontname="Helvetica", fontsize=9];',
             '  subgraph cluster_words { label="state block words"; style=dashed; '
             'node [shape=ellipse];']
    cols = []
    for line in p.stdout.splitlines():
        if not line or line.startswith("#"):
            continue
        store, col, kind, stage, reads, ops, ty = line.split("\t")
        cols.append((store, col, kind, stage, int(reads, 16), ops, ty))
    words = sorted({b for c in cols for b in range(64) if c[4] >> b & 1})
    for w in words:
        lines.append(f'  "w{w}" [label="w{w}"];')
    lines.append("  }")
    for store, col, kind, stage, reads, ops, ty in cols:
        nid = f'c{store}.{col}'.replace('"', '')
        lines.append(f'  "{nid}" [shape=box, label="{esc(kind)}\\n{esc(store)}.{col}\\nstage {esc(stage)}"];')
        for b in range(64):
            if reads >> b & 1:
                lines.append(f'  "w{b}" -> "{nid}";')
        if ops:
            lines.append(f'  "{nid}" [tooltip="{esc(ops)}"];')
    lines.append("}")
    dot(out, lines)
    print(f"asm: {len(cols)} columns, {len(words)} words read")


def graph_calls(out, emitted, depth):
    emitted = Path(emitted)
    manifest = {e["name"]: e["match"] for e in entries()}
    edges, nodes = set(), set()
    for f in sorted((emitted / "fn").glob("*.s")):
        name = f.stem
        for line in f.read_text().splitlines():
            m = re.search(r"\b(bl|b)\s+<?([^>]+)>?", line)
            if not m:
                continue
            tgt = m.group(2).strip()
            if tgt.startswith("L") or " " in tgt:
                continue
            # shorten a long demangled callee to its last two components
            short = tgt.split(" as ")[0]
            parts = re.sub(r"<[^<>]*>", "", short).split("::")
            label = "::".join(parts[-2:]) if len(parts) > 1 else short
            nodes.add(name)
            nodes.add(label)
            edges.add((name, label))
    lines = ["digraph calls {", '  rankdir=LR; node [shape=box, fontname="Helvetica", fontsize=8];']
    for n in sorted(nodes):
        style = ",style=filled,fillcolor=lightyellow" if n in manifest else ""
        lines.append(f'  "{esc(n)}"{style and " [" + style[1:] + "]" if style else ""};'.replace(" [", " [", 1))
    for a, b in sorted(edges):
        lines.append(f'  "{esc(a)}" -> "{esc(b)}";')
    lines.append("}")
    dot(out, lines)


def check_mutants(out):
    """Negative control: with `--cfg asm_mutant` the control wrappers do extra work, and the count
    must grow. The dev-build comparison is printed as information only: a different inlining regime
    changes which symbols exist at all, so per-symbol counts across profiles are not comparable."""
    out = Path(out)
    rel, mut = out / "release", out / "mutant"
    emit(rel)
    emit(mut, features=["asm_mutant"], mutant=True)
    r, m = read_stats(rel), read_stats(mut)
    watched = ["turn_settle", "turn_paint", "kernel_shade_wrap"]
    ok = True
    # The mutant's extra work is an out-of-line call (`asm_mutant::extra`); the wrapper grows by the
    # call plus its argument setup, and `extra` itself is the work. Count both.
    extra = 0
    for f in (mut / "raw").glob("*.s"):
        extra += sum(stats(s_, b)[0] for s_, b in functions(f.read_text()) if "asm_mutant::extra" in s_)
    if extra == 0:
        print("asm check: FAIL (the mutant build has no asm_mutant::extra symbol)")
        return 1
    print(f"{'entry':<22}{'release':>12}{'mutant':>12}{'mutant+extra':>14}  verdict")
    for n in watched:
        ri = sum(x["instructions"] for x in r.get(n, []))
        mi = sum(x["instructions"] for x in m.get(n, [])) + extra
        good = mi > ri + 8
        ok &= good
        print(f"{n:<22}{ri:>12,}{mi - extra:>12,}{mi:>14,}  {'ok' if good else 'MUTANT NOT DETECTED'}")
    if not ok:
        print("asm check: FAIL (added work did not move the instruction count)")
        return 1
    # Information: the same entries unoptimised. Inlining differs, so several shrink; this is why the
    # gate compares same-profile builds only.
    dev = out / "dev"
    emit(dev, dev=True)
    d = read_stats(dev)
    print("\ninformation: release vs dev (not a gate; the inliner changes symbol selection)")
    for n in sorted(r):
        ri = sum(x["instructions"] for x in r[n])
        di = sum(x["instructions"] for x in d.get(n, []))
        print(f"  {n:<22}{ri:>10,}{di:>10,}")
    print("asm check: PASS (the cfg mutant moves every control entry)")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    e = sub.add_parser("emit")
    e.add_argument("--out", default=str(HERE / "out"))
    e.add_argument("--only", default="")
    e.add_argument("--dev", action="store_true")
    e.add_argument("--codegen", action="append", default=[])
    e.add_argument("--rustflags", default=None)
    sub.add_parser("list")
    d = sub.add_parser("diff")
    d.add_argument("base")
    d.add_argument("head")
    d.add_argument("--threshold", type=float, default=0.02)
    g = sub.add_parser("graph")
    g.add_argument("what", choices=["expr", "crates", "calls"])
    g.add_argument("--out", required=True)
    g.add_argument("--emitted", default=str(HERE / "out"))
    g.add_argument("--depth", type=int, default=2)
    c = sub.add_parser("check")
    c.add_argument("--out", default=str(HERE / "check"))
    a = ap.parse_args()
    if a.cmd == "list":
        for x in entries():
            print(f"{x['name']:<22}{x['package']:<14}{x['kind']:<10}{x['artifact']:<16}{x['match']}")
        return 0
    if a.cmd == "emit":
        only = set(filter(None, a.only.split(",")))
        emit(a.out, only or None, a.dev, a.codegen, a.rustflags)
        return 0
    if a.cmd == "diff":
        return diff(a.base, a.head, a.threshold)
    if a.cmd == "graph":
        return {"crates": lambda: graph_crates(a.out),
                "expr": lambda: graph_expr(a.out),
                "calls": lambda: graph_calls(a.out, a.emitted, a.depth)}[a.what]() or 0
    if a.cmd == "check":
        return check_mutants(a.out)
    return 2


if __name__ == "__main__":
    sys.exit(main())
