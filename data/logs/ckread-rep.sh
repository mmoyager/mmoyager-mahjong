#!/bin/bash
# How much does the *current* lineage move between training runs? ck-read's
# +163 against the stronger yardstick is a single run, and every claim since
# round 15 rests on this configuration, so its run-to-run spread needs measuring.
cd ~/Documents/mmoyager/mmoyager_mahjong
for seed in 11 22; do
  echo "=== ck-read replica seed $seed ==="
  .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v13.bin \
    --out /tmp/ck-read-rep$seed.bin --hidden 768,768 --epochs 4 --lr 1e-3 --lr-decay 0.8 \
    --max-records 3400000 --value-coef 0.5 --return-scale 0.25 --seed $seed --threads 8 \
    --note ck-read-replica-$seed 2>&1 | grep -E "epoch 4|saved"
  export RAYON_NUM_THREADS=8
  for s in 4242 5150; do
    printf "  rep$seed vs v2 @9600 seed %s  " "$s"
    ./target/release/mmj-eval --a /tmp/ck-read-rep$seed.bin --b efficiency-v2 --games 9600 --seed $s --json | \
      python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
  done
done
echo "CKREAD REP DONE"
