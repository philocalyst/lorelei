# Headless effects benchmark

The frozen contract is in `docs/lanes/measurement/EFFECTS-BENCH.md`, and sample counts and measurements are in the adjacent card report. The measurement and gate paths run separately. The runner keeps failures with the attempted cell.

From the repository root, set `CARGO_TARGET_DIR` to this lane's target and `BOUGH_FONT` to the reference DejaVuSans.ttf. Run:

```sh
export RUSTC_WRAPPER=
nice -n 15 cargo build --offline --locked --release \
  --features full-effects,typed-core \
  --manifest-path comparisons/bough-fx-bench/Cargo.toml -j 8
comparisons/bough-fx-bench/scripts/run.sh runtime/effects
comparisons/bough-fx-bench/scripts/run-startup.sh > runtime/effects/startup.tsv
```

The façade program uses the author prelude and an offscreen CPU target. It implements startup, idle, a single transition, many simultaneous transitions, hover, theme, scroll and resize. Checks include a required sample-pixel witness, the full-redraw oracle at ages 1–4, a fresh mount, and a dropped-theme-input control. The adapter refuses the complete feature manifest and the video cell. The typed core admits opaque, integer-position rectangles with colour-only transitions and theme changes. It uses the CPU software oracle. Its results are diagnostic.

## Staged paint and framework probes

The staged adapter runs the pinned `bough-paint` seam (`64101ac59d`) as an offscreen renderer. Its glass-over-video trace uses four rounded backdrops, brightness and saturation, a moving spring with a retarget, two built-in paints and a synthetic NV12 publisher. Its card bodies are solid rounded translucent fills. It checks the whole-frame specification on the initial frame and an uncached redraw at ring ages 1–4. It reports video generation together with rendering.

```sh
comparisons/bough-fx-bench/scripts/build-fx-seam.sh
comparisons/bough-fx-bench/scripts/run-additional.sh runtime/effects-extra
```

The zgui and zgx entries exercise the one-moving foreground over layered gradient cards and shadows. Both use each framework's GPU renderer. The source snapshots are pinned in the build script. Give `BOUGH_REF` the reference directory. Cargo offline mode is the default. `CARGO_NET_OFFLINE=false` lets it fetch locked crates when they are absent locally.

```sh
comparisons/bough-fx-bench/scripts/build-references.sh zgui
comparisons/bough-fx-bench/scripts/build-references.sh zgx
lockf -k "$HOME/work/locks/bench.lock" sh -c '
  for n in 200 1000 5000; do
    uptime
    "$CARGO_TARGET_DIR/release/solfx" "$n"
    "$CARGO_TARGET_DIR/dist/examples/solfx" "$n"
    uptime
  done
'
```

The zgx probe checks output change and retained equality against a fresh renderer at the final step. The zgui probe checks pixel hashes in normal and forced-full mode. Use `release/solfx CARD_COUNT check` and replay with `ZGUI_FULL_DAMAGE=1`, then compare the `zgui_pixel` rows. Common GPU completion timing boundaries, more than one animation trace and reference video remain needed for matched ranking. The supplementary adapters expose their coverage in the report.
