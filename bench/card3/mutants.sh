#!/bin/sh
# Integration Card 3: the negative controls. Each mutant must make the lowered pixel gates FAIL.
# Run from the repository root: sh bench/card3/mutants.sh   (set CARGO_TARGET_DIR; uses `nice -n 15 -j 8`)
# Exit status 0 only if every mutant failed its gate.
export RUSTC_WRAPPER=
ok=0
for m in mutant_free_enter mutant_free_leave mutant_lower_alpha mutant_lower_hover; do
  if nice -n 15 cargo test -p bough --features "$m" --test lowered_slice --test lowered_counter -j 8 >"/tmp/card3-$m.log" 2>&1; then
    echo "MUTANT SURVIVED: $m (the gates passed with it)"; ok=1
  else
    n=$(grep -c "^test .* FAILED" "/tmp/card3-$m.log")
    if [ "$n" -gt 0 ]; then echo "killed: $m ($n tests failed)"; else echo "BUILD OR HARNESS ERROR under $m (see /tmp/card3-$m.log)"; ok=1; fi
  fi
done
exit $ok
