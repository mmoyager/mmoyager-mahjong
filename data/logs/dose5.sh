#!/bin/bash
# The ladder rung: same validated dose shape as the +193, but the labels come
# from the third teacher level (open hands count as threats) instead of v2.
# Started from ck-best, which has never received a dose, so this is a first dose
# and can be compared directly with ck-region's +193.
export RAYON_NUM_THREADS=4
cd ~/Documents/mmoyager/mmoyager_mahjong
BASE=data/checkpoints/ck-best.bin
echo "=== generate region labels from teacher v3 (6000 games, seed 4141) ==="
./target/release/mmj-selfplay generate --games 6000 --out /tmp/dose5-region-v3.bin \
  --checkpoint "$BASE" --seats learner,learner,learner,learner \
  --greedy --epsilon 0 --batch 256 --seed 4141 \
  --dagger --dagger-shanten 2 --teacher-v3 --label-kind imitation
echo "=== train, validated dose shape ==="
.venv/bin/python python/trainer/train.py --data /tmp/dose5-region-v3.bin \
  --init "$BASE" --out /tmp/ck-dose5.bin --epochs 2 --lr 5e-5 \
  --entropy-coef 0.005 --value-coef 0.25 --return-scale 0.25 \
  --max-records 900000 --note dose5-region-v3-from-ckbest
echo "=== evaluate at three seeds (compare with ck-region: +1386/+1335/+1352) ==="
for s in 4242 5150 31337; do
  echo "--- seed $s"
  ./target/release/mmj-eval --a /tmp/ck-dose5.bin --b efficiency --games 4800 --seed $s --json
done
echo "--- dose5 vs ck-region head to head"
./target/release/mmj-eval --a /tmp/ck-dose5.bin --b data/checkpoints/ck-region.bin --games 4800 --seed 5150 --json
echo "DOSE5 DONE"
