#!/bin/bash
# Stack the one mechanism that is already established (round 16's DAgger dose,
# +220 with three replicas) onto the current best, which has never received one.
# Every claim here gets the round-24 protocol: the incumbent is re-measured in
# the same batch, and the dose is trained three times and judged by its group mean.
cd ~/Documents/mmoyager/mmoyager_mahjong
CK=data/checkpoints/ck-ab.bin
echo "=== DAgger labels on ck-ab's own states ==="
./target/release/mmj-selfplay generate --games 6000 --out /tmp/dose-ab.bin \
  --checkpoint "$CK" --seats learner,learner,learner,learner \
  --greedy --epsilon 0 --batch 256 --seed 6161 \
  --dagger --teacher-v2 --label-kind imitation
export RAYON_NUM_THREADS=8
for seed in 1 2 3; do
  echo "=== dose replica $seed ==="
  .venv/bin/python -u python/trainer/train.py --data /tmp/dose-ab.bin --init "$CK" \
    --out /tmp/dose-ab-$seed.bin --epochs 2 --lr 5e-5 --max-records 900000 --seed $seed \
    --entropy-coef 0.005 --value-coef 0.25 --return-scale 0.25 --threads 8 \
    --note dose-on-ckab-$seed 2>&1 | grep -E "epoch 2|saved"
done
echo "=== same-batch measurements (incumbent first) ==="
printf "%-14s vs v2 @9600  " "ck-ab"
./target/release/mmj-eval --a "$CK" --b efficiency-v2 --games 9600 --seed 4242 --json | \
  python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
for seed in 1 2 3; do
  printf "%-14s vs v2 @9600  " "dose-$seed"
  ./target/release/mmj-eval --a /tmp/dose-ab-$seed.bin --b efficiency-v2 --games 9600 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "DOSE ON CKAB DONE"
