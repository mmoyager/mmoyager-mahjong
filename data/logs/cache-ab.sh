#!/bin/bash
# Does the per-thread shanten memo actually buy anything? The encoder, the rule
# teacher and the engine's own decision generation all ask about the same hands
# within one decision, so the calls repeat. Old = no memo, new = memo; runs are
# interleaved so the background loop's load cancels.
cd ~/Documents/mmoyager/mmoyager_mahjong
OLD=/tmp/mmj-nocache/release/mmj-selfplay
NEW=./target/release/mmj-selfplay
gen() {
  printf "  %-5s seed %s: " "$2" "$3"
  "$1" generate --games 1200 --out /tmp/cch-$2-$3.bin --checkpoint data/checkpoints/ck-ab.bin \
    --seats learner,learner,learner,learner --greedy --epsilon 0 --batch 256 --seed "$3" \
    --dagger --teacher-v2 --label-kind imitation 2>&1 | tail -1
}
for seed in 4242 5150; do
  gen "$OLD" nocache "$seed"
  gen "$NEW" cache "$seed"
done
echo "CACHE AB DONE"
