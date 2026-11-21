# Lane: style-motion-media

State: slice 1 `MEASURED` (2026-11-21): layers (backdrop blur and filters), springs with settle, output-paced
time, NV12 video and shader paints on `bough-paint`'s per-archetype scene, CPU tile kernel and Metal.

* Design: [`DESIGN.md`](DESIGN.md) (how a layer is damaged and cached; motion; media; what was taken from zgui/zgx).
* Card and report: [`cards/`](cards/).
* Gates: `cargo test --release -p bough-paint --test fx` (and the five `mutant_*` features of `bough-paint`'s
  `Cargo.toml` listed under "effects gates", each of which must fail it).
* Numbers: `bench/fx/run.sh` (raw output in `bench/results/fx/`).

Backlog: `docs/AVENUES.md`; question and first task: `docs/LANES.md`. Local state goes in the ignored
`runtime/lanes/style-motion-media/` (`STATE.md`).
