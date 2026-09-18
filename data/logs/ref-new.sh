#!/bin/bash
# The furiten fix changes what the encoder reports, and it also changes the
# frozen baseline's own play in those rare spots. Every comparison must therefore
# be re-measured with the current binary before it is quoted.
export RAYON_NUM_THREADS=3
cd ~/Documents/mmoyager/mmoyager_mahjong
for s in 4242 5150 31337; do
  echo "--- ck-region vs efficiency, seed $s (current binary)"
  ./target/release/mmj-eval --a data/checkpoints/ck-region.bin --b efficiency --games 4800 --seed $s --json
done
for s in 4242 5150 31337; do
  echo "--- ck-best vs efficiency, seed $s (current binary)"
  ./target/release/mmj-eval --a data/checkpoints/ck-best.bin --b efficiency --games 4800 --seed $s --json
done
echo "REF NEW DONE"
