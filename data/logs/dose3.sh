#!/bin/bash
# Attribution control for the +193 dose: identical shape (labels on the states
# the current best actually reaches, region rows only, no anchor, 2 epochs at
# lr 5e-5) but with the teacher labelling *every* decision instead of only the
# 2+ shanten discards. If this helps as much as dose 2, the mechanism is "one
# more pass over clean teacher labels", not "the region was the weak spot".
export RAYON_NUM_THREADS=4
cd ~/Documents/mmoyager/mmoyager_mahjong
while pgrep -f "dose2.sh" > /dev/null; do sleep 20; done
CK=data/checkpoints/ck-region.bin
echo "=== generate full-coverage labels (6000 games, same seed as dose 2) ==="
./target/release/mmj-selfplay generate --games 6000 --out /tmp/dose3-full.bin \
  --checkpoint "$CK" --seats learner,learner,learner,learner \
  --greedy --epsilon 0 --batch 256 --seed 9091 \
  --dagger --teacher-v2 --label-kind imitation
echo "=== train, same shape as dose 2 ==="
.venv/bin/python python/trainer/train.py --data /tmp/dose3-full.bin \
  --init "$CK" --out /tmp/ck-dose3.bin --epochs 2 --lr 5e-5 \
  --entropy-coef 0.005 --value-coef 0.25 --return-scale 0.25 \
  --max-records 900000 --note dose3-full-coverage
echo "=== evaluate at three seeds ==="
for s in 4242 5150 31337; do
  echo "--- seed $s"
  ./target/release/mmj-eval --a /tmp/ck-dose3.bin --b efficiency --games 4800 --seed $s --json
done
echo "--- vs dose 2"
./target/release/mmj-eval --a /tmp/ck-dose3.bin --b /tmp/ck-dose2.bin --games 4800 --seed 5150 --json
echo "DOSE3 DONE"
