#!/bin/bash
export RAYON_NUM_THREADS=3
cd ~/Documents/mmoyager/mmoyager_mahjong
CK=/tmp/ck-region.bin
./target/release/mmj-selfplay generate --games 2000 --out /tmp/beh-region.bin \
  --checkpoint "$CK" --seats learner,learner,learner,learner \
  --greedy --epsilon 0 --batch 256 --seed 5150 --label-kind selfplay > /dev/null
./target/release/mmj-selfplay generate --games 2000 --out /tmp/lab-region.bin \
  --checkpoint "$CK" --seats learner,learner,learner,learner \
  --greedy --epsilon 0 --batch 256 --seed 5150 --dagger --teacher-v2 --label-kind imitation > /dev/null
echo "REGION BEHAVIOR DONE"
