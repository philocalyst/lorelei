# rethink-baseline

Commit `b9a196b6eccd0130377b844e900ccd335d9b9a26`

Native headless macOS. Typed core: prepared Latin-1 DejaVu Sans 14/28 px, opaque colors, arithmetic rows, remount on resize, group-1 keymap and CPU spans. pbench includes offscreen Metal.

| Cell | Operation | Median instructions |
|---|---|---:|
| sheet_x/rows1000 | mount (instr_p50) | 357,339 |
| sheet_x/rows1000 | floor (instr_p50) | 0 |
| sheet_x/rows1000 | idle (instr_p50) | 63 |
| sheet_x/rows1000 | hover (instr_p50) | 5,192 |
| sheet_x/rows1000 | hover_move (instr_p50) | 6,359 |
| sheet_x/rows1000 | theme (instr_p50) | 27,283 |
| sheet_x/rows1000 | edit_exposed (instr_p50) | 1,928 |
| sheet_x/rows1000 | edit_unexposed (instr_p50) | 1,344 |
| sheet_x/rows1000 | part_model_edit (instr_p50) | 58 |
| sheet_x/rows1000 | part_total_word (instr_p50) | 714 |
| sheet_x/rows1000 | part_delta_total (instr_p50) | 634 |
| sheet_x/rows1000 | part_diff_only (instr_p50) | 162 |
| sheet_x/rows1000 | part_journal_only (instr_p50) | 724 |
| sheet_x/rows1000 | part_mark_cell (instr_p50) | 89 |
| sheet_x/rows1000 | part_pointer_same (instr_p50) | 439 |
| sheet_x/rows1000 | select (instr_p50) | 766 |
| sheet_x/rows1000 | frame_in_motion (instr_p50) | 168 |
| sheet_x/rows1000 | frame_at_rest (instr_p50) | 10 |
| sheet_x/rows1000 | scroll_3_rows (instr_p50) | 5,116 |
| sheet_x/rows1000 | scroll_6px (instr_p50) | 980 |
| sheet_x/rows1000 | scroll_page (instr_p50) | 33,196 |
| sheet_x/rows1000 | insert_row (instr_p50) | 20,140 |
| sheet_x/rows1000 | remove_row (instr_p50) | 19,840 |
| sheet_x/rows1000 | redescribe_idle (instr_p50) | 13,988 |
| sheet_x/rows1000 | redescribe_edit (instr_p50) | 14,048 |
| sheet_x/rows10000 | mount (instr_p50) | 355,733 |
| sheet_x/rows10000 | floor (instr_p50) | 0 |
| sheet_x/rows10000 | idle (instr_p50) | 63 |
| sheet_x/rows10000 | hover (instr_p50) | 5,192 |
| sheet_x/rows10000 | hover_move (instr_p50) | 6,359 |
| sheet_x/rows10000 | theme (instr_p50) | 27,283 |
| sheet_x/rows10000 | edit_exposed (instr_p50) | 1,928 |
| sheet_x/rows10000 | edit_unexposed (instr_p50) | 1,344 |
| sheet_x/rows10000 | part_model_edit (instr_p50) | 58 |
| sheet_x/rows10000 | part_total_word (instr_p50) | 714 |
| sheet_x/rows10000 | part_delta_total (instr_p50) | 634 |
| sheet_x/rows10000 | part_diff_only (instr_p50) | 162 |
| sheet_x/rows10000 | part_journal_only (instr_p50) | 724 |
| sheet_x/rows10000 | part_mark_cell (instr_p50) | 89 |
| sheet_x/rows10000 | part_pointer_same (instr_p50) | 439 |
| sheet_x/rows10000 | select (instr_p50) | 766 |
| sheet_x/rows10000 | frame_in_motion (instr_p50) | 168 |
| sheet_x/rows10000 | frame_at_rest (instr_p50) | 10 |
| sheet_x/rows10000 | scroll_3_rows (instr_p50) | 5,116 |
| sheet_x/rows10000 | scroll_6px (instr_p50) | 980 |
| sheet_x/rows10000 | scroll_page (instr_p50) | 33,196 |
| sheet_x/rows10000 | insert_row (instr_p50) | 87,640 |
| sheet_x/rows10000 | remove_row (instr_p50) | 87,340 |
| sheet_x/rows10000 | redescribe_idle (instr_p50) | 13,988 |
| sheet_x/rows10000 | redescribe_edit (instr_p50) | 14,048 |
| events_x/rows10000 | pointer_onto_row (dispatch_instr) | 724 |
| events_x/rows10000 | pointer_onto_row (step_instr) | 1,578 |
| events_x/rows10000 | pointer_same_row (dispatch_instr) | 462 |
| events_x/rows10000 | pointer_same_row (step_instr) | 527 |
| events_x/rows10000 | click_row (dispatch_instr) | 2,221 |
| events_x/rows10000 | click_row (step_instr) | 3,137 |
| events_x/rows10000 | key_unfocused (dispatch_instr) | 115 |
| events_x/rows10000 | key_unfocused (step_instr) | 331 |
| events_x/rows10000 | text_focused_field (dispatch_instr) | 233 |
| events_x/rows10000 | text_focused_field (step_instr) | 1,421 |
| text_x | rows/mount (instr_p50) | 120,273 |
| text_x | rows/idle (instr_p50) | 53 |
| text_x | rows/hover (instr_p50) | 2,444 |
| text_x | rows/tint (text colour alone, every line) (instr_p50) | 1,528 |
| text_x | rows/scroll 3 rows (instr_p50) | 3,813 |
| text_x | rows/scroll a page (24 rows enter) (instr_p50) | 18,585 |
| text_x | rows/label edit (toggle a suffix on an exposed row) (instr_p50) | 1,886 |
| text_x | rows/value edit (an exposed row's number) (instr_p50) | 1,184 |
| text_x | rows/remove a row then insert one (instr_p50) | 32,171 |
| text_x | rows/collect the glyph memo (clear, re-describe what is exposed) (instr_p50) | 40,300 |
| text_x | typing/type a character (event, handler, settle) (instr_p50) | 3,014 |
| text_x | typing/caret move (instr_p50) | 1,370 |
| text_x | search/type a character (event, handler, settle) (instr_p50) | 832,961 |
| text_x | search/caret move (instr_p50) | 1,370 |
| text_x | search/narrow by a key, widen by a backspace (24 rows re-enter) (instr_p50) | 199,468 |
| anim_x/208 | frame_endless (instr_p50) | 161 |
| anim_x/208 | frame_endless_and_fade (instr_p50) | 265 |
| anim_x/208 | fade_start_turn (instr_p50) | 676 |
| anim_x/208 | pointer_words (instr_p50) | 4,182 |
| anim_x/208 | scroll_3_rows (instr_p50) | 3,413 |
