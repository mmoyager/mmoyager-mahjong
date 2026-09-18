#!/bin/bash
# Round 29: the loop's iteration is dominated by CPU training (650-720 s of
# ~1050-1200 s). Before touching the recipe, check the cheap knobs: how many
# torch threads actually help on 4 physical cores, and does a bigger batch help
# enough to be worth the fewer optimizer steps?
cd ~/Documents/mmoyager/mmoyager_mahjong
for t in 4 8 2; do
  echo "=== threads $t (batch 512) ==="
  .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v12.bin \
    --out /tmp/th-$t.bin --max-records 300000 --epochs 1 --lr 3e-4 --batch-size 512 \
    --threads $t --seed 1 --note th-$t 2>&1 | grep -E "epoch 1"
done
for t in 4 8; do
  echo "=== threads $t (batch 1024) ==="
  .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v12.bin \
    --out /tmp/th-$t-1024.bin --max-records 300000 --epochs 1 --lr 3e-4 --batch-size 1024 \
    --threads $t --seed 1 --note th-$t-1024 2>&1 | grep -E "epoch 1"
done
echo "THREADS BENCH DONE"
