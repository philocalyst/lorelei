# Effects seam replay and independent review

Pin integration at `7991e7bd4e`, which includes `fbf6a2637b` and the Round 2 measurement merge. The effects contract is frozen in the shared `EFFECTS-BENCH-SPEC.md`. Count samples and actual submission at their native timing boundaries. Keep the opaque rectangle typed diagnostic and reduced paint-seam cells distinguishable from the full effects contract. Time only after the relevant gate passes.

## Predictions and gates

- Unmodified P1–P8 should reproduce work counts. Treat timing as host-specific and compare instruction scope, profile, cache mode and scripts. P6's four-video case is expected to remain void until the driver selects an existing source.
- Expect bins to explain most of the sparse one-spring instruction slope. Expect prefix shading to explain most of the dense total. Instrument disjoint phases whose totals sum. The published dense one-spring trace also changes shader uniforms. Isolate that work in a separate control. Failure of either attribution prediction leaves it open until counter evidence explains the gap.
- Exact retained/full equality across moves, halo contacts, partial windows, wrap and re-entry. Quantify the tent filter separately against a discrete, normalized Gaussian on opaque step patterns. Before measuring, use a 16/255 max error and 5/255 RMS investigation line in the ±3σ edge region. These are review thresholds, and the exact CPU/Metal gate still compares bytes against the kernel's defined filter.
- Run spring retarget and lease storm scripts outside the original suite. A fresh render alone can share an implementation defect, so include independent state and pixel constraints. Exercise `Gpu::frame` separately from `Gpu::staged`.
- No changes to the legacy façade raster. Any paint fix travels as a patch with its focused reproducer. D1's shared prefix goes to the paint lane as a concrete design.

The staged video replay has a second input mode with 120 predecoded NV12 frames. The benchmark generates those source frames before the timed replay and copies each published frame through the normal lease and scene update. Its timing still includes the copy, publication, layer repair and output shading. The same pixel oracle and mutants must pass. Expect identical visible frame sequence and output tile counts for both modes. Refuse timing if the gate fails. The array size and copy are part of the predecoded measurement's scope.


The native-PNG zgui draw-count replay adds a counter at the offscreen renderer's `FrameOutcome::Presented` return. Keep the same inputs and retained/full gates. The counter must stay at zero during idle and increase on changing traces. Record requests and successful draws separately from the uninstrumented wall values. A failed gate still refuses timing. Use 480 ticks for motion so at least 200 active draw ticks can be observed. Startup uses 240 fresh child processes. The count scope ends at the successful renderer outcome.


The dense shader control keeps identical records, paint order, and random colors at 600 cards. Run both arms on the same instrumentation build. Both animate the same spring. One advances the shader word with the spring clock. The other holds the shader word at its initial value. Expect equal mounted record and layer counts, fewer visited records and reached prefix jobs with the frozen shader word, and exact final redraw in both arms. The separate `NOSHADER=1` run removes the shader records and changes the later random color stream. Use that run as a sensitivity check.
