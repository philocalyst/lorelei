# Card 18. Author functions as paints with declared reads

```text
lane:            style-motion-media
state:           CARD
owner:           fxsol (GPT-6.2 Sol)
predecessor:     852f1467de
question:        Can one typed wgsl-rs colour function run in the CPU paint and generated Metal code?
change:          An Author paint carries the window, radius, one state operand and two
                 colour operands. A program descriptor names the matching Rust function
                 and the WGSL module. Validate a module that has no globals or entry
                 points and uses integer/boolean scalar or vector types. Compile it once
                 per Metal executor construction. Regular damage follows the expressions.
platforms:       CPU tile kernel and Metal, headless macOS
workloads:       Exact fixture with custom functions of position and a declared state word.
gates:           CPU/Metal/whole-frame exactness, clipped output, idle under unrelated
                 state and invalid module refusal. Mutant discards the clock operand.
thresholds:      One shader state word visit reaches only its declared readers. No work
                 or allocations to compile shader code on a frame.
```
