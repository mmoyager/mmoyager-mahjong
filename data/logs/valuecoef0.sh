#!/bin/bash
# Round 27, arm 2. The training objective's value-loss scale is listed explicitly
# in the project goal. The value head's honest R^2 is ~0.11-0.14, so most of what
# it sends into the shared trunk is noise. This is a clean A/B against arm A of
# recipe-ab.sh: identical recipe (900k rows x 4 epochs, lr 3e-4, lr-decay 0.8),
# identical data, identical seeds -- only --value-coef changes, from 0.5 to 0.0.
# ck-ab.bin is arm A's median replica, and it is re-measured in the same batch.
cd ~/Documents/mmoyager/mmoyager_mahjong
export RAYON_NUM_THREADS=8
while pgrep -f "dose-on-ckab.sh" > /dev/null; do sleep 10; done
for seed in 1 2 3; do
  echo "=== value-coef 0 replica $seed ==="
  .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v12.bin \
    --out /tmp/vc0-$seed.bin --hidden 768,768 --epochs 4 --lr 3e-4 --lr-decay 0.8 \
    --max-records 900000 --value-coef 0.0 --return-scale 0.25 --seed $seed --threads 8 \
    --note valuecoef0-$seed 2>&1 | grep -E "epoch 4|saved"
done
echo "=== same-batch measurements (incumbent first) ==="
printf "%-14s vs v2 @9600  " "ck-ab"
./target/release/mmj-eval --a data/checkpoints/ck-ab.bin --b efficiency-v2 --games 9600 --seed 4242 --json | \
  python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
for seed in 1 2 3; do
  printf "%-14s vs v2 @9600  " "vc0-$seed"
  ./target/release/mmj-eval --a /tmp/vc0-$seed.bin --b efficiency-v2 --games 9600 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "VALUECOEF0 DONE"
