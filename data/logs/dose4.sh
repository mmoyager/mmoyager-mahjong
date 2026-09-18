#!/bin/bash
# Attribution for the +193: the validated dose started from ck-best and labelled
# only the 2+ shanten discards. This control starts from the *same* ck-best with
# the *same* shape but labels every decision, so the difference between the two
# isolates the region focus from "one more clean pass over teacher labels".
export RAYON_NUM_THREADS=4
cd ~/Documents/mmoyager/mmoyager_mahjong
while pgrep -f "dose3.sh" > /dev/null; do sleep 20; done
BASE=data/checkpoints/ck-best.bin
echo "=== generate full-coverage labels from ck-best (6000 games, new seed) ==="
./target/release/mmj-selfplay generate --games 6000 --out /tmp/dose4-full.bin \
  --checkpoint "$BASE" --seats learner,learner,learner,learner \
  --greedy --epsilon 0 --batch 256 --seed 6061 \
  --dagger --teacher-v2 --label-kind imitation
echo "=== train, same shape as the validated dose ==="
.venv/bin/python python/trainer/train.py --data /tmp/dose4-full.bin \
  --init "$BASE" --out /tmp/ck-dose4.bin --epochs 2 --lr 5e-5 \
  --entropy-coef 0.005 --value-coef 0.25 --return-scale 0.25 \
  --max-records 900000 --note dose4-full-from-ckbest
echo "=== evaluate at three seeds (reference: ck-region +1386/+1335/+1352 over ck-best) ==="
for s in 4242 5150 31337; do
  echo "--- seed $s"
  ./target/release/mmj-eval --a /tmp/ck-dose4.bin --b efficiency --games 4800 --seed $s --json
done
echo "--- dose4 vs ck-best head to head"
./target/release/mmj-eval --a /tmp/ck-dose4.bin --b "$BASE" --games 4800 --seed 5150 --json
echo "DOSE4 DONE"
