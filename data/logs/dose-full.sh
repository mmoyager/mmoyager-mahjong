#!/bin/bash
# Arm B of the dose comparison: identical base, identical training settings,
# identical seeds as the four region-dose replicas of round 11 -- only the label
# coverage differs (every decision instead of only the 2+ shanten discards).
# Comparing the two group means therefore isolates *where* the DAgger labels go.
cd ~/Documents/mmoyager/mmoyager_mahjong
BASE=data/reference/ck-v9.bin
DATA=/tmp/attr-full.bin
for seed in 101 202 303 404; do
  echo "=== full-coverage replica seed $seed ==="
  .venv/bin/python -u python/trainer/train.py --data "$DATA" --init "$BASE" \
    --out /tmp/full-$seed.bin --epochs 2 --lr 5e-5 --seed $seed --threads 4 \
    --entropy-coef 0.005 --value-coef 0.25 --return-scale 0.25 \
    --max-records 900000 --note full-dose-$seed 2>&1 | grep -E "epoch 2"
done
export RAYON_NUM_THREADS=4
for seed in 101 202 303 404; do
  for ev in 4242 5150; do
    echo "--- full $seed, eval seed $ev"
    ./target/release/mmj-eval --a /tmp/full-$seed.bin --b efficiency --games 4800 --seed $ev --json
  done
done
echo "FULL DOSE DONE"
