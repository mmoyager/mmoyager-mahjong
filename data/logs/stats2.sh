#!/bin/bash
# Same conditions as the Tenhou benchmark: hanchan (East+South, ~10.6 hands per
# match) with red fives. Also measure the AI against a *stronger* opponent than
# the frozen baseline, to see how much of its margin comes from beating a weak,
# passive bot.
export RAYON_NUM_THREADS=6
cd ~/Documents/mmoyager/mmoyager_mahjong
while pgrep -f "stats-human.sh" > /dev/null; do sleep 20; done
echo "=== student, hanchan (comparable to Tenhou rates) ==="
./target/release/mmj-selfplay stats --games 300 --hanchan --seats learner,learner,learner,learner \
  --checkpoint data/checkpoints/ck-region.bin --seed 4242
echo "=== teacher v2, hanchan ==="
./target/release/mmj-selfplay stats --games 300 --hanchan --seats efficiency,efficiency,efficiency,efficiency \
  --teacher-v2 --seed 4242
echo "=== student vs the stronger teacher (paired) ==="
./target/release/mmj-eval --a data/checkpoints/ck-region.bin --b efficiency-v2 --games 4800 --seed 4242 --json
echo "=== student vs the frozen baseline (reference) ==="
./target/release/mmj-eval --a data/checkpoints/ck-region.bin --b efficiency --games 4800 --seed 4242 --json
echo "STATS2 DONE"
