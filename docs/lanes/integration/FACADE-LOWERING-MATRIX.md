# Façade lowering blueprint for Card 3

This is a construction plan, not a claim that the façade (A) already runs on the new core (B). The author contract is `crates/bough/src/lib.rs::prelude`. Existing raw scene ports remain a separate compatibility surface. No per-view runtime switch is required or permitted by this plan.

| Prelude export | Lowering destination | Required behavior and edge cases |
|---|---|---|
| `View` | `World<V, B>` and typed `Sink<V>` | Preserve author method signatures; adapt internally. Root state stays exclusively borrowed. Seal callback owner type at admission; no erased callback may run against another view. |
| `Changed` | typed state comparison and dependency incidence | Capture a turn's before/after state once. Multiple fields, no-op writes, nested handlers, and structural changes must agree with re-description. |
| `rgb`, `Color` | staged paint operands and theme table | Preserve channel order, premultiplication, opacity and transparent colors. Theme changes invalidate the records that use them. |
| `Element`, `IntoProp` | `el::Element::drive<S: Sink<V>>`, typed expression properties | Values, signals and mapped properties remain accepted. Preserve evaluation stage; closure captures must never become raw word slices. |
| `each` | child store under the parent element's call site | Keep lexical ancestry and painter order. Empty/nested collections and changed child types require checked admission. |
| `Entities`, `Entity` | typed child view store and logical identity | Owner, generation, removal and replacement must be checked before dispatch. Preserve entity-local state and invalidation. |
| `List`, `Slot` | relation store plus keyed child store | Keep insertion/removal/edit behavior and stable logical identity independent of physical row. Preserve slot borrow rules and offscreen edits. |
| `list` | `el` list emission plus `bough-lay` extents | Materialize exposure only; nested scrollers, resize, variable heights, fractional scale, row replacement and ring reuse need semantic gates. Focus/press/AX follow logical identity. |
| `boundary`, `Reader`, `Request` | owned async state and typed completion turns | Preserve pending/ready/error, cancellation, stale completion refusal and owner lifetime. A completion must enter a normal turn; never mutate a published frame. |
| `text_field` | text input element and `bough-face` editing/shaping bridge | Preserve selection, caret, clipboard, UTF-8 boundaries, IME and undo. Prepared-face refusal must select a real full shaper, not omit text. |
| `tooltip` | lexical overlay element and local timing state | Pointer ancestry, clipping, delay, cancellation, z order and focus must remain coherent. |
| `Interact`, `Key`, `Mods` | typed event descriptors and lexical propagation | Preserve handler methods, button/key/modifier mapping, capture/bubble/decline, focus and key repeat. Handler identity is not a pointer-sized cast. |
| `block`, `column`, `row`, `para` | `bough-lay::shape` and flex solver; `bough-face` paragraph measure | Keep child order, intrinsic sizes, wrap, gaps, alignment, min/max sizes, percentages and text baselines. Refused fixed-shape limits need a general layout fallback. |
| `Align`, `Justify`, `Styled` | layout words and typed modifier chain | Retain modifier precedence, per-side spacing, absolute placement, clipping, style inheritance and scale. Exact memo equality includes shape words, scale and expression inputs. |
| `sig!`, `Signal` | typed field projection and staged expressions | Author syntax stays unchanged. Track actual dependency fields; support padded captured values without reading padding. Avoid a process-global revision as semantic authority. |
| `Curve`, `Ease` | time expressions and kernel command transitions | Output-paced animation, interrupted transitions, wrap-safe clocks, exact endpoints, moving pick geometry and damage sweeps. Derived blur/gradient/transform fields must reach the target. |
| `button`, `div` | primitive box/text composition and interaction modifiers | Preserve existing functions; no new component library. All style and behavior lower through the same element path. |
| `image` | leased image resource and staged image command | Decoding, alpha, crop/scale/rotation, resource lifetime and updates must remain correct. SVG needs cached rasterization. |
| `text`, `text_input` | face runs plus full-shaper fallback and editor state | Empty/stale/foreign runs draw nothing; fallback handles non-Latin text, bidi and emoji. Text input must preserve existing public editing semantics. |
| `Application`, `Window`, `Flex` | host turn loop, typed world, layout adapter | Resize, DPI, headless rendering, close/drop, frame submission and resource lifetime remain application behavior. Preserve startup and platform options. |

The implementation inventory spans `app.rs` (turn/host), `ui.rs` (public builders), `elem.rs` (element protocol), `layout.rs` (flow/modifiers), `signal.rs` (properties), `list.rs` and `listview.rs` (collections), `input.rs` (events/editing), `text.rs` (text preparation), `deco.rs` (effects) and `reader.rs` (async state). Lower below these public signatures. Paints not admitted by the optimized primitive path use the general kernel executor within the same world; layout and shaping need their own complete fallbacks.

| Existing façade test file | Card 3 authority and target |
|---|---|
| `boxops.rs` | modifier composition and box geometry → layout words and paint commands |
| `capacity.rs` | retained capacity and exposure limits → child store/record ledger |
| `counter.rs` | state, handler and redraw vertical slice → typed world turn |
| `cpu_target.rs` | CPU surface correctness → validated kernel program and output slice |
| `describe.rs` | re-description and changed properties → element lowering/dependency updates |
| `entity.rs` | child ownership and lifetime → typed entity child store |
| `far.rs` | remote/offscreen state → logical collections and exposure |
| `gpu_target.rs` | target lifetime/device ownership → branded kernel target admission |
| `lay_diff.rs` | differential layout → `bough-lay` versus existing solver oracle |
| `layout.rs` | flow geometry → solver, measurements and exact shape memo |
| `list.rs` | collection edits → keyed relation operations |
| `list_window.rs` | virtualization and window movement → extents/exposure/ring reuse |
| `owners.rs` | owner authority → world and resource brands |
| `reader.rs` | async completion/cancellation → owned request transitions |
| `reflect.rs` | observation and mutation → world-branded reflection |
| `scroll_box.rs` | scroll/clipping/layout interaction → spaces and hierarchical extents |
| `seals.rs` | frame/resource lifetime → immutable bounded frame leases |
| `select_all.rs` | editable selection → editor and shaped text geometry |
| `startup.rs` | mount/startup lifecycle → application/host and lazy executor |

Run these tests unchanged against the composed façade before claiming compatibility. Add independent pixel and semantic comparisons (`retained == full == immediate == fresh mount`, ring ages 1–4), negative controls, nested lists and a deep real view compile/size line. A matrix entry is a target, not evidence of completion. The new-core benchmark's prepared Latin-1 face, opaque colors, arithmetic rows, resize remount, group-1 keymap and CPU spans do not establish this façade contract.
