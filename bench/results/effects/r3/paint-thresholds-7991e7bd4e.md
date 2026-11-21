# Paint slice 1 thresholds at wave3 7991e7bd4e

The four completed stock `effects` and `pane` scenes used `bench/fx/run.sh` in the dist profile. The CPU1 counts are calling-thread retired instructions. The 84 executor rows matched their final fresh full redraws. The media, dense and flat-paint captures are still in progress.

| Prediction | Pinned evidence | Outcome |
|---|---|---|
| P1 idle | Four `effects` and `pane` rows. Zero records, tiles and layer jobs. | Held on observed rows |
| P2 one spring | One record visit at each size. 690.8 and 921.0 K instructions at 300 and 3,000 cards. | 1.333× exceeds the 1.15× limit |
| P3 hover above | Four `effects` and `pane` rows. Zero prefix tiles and pyramid blocks. | Held on observed rows |
| P4 hover beneath | 16 prefix tiles and 7,281 tent pixels reach 51.14% of the pane. | Missed the 25% limit |
| P5 pane moves | 0.2 mean prefix tiles and 80 of 9,411 pyramid pixels (0.85%). | Pyramid held. Per-frame prefix maximum unavailable. |
| P6 one video | The focused fixture proves the stock four-source row lacks the selected source. The patch passes publication and damage checks. | Stock timing incomplete |
| P7 flat paint | Independent baseline has all 10 A1 phases. | Pinned A1 phases pending |
| P8 `effects` hover above glass | 0.023× no-cache instructions. | Held |
| P8 `effects` glass moves 1 px | 0.112× no-cache instructions. | Held |
| P8 `pane` hover above glass | 0.284× no-cache instructions. | Held |
| P8 `pane` glass moves 1 px | 0.521× no-cache instructions. | Missed the 0.50× limit |

[Pinned effects 300](stock-effects-300-7991e7bd4e.txt) · [Pinned effects 3,000](stock-effects-3000-7991e7bd4e.txt) · [Pinned pane 300 and 3,000](stock-pane-7991e7bd4e.txt) · [Independent P7 baseline](p7-baseline-71ee3be0aa.txt)
