#!/bin/sh
# Usage: fx-seam-run.sh OUTDIR [SIZES] [CELLS]
# A row becomes eligible for timing when its pixel check and applicable mutants pass.
set -eu
out=${1:?output directory}
sizes=${2:-"200 1000 5000"}
cells=${3:-"startup idle one many hover theme scroll resize glass"}
bin=${BIN:-$HOME/work/targets/solfx/fx-seam-r3/release/examples/fx_bench}
export BIN="$bin"
mkdir -p "$out"
TMO_CHECK=${TMO_CHECK:-900}; TMO_MEASURE=${TMO_MEASURE:-600}   # per-cell wall limits, seconds
export TMO_CHECK TMO_MEASURE
if [ "${4:-}" != --locked ]; then
    exec lockf -k "$HOME/work/locks/bench.lock" nice -n 15 "$0" "$out" "$sizes" "$cells" --locked
fi

lim() { s=$1; shift; perl -e 'alarm shift; exec @ARGV' "$s" "$@"; }   # exit 142: cell timed out
expect='retained differs|independent constraint|fresh mount differs|independent full frame|idle frames did framework work|idle frame changed|did not change|never changed|too few changing'
mutant() {
    kind=$1
    log=$stem.mutant-$kind
    rc=0
    lim "$TMO_CHECK" "$bin" check "$cell" "$cards" "--mutant=$kind" > "$log" 2>&1 || rc=$?
    if [ "$rc" -eq 101 ] && rg -q "$expect" "$log"; then
        printf 'mutant_%s=killed\n' "$kind"
    else
        printf 'mutant_%s=unexpected_exit:%s\n' "$kind" "$rc"
        return 1
    fi
}

"$bin" manifest > "$out/manifest.tsv"
shasum -a 256 "$bin" | awk '{print $1}' > "$out/binary.sha256"
for cards in $sizes; do
    for cell in $cells; do
        stem=$out/$cell-$cards
        check=0
        mutations=0
        {
            uptime
            lim "$TMO_CHECK" "$bin" check "$cell" "$cards" || check=$?
            printf 'check_exit=%s\n' "$check"
            if [ "$check" -eq 0 ] && [ "$cell" != startup ]; then
                mutant input || mutations=1
                if [ "$cell" != idle ]; then mutant damage || mutations=1; fi
            fi
            printf 'mutation_check_exit=%s\n' "$mutations"
        } > "$stem.check" 2>&1
        if [ "$check" -eq 0 ] && [ "$mutations" -eq 0 ]; then
            measure=0
            {
                lim "$TMO_MEASURE" "$bin" measure "$cell" "$cards" || measure=$?
                printf 'measure_exit=%s\n' "$measure"
                uptime
            } > "$stem.csv" 2>&1
        else
            printf 'measure skipped: gate failed\n' > "$stem.csv"
        fi
    done
    if [ ! -f "$out/startup-$cards.check" ] || ! rg -q '^check_exit=0$' "$out/startup-$cards.check"; then continue; fi
    python3 -I - "$bin" "$cards" > "$out/startup-cold-$cards.tsv" <<'PY'
import subprocess
import sys
import time

binary, cards = sys.argv[1:]
print("sample\tchild_mount_first_frame_ns\tspawn_to_exit_ns")
for i in range(240):
    start = time.perf_counter_ns()
    process = subprocess.run(
        [binary, "measure", "startup", cards, "1", "--bringup", "--nowarm"],
        capture_output=True, text=True, check=True,
    )
    elapsed = time.perf_counter_ns() - start
    rows = [line for line in process.stdout.splitlines() if line.startswith("fx_startup,")]
    if len(rows) != 1:
        raise RuntimeError(f"expected one startup sample, got {len(rows)}")
    print(f"{i}\t{rows[0].split(',')[3]}\t{elapsed}")
PY
done
