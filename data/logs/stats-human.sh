#!/bin/bash
export RAYON_NUM_THREADS=6
cd ~/Documents/mmoyager/mmoyager_mahjong
echo "=== student (ck-region), 4 seats ==="
./target/release/mmj-selfplay stats --games 400 --seats learner,learner,learner,learner \
  --checkpoint data/checkpoints/ck-region.bin --seed 900
echo "=== teacher v2, 4 seats ==="
./target/release/mmj-selfplay stats --games 400 --seats efficiency,efficiency,efficiency,efficiency \
  --teacher-v2 --seed 900
echo "=== student vs 3x teacher v2 (mixed table) ==="
./target/release/mmj-selfplay stats --games 400 --seats learner,efficiency,learner,efficiency \
  --checkpoint data/checkpoints/ck-region.bin --teacher-v2 --seed 900
echo "STATS DONE"
