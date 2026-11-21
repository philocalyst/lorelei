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

## Round 2 attribution and revision replay

`profile-one.sh` takes a directory containing the façade and typed-core executables. It runs the original 20-tick trace plus a separate mount bracket with CPU1 selected for the façade. Pass `BOUGH_FX_SAMPLES=240` to the façade runner for the long repeat. The default is 240. The runners retain the shared lock for a complete batch to avoid long gaps between cells on a busy host.

```sh
comparisons/bough-fx-bench/scripts/profile-one.sh "$CARGO_TARGET_DIR/release"
comparisons/bough-fx-bench/scripts/build-revision.sh 71ee3be0aa
comparisons/bough-fx-bench/scripts/build-revision.sh 62d53d3abe
export BOUGH_REFERENCE_TILE="$PWD/comparisons/bough-fx-bench/src/tile.png"
comparisons/bough-fx-bench/scripts/run-reference-suite.sh runtime/effects-refs
python3 comparisons/bough-fx-bench/scripts/matrix.py BASELINE_DIR WAVE3_DIR runtime/effects-refs
```

The revision builder copies the same harness to an archive of each production revision and builds into separate subdirectories of `CARGO_TARGET_DIR`. Set it to the revision's artifact subdirectory while running `run.sh` or pass the actual typed-core executable to `run-revision-core.sh`. The reference suite executes 240 active input steps and checks independently rendered full frames. zgui's PNG variant uses the native loader and sets the headless refresh to 120 Hz. The zgx video adapter uses the staged fixture's NV12 input pattern, converted to RGBA before updating its image leaf. Each adapter records submitted or requested frame counts. The rendering paths, cadence, animation semantics and video representations have different measured scopes. See the report's validity column before using any timing comparison.

## Round 3 independent replay

Use the merged `7991e7bd4e` source as the replay pin. The reviewed `bough-paint` workspace comes from a Git archive of that revision. `bench/fx/run.sh` runs the paint lane's stock P1–P8 examples. The replay applies the shared lock to the script's build, and the script itself acquires it around each run. `paint-thresholds.py` reads the stock effects and pbench logs alongside a P7 pbench baseline from `71ee3be0aa`.

The direct Gaussian step and layer-storage fixture lives in `references/fx-gaussian.rs`. Copy it to `crates/bough-paint/tests/solfx_gaussian.rs` inside the pinned archive, then run `cargo test --offline --release -p bough-paint --test solfx_gaussian -- --nocapture`. The reference is an independent discrete Gaussian convolution in byte space. It prints maximum and RMS step-region error and the logical and reserved bytes of four panes.

`run-startup-processes.sh` takes `facade`, `core`, `zgui` or `zgx` plus an output directory. The default is 240 warm-cache invocations per scene size. The façade timer starts at exec. The core and reference timers start inside each new process. `matrix.py` accepts `--core` with the output of `run-revision-core.sh` and `--startup` with those process capture files.

The independent retained-layer and spring/video probes are `references/fx-layers.rs` and `references/fx-motion-media.rs`. Copy them into the pinned archive as `crates/bough-paint/tests/solfx_layers.rs` and `crates/bough-paint/tests/solfx_motion_media.rs`. Run each with `cargo test --offline --release -p bough-paint --test NAME -- --nocapture`. These review probes include defects expected to fail on the baseline. Use their focused test names to verify the candidate patches in a separate archive.
