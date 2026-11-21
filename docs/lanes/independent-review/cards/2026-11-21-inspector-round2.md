lane: independent-review
state: CARD
predecessors: submission fbbd5918a4, integration 884e8ca38b, nightly-2026-10-17
question: Which four follow-up gates replay, and which parts of the declared contracts are still missing?
change: Replay the submitted gates and mutants, verify the patch boundary and semantic ports, and compose with Wave 3.
platform: macOS aarch64, headless
thresholds: only new fields in the golden, deterministic identities, identical opt-out __text against Wave 3,
            actual in-process code replacement with unchanged live state, and no extra workspace failures.
controls: site, overlay, slow transport and re-description mutants must fail at their respective assertions.
measurement: matched dist builds, executable section hashes, and process/world/field evidence from the app.
