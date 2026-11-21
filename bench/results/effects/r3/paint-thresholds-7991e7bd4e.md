# Paint slice 1 thresholds at wave3 7991e7bd4e

The five completed stock `effects`, `pane` and `media` scenes used `bench/fx/run.sh` in the dist profile. The CPU1 counts are calling-thread retired instructions. The 111 executor rows matched their final fresh full redraws. The remaining media, dense and flat-paint captures are still in progress.

| Prediction | Pinned evidence | Outcome |
|---|---|---|
| P1 idle | Four `effects` and `pane` rows plus `media 4`. Zero records, tiles and layer jobs. | Held on observed rows |
| P2 one spring | One record visit at each size. 690.8 and 921.0 K instructions at 300 and 3,000 cards. | 1.333× exceeds the 1.15× limit |
| P3 hover above | Four `effects` and `pane` rows. Zero prefix tiles and pyramid blocks. | Held on observed rows |
| P4 hover beneath | 16 prefix tiles and 7,281 tent pixels reach 51.14% of the pane. | Missed the 25% limit |
| P5 pane moves | 0.2 mean prefix tiles and 80 of 9,411 pyramid pixels (0.85%). | Both limits held. The 24-frame prefix total is at most six tiles. |
| P6 one video | The stock four-source row drew zero tiles at 5.3 K instructions. The selected fifth source does not exist. The patched fixture passes publication and damage checks. | Four-source timing void. 24/48 replay pending |
| P7 flat paint | Independent baseline has all 10 A1 phases. | Pinned A1 phases pending |
| P8 `effects` hover above glass | 0.023× no-cache instructions. | Held |
| P8 `effects` glass moves 1 px | 0.112× no-cache instructions. | Held |
| P8 `pane` hover above glass | 0.284× no-cache instructions. | Held |
| P8 `pane` glass moves 1 px | 0.521× no-cache instructions. | Missed the 0.50× limit |

P5 prints its mean prefix count to one decimal place. A displayed 0.2 over 24 samples permits at most six prefix tiles in total. The pane is 210 pixels wide and moves only horizontally within the frame. An edge of its halo spans at least seven tiles, so even the full measured prefix total fits the per-frame perimeter limit.

[Pinned effects 300](stock-effects-300-7991e7bd4e.txt) · [Pinned effects 3,000](stock-effects-3000-7991e7bd4e.txt) · [Pinned pane 300 and 3,000](stock-pane-7991e7bd4e.txt) · [Pinned media 4](stock-media4-7991e7bd4e.txt) · [Independent P7 baseline](p7-baseline-71ee3be0aa.txt)
