#!/bin/bash
export RAYON_NUM_THREADS=3
cd ~/Documents/mmoyager/mmoyager_mahjong
for t in efficiency-v2 efficiency-v3; do
  for s in 4242 5150 31337; do
    echo "--- $t vs frozen baseline, seed $s"
    ./target/release/mmj-eval --a "$t" --b efficiency --games 4800 --seed $s --json
  done
done
echo "--- v3 vs v2, seed 5150"
./target/release/mmj-eval --a efficiency-v3 --b efficiency-v2 --games 4800 --seed 5150 --json
echo "TEACHER CMP DONE"
