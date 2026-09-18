#!/bin/bash
# Round 27, arm 3: does the encoder's acceptance pruning actually move throughput?
# The two binaries differ only in that change (verified byte-identical records),
# so any difference in games/s is the change. Runs are interleaved old/new/old/new
# so that drift from other jobs on the machine cancels.
cd ~/Documents/mmoyager/mmoyager_mahjong
export RAYON_NUM_THREADS=8
run() { # $1 = binary, $2 = tag
  RAYON_NUM_THREADS=8 "$1" generate --games 1200 --out /tmp/tp-$2.bin \
    --checkpoint data/checkpoints/ck-ab.bin --seats learner,learner,learner,learner \
    --greedy --epsilon 0 --batch 256 --seed 4242 --dagger --teacher-v2 \
    --label-kind imitation 2>&1 | tail -1 | sed "s/^/  $2: /"
  RAYON_NUM_THREADS=8 "$1" generate --games 1200 --out /tmp/tp-$2.bin \
    --checkpoint data/checkpoints/ck-ab.bin --seats learner,learner,learner,learner \
    --greedy --epsilon 0 --batch 256 --seed 5150 --dagger --teacher-v2 \
    --label-kind imitation 2>&1 | tail -1 | sed "s/^/  $2: /"
}
for round in 1 2; do
  echo "--- round $round ---"
  run ./target/release/mmj-selfplay old
  run /tmp/mmj-perf/release/mmj-selfplay new
done
echo "THROUGHPUT AB DONE"
