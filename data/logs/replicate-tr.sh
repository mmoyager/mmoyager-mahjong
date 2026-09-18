#!/bin/bash
# Replication of the teacher-ranking result. A single training run's margin is
# not evidence (round 9-11), and this is the first change that improved the
# *strong-opponent* margin, so it gets the full treatment: two more independent
# replicas of the same recipe, each measured against both yardsticks.
cd ~/Documents/mmoyager/mmoyager_mahjong
for seed in 2 3; do
  echo "=== replica seed $seed ==="
  .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v11.bin \
    --out /tmp/ck-tr$seed.bin --hidden 768,768 --epochs 4 --lr 1e-3 --lr-decay 0.8 \
    --seed $seed --max-records 3400000 --value-coef 0.5 --return-scale 0.25 --threads 8 \
    --note teacher-rank-737-seed$seed 2>&1 | grep -E "epoch 4|saved"
done
export RAYON_NUM_THREADS=8
for name in ck-tr2 ck-tr3; do
  for opp in efficiency efficiency-v2; do
    printf "%-8s vs %-14s " "$name" "$opp"
    ./target/release/mmj-eval --a /tmp/$name.bin --b "$opp" --games 4800 --seed 4242 --json | \
      python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
  done
done
echo "REPLICATE DONE"
