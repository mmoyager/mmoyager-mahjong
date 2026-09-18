#!/bin/bash
# The yardstick: the current best checkpoint against the frozen baseline, in the
# same session and with the same binary as every other number reported today.
export RAYON_NUM_THREADS=3
cd ~/Documents/mmoyager/mmoyager_mahjong
echo "--- ck-best vs frozen baseline (reference)"
./target/release/mmj-eval --a data/checkpoints/ck-best.bin --b efficiency --games 4800 --seed 4242 --json
echo "--- ck-best vs frozen baseline, second seed"
./target/release/mmj-eval --a data/checkpoints/ck-best.bin --b efficiency --games 4800 --seed 5150 --json
echo "REF EVAL DONE"
