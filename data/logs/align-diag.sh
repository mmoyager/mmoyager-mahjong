#!/bin/bash
# Where does the *current* model still disagree with its teacher, on the states it
# actually reaches? Two deterministic runs over the same deals: one records the
# student's own choices, the other records the teacher's labels for the same
# states. Determinism is what makes the two files row-aligned (round 19's lesson).
cd ~/Documents/mmoyager/mmoyager_mahjong
CK=data/checkpoints/ck-dagger.bin
./target/release/mmj-selfplay generate --games 2000 --out /tmp/diag2-beh.bin \
  --checkpoint "$CK" --seats learner,learner,learner,learner \
  --greedy --epsilon 0 --batch 256 --seed 5150 --label-kind selfplay > /dev/null
./target/release/mmj-selfplay generate --games 2000 --out /tmp/diag2-tea.bin \
  --checkpoint "$CK" --seats learner,learner,learner,learner \
  --greedy --epsilon 0 --batch 256 --seed 5150 --dagger --teacher-v2 --label-kind imitation > /dev/null
echo "ALIGN DIAG DONE"
