#!/bin/bash
# Limited to 3 rayon threads so the critical-path jobs keep the rest of the machine.
export RAYON_NUM_THREADS=3
cd ~/Documents/mmoyager/mmoyager_mahjong
CK=data/checkpoints/ck-best.bin
for spec in "sh=2" "sh=1&calls=1" "sh=2&calls=1"; do
  echo "--- assisted:$spec vs frozen baseline"
  ./target/release/mmj-eval --a "assisted:$CK?$spec" --b efficiency --games 4800 --seed 4242 --json
done
echo "HYBRID EVAL DONE"
