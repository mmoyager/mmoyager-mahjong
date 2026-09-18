#!/bin/bash
cd ~/Documents/mmoyager/mmoyager_mahjong
./target/release/mmj-selfplay generate --games 2000 --out /tmp/diag-teacher-v2.bin \
  --checkpoint data/checkpoints/ck-best.bin --seats learner,learner,learner,learner \
  --greedy --epsilon 0 --batch 256 --seed 5150 \
  --dagger --teacher-v2 --label-kind imitation > /dev/null
echo "V2 LABELS DONE"
