#!/bin/sh
# Integration Cards 3 to 5: the negative controls. Each mutant must make its gates FAIL: the lowered pixel
# gates (Card 3), then the typed path's (Cards 4 and 5).
# Run from the repository root: sh bench/card3/mutants.sh   (set CARGO_TARGET_DIR; uses `nice -n 15 -j 8`)
# Exit status 0 only if every mutant failed its gate. Logs go to a directory of this run's own (mktemp -d).
export RUSTC_WRAPPER=
ok=0
LOGS=$(mktemp -d "${TMPDIR:-/tmp}/card3-mutants.XXXXXX") || exit 2
echo "logs: $LOGS"
# the typed path: which tests each mutant is expected to fail (any one failing test kills it)
for m in bough-dataflow/mutant_delta bough-dataflow/mutant_journal bough-dataflow/mutant_keyed mutant_rigid mutant_typed_tape mutant_typed_log mutant_typed_skip; do
  log="$LOGS/typed-$(echo "$m" | tr / _).log"
  if nice -n 15 cargo test --release -p bough --features "$m" --no-fail-fast --test typed_slice --test typed_words --test typed_list --test typed_review --test typed_twin -j 8 >"$log" 2>&1; then
    echo "MUTANT SURVIVED: $m (the typed gates passed with it)"; ok=1
  else
    n=$(grep -c "^test .* \.\.\. FAILED" "$log")
    if [ "$n" -gt 0 ]; then echo "killed: $m ($n typed tests failed)"; else echo "BUILD OR HARNESS ERROR under $m (see $log)"; ok=1; fi
  fi
done
for m in mutant_free_enter mutant_free_leave mutant_lower_alpha mutant_lower_hover; do
  if nice -n 15 cargo test -p bough --features "$m" --test lowered_slice --test lowered_counter -j 8 >"$LOGS/lowered-$m.log" 2>&1; then
    echo "MUTANT SURVIVED: $m (the gates passed with it)"; ok=1
  else
    n=$(grep -c "^test .* \.\.\. FAILED" "$LOGS/lowered-$m.log")
    if [ "$n" -gt 0 ]; then echo "killed: $m ($n tests failed)"; else echo "BUILD OR HARNESS ERROR under $m (see $LOGS/lowered-$m.log)"; ok=1; fi
  fi
done
exit $ok
