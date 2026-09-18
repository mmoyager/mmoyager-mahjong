#!/bin/bash
# Which dimension of the training recipe matters? Round 25 gave a three-replica
# group for "900k rows x 4 epochs, lr 3e-4" (+271.5 against the stronger
# yardstick, sd 65) that beat the old "3.4M x 4, lr 1e-3" by a wide margin, but
# that was a cross-experiment comparison. Two arms isolate the dimensions:
#   B: 3.4M rows x 4 epochs, lr 3e-4   (does more data help at the same rate?)
#   C: 900k rows x 4 epochs, lr 1e-3   (does the old aggressive rate matter?)
# Arm A (900k x 4, lr 3e-4) is already measured: +220.2 / +344.6 / +249.7.
cd ~/Documents/mmoyager/mmoyager_mahjong
export RAYON_NUM_THREADS=8
for arm in b c; do
  case $arm in
    b) rows=3400000; lr=3e-4 ;;
    c) rows=900000;  lr=1e-3 ;;
  esac
  for seed in 1 2 3; do
    echo "=== arm $arm replica $seed (rows $rows, lr $lr) ==="
    .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v12.bin \
      --out /tmp/rec-$arm-$seed.bin --hidden 768,768 --epochs 4 --lr $lr --lr-decay 0.8 \
      --max-records $rows --value-coef 0.5 --return-scale 0.25 --seed $seed --threads 8 \
      --note recipe-$arm-$seed 2>&1 | grep -E "epoch 4|saved"
    printf "  $arm-$seed vs v2 @9600  "
    ./target/release/mmj-eval --a /tmp/rec-$arm-$seed.bin --b efficiency-v2 --games 9600 --seed 4242 --json | \
      python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
  done
done
echo "RECIPE AB DONE"
