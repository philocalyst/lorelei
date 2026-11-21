# Staged video slot ownership and publication

Review pin `7991e7bd4e`. This design covers source frames that arrive independently of scene transactions. The current seam couples publication and retirement in `media::Pool::publish`, while changing the visible slot is a separate `Scene::set`. `Pool::reclaim` frees every retired slot at the next completed draw. The independent tests exercise pending replacement, cancellation, format churn and rapid publication. Check the Round 3 report for outcomes on the pinned binary.

## Lifetime and memory model

1. A decode lease has an arena identity, slot index and generation. A lease keeps its range reserved until it publishes or is cancelled. Only the owning pool can expose its planes or accept its publication. A cancelled decoder returns the range. A dropped writer returns it on the next pool service operation, using an explicit reservation guard or an `abandon` operation with a separately enforced lifecycle contract.
2. Publishing produces a ready immutable frame handle. It never drops the last scene reference to the preceding image. Keep one pending frame per source. Replacing it releases the discarded ready frame if no scene, submitted pass or other user references it. Apply backpressure if all the slots are actually held.
3. Installing the ready handle into a scene record happens in the scene transaction. It pins the new handle and releases the old scene reference. Only the scene transaction can change that reference count. Multiple records that sample one frame pin it separately. A retire ticket covers the last dispatch fence that could read an old range. Synchronous CPU or `Gpu::staged` completion can discharge that fence immediately after return. Do not assign retire tickets to uninstalled intermediate frames.
4. A range is reusable when there is no writer, ready-frame holder or installed scene reference, and its last submitted fence completed. Recycling changes the generation. Protect CPU and GPU publication against partial plane writes by accepting only ready handles. The plane bytes stay immutable across all sampling until reclaim. On resize or seek, reuse larger freed blocks or coalesce adjacent free ranges, and avoid moving pinned data. Track pinned and free bytes and expose bounded-refusal diagnostics.

## Verifying the design

- Use `a_slot_a_record_still_names_is_never_reused_after_a_publish_storm` to render an old record through a stream of uncommitted source updates. A mutant frees the installed handle at producer publication time. A forced redraw must reveal the pixel change.
- Use `a_storm_between_draws_holds_a_bounded_number_of_slots` with one displayed and one pending frame. A mutant puts every unpublished successor behind the render fence. The pool exhausts while nothing reads those superseded frames.
- Use `an_abandoned_lease_is_not_lost_for_good` under cancellation and `a_resolution_change_does_not_strand_a_pool_of_free_slots` under resolution change. A mutant that omits cancellation or never reuses a large free block should exhaust the pool.
- Add a second scene record sampling the old slot. Replace the first record and finish a draw. The old lease still has one live record reference. The free-on-single-release mutant must hand it out too soon.
- Add a simulated delayed GPU completion, a stale generation and a wrong-pool handle as independent gates. A fence mutant, generation mutant or pool identity mutant must fail before the range is reused or sampled.
