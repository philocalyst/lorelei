# Attribute and recover typed core instruction regressions

- Lane and owner are independent-review and review.
- Predecessors are cc65ee215f, e0b7a4c889, 5cbb9b20ed and 71ee3be0aa. Builds use independent target directories, pinned nightly and the dist profile.
- Question. How much changed across integration and what work can move off the hot path while preserving refusal and pixel gates?
- Workload. Sheet at 10,000 logical rows, event dispatch at 100, 10,000 and 1 million, and the 22-rectangle sheet layout memo. B is the typed core with prepared Latin-1 DejaVu Sans 14/28 px, opaque colors, arithmetic rows, remount on resize, group-1 keymap and CPU spans.
- Prediction. Card 1 trips visible work on hover and edits. Card 2 charges the memo a second shape pass. A fused exact memo pass removes substantially more than 500 instructions. Adversarial shapes, scales, inputs and structural substitutions must still be checked.
- Gates. Existing retained/full/fresh oracles, negative controls covering each admission change, all reported cells and losses. Exact safety oracles take priority over the published baseline targets.
- Measurement. Three locked process runs per build on headless arm64 macOS. Retired calling-thread instructions with benchmark-reported p50 and host load. Keep executable and fixture hashes. Attribution requires source and assembly, with ablations when available.
- Stop conditions. Report unresolved cells, budget failures and missed thresholds. Avoid a change whose validation cannot be demonstrated or whose benchmark win depends on the fixture.
