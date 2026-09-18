#!/bin/bash
# How much does a *training run's* random seed move the measured strength?
#
# This is the number the whole project has been missing: every recipe comparison
# so far was n=1 per side, and round 10 showed two identical 512 replicas
# differing by 373 points. Here four replicas share the data file, the base
# checkpoint and the recipe, and differ only in the shuffle seed (and the
# initial weights' loss landscape is shared because they all start from ck-v9).
#
# Threads are capped so the training loop keeps half the machine.
cd ~/Documents/mmoyager/mmoyager_mahjong
BASE=data/reference/ck-v9.bin
DATA=/tmp/attr-region.bin
for seed in 101 202 303 404; do
  echo "=== replica seed $seed ==="
  .venv/bin/python -u python/trainer/train.py --data "$DATA" --init "$BASE" \
    --out /tmp/var-$seed.bin --epochs 2 --lr 5e-5 --seed $seed --threads 4 \
    --entropy-coef 0.005 --value-coef 0.25 --return-scale 0.25 \
    --max-records 900000 --note variance-study-$seed 2>&1 | grep -E "epoch 2|val_acc"
done
export RAYON_NUM_THREADS=4
for seed in 101 202 303 404; do
  for ev in 4242 5150; do
    echo "--- replica $seed, eval seed $ev"
    ./target/release/mmj-eval --a /tmp/var-$seed.bin --b efficiency --games 4800 --seed $ev --json
  done
done
echo "VARIANCE DONE"
