#!/bin/bash
# Wait for the assist sweep, then run the strategic ablations.
cd ~/Documents/mmoyager/mmoyager_mahjong
while pgrep -f "assist-sweep.sh" > /dev/null; do sleep 10; done
CK=data/checkpoints/ck-best.bin
for mode in no-riichi force-riichi no-call force-call; do
  echo "--- filter:$mode"
  ./target/release/mmj-eval --a "filter:$mode:$CK" --b "$CK" --games 4800 --seed 4242 --json
done
echo "LAB DONE"
