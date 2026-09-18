#!/bin/bash
# Second-seed confirmation of the region-distilled checkpoint, the only
# configuration this round that beat the reference against the external
# opponent instead of only in a mirror match.
export RAYON_NUM_THREADS=4
cd ~/Documents/mmoyager/mmoyager_mahjong
while pgrep -f "confirm.sh" > /dev/null; do sleep 15; done
echo "--- region-tuned vs baseline, seed 5150 (reference: ck-best +1136)"
./target/release/mmj-eval --a /tmp/ck-region.bin --b efficiency --games 4800 --seed 5150 --json
echo "--- region-tuned vs baseline, seed 31337 (independent check)"
./target/release/mmj-eval --a /tmp/ck-region.bin --b efficiency --games 4800 --seed 31337 --json
echo "--- ck-best vs baseline, seed 31337 (matched reference)"
./target/release/mmj-eval --a data/checkpoints/ck-best.bin --b efficiency --games 4800 --seed 31337 --json
echo "CONFIRM2 DONE"
