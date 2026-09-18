#!/bin/bash
# Repeat the *validated* recipe shape exactly (region rows only, no anchor,
# 2 epochs at lr 5e-5) from the current best, on fresh deals, and verify at
# three seeds. Iterations 18-20 went through the loop, which keeps the 900k
# imitation anchor in the mixture -- the validated dose had no anchor, so that
# difference is the first thing to rule out.
export RAYON_NUM_THREADS=4
cd ~/Documents/mmoyager/mmoyager_mahjong
CK=data/checkpoints/ck-region.bin
echo "=== generate fresh region labels (6000 games, new seed) ==="
./target/release/mmj-selfplay generate --games 6000 --out /tmp/dose2-region.bin \
  --checkpoint "$CK" --seats learner,learner,learner,learner \
  --greedy --epsilon 0 --batch 256 --seed 9091 \
  --dagger --dagger-shanten 2 --teacher-v2 --label-kind imitation
echo "=== train on region rows only ==="
.venv/bin/python python/trainer/train.py --data /tmp/dose2-region.bin \
  --init "$CK" --out /tmp/ck-dose2.bin --epochs 2 --lr 5e-5 \
  --entropy-coef 0.005 --value-coef 0.25 --return-scale 0.25 \
  --max-records 900000 --note dose2-region-only
echo "=== evaluate against the frozen baseline at three seeds ==="
for s in 4242 5150 31337; do
  echo "--- seed $s"
  ./target/release/mmj-eval --a /tmp/ck-dose2.bin --b efficiency --games 4800 --seed $s --json
done
echo "=== head to head against the current best ==="
./target/release/mmj-eval --a /tmp/ck-dose2.bin --b "$CK" --games 4800 --seed 5150 --json
echo "DOSE2 DONE"
