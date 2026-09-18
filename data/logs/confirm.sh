#!/bin/bash
# Second-seed confirmation of the best configuration measured this round.
export RAYON_NUM_THREADS=3
cd ~/Documents/mmoyager/mmoyager_mahjong
while pgrep -f "ref-eval.sh" > /dev/null || pgrep -f "region-chain.sh" > /dev/null; do sleep 15; done
CK=data/checkpoints/ck-best.bin
echo "--- assisted sh=1&calls=1 vs baseline, seed 5150"
./target/release/mmj-eval --a "assisted:$CK?sh=1&calls=1" --b efficiency --games 4800 --seed 5150 --json
echo "--- assisted sh=2&calls=1 vs baseline, seed 5150"
./target/release/mmj-eval --a "assisted:$CK?sh=2&calls=1" --b efficiency --games 4800 --seed 5150 --json
echo "CONFIRM DONE"
