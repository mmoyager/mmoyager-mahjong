#!/bin/bash
# Round 28, arm 2: the training recipe was A/B'd on rows (900k vs 3.4M) and on
# learning rate (3e-4 vs 1e-3) in round 26, but never on *epochs* -- every arm so
# far used 4. This arm is the best-known recipe with 8 epochs instead, same data,
# same seeds, three replicas, and the incumbent (ck-ab, which is the median
# replica of the 4-epoch arm) re-measured in the same batch.
cd ~/Documents/mmoyager/mmoyager_mahjong
export RAYON_NUM_THREADS=8
for seed in 1 2 3; do
  echo "=== 8-epoch replica $seed ==="
  .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v12.bin \
    --out /tmp/ep8-$seed.bin --hidden 768,768 --epochs 8 --lr 3e-4 --lr-decay 0.8 \
    --max-records 900000 --value-coef 0.5 --return-scale 0.25 --seed $seed --threads 8 \
    --note ep8-$seed 2>&1 | grep -E "epoch 8|saved"
done
echo "=== same-batch measurements (incumbent first) ==="
printf "  %-10s " ck-ab
./target/release/mmj-eval --a data/checkpoints/ck-ab.bin --b efficiency-v2 --games 9600 --seed 4242 --json | \
  python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
for seed in 1 2 3; do
  printf "  %-10s " "ep8-$seed"
  ./target/release/mmj-eval --a /tmp/ep8-$seed.bin --b efficiency-v2 --games 9600 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "EPOCHS8 DONE"
