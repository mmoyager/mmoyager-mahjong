#!/bin/bash
# Round 29: the loop's iteration is dominated by training (650-1100 s), not by
# self-play (180 s) or evaluation (360 s). Before changing anything, measure how
# the trainer's throughput scales with batch size on a fixed slice of data.
cd ~/Documents/mmoyager/mmoyager_mahjong
for bs in 512 1024 2048; do
  echo "=== batch-size $bs ==="
  .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v12.bin \
    --out /tmp/bs-$bs.bin --max-records 300000 --epochs 1 --lr 3e-4 --batch-size $bs \
    --seed 1 --note bs-$bs 2>&1 | grep -E "loaded|epoch 1|saved"
done
echo "BATCH BENCH DONE"
