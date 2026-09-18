#!/bin/bash
# Same recipe, same data volume, same teacher labels -- only the danger feature
# block differs:
#   arm A: im-v12 (open-hand threats ON, no sequence read)  <- the old block
#   arm B: im-v13 (open-hand threats OFF, sequence read ON) <- round 22's block
# Round 14 measured the teacher's own margin with each model; this asks whether
# the *student* inherits it.
cd ~/Documents/mmoyager/mmoyager_mahjong
for arm in a b; do
  case $arm in
    a) data=data/selfplay/im-v12.bin ;;
    b) data=data/selfplay/im-v13.bin ;;
  esac
  echo "=== arm $arm ($data) ==="
  .venv/bin/python -u python/trainer/train.py --data "$data" --out /tmp/ck-read-$arm.bin \
    --hidden 768,768 --epochs 4 --lr 1e-3 --lr-decay 0.8 --max-records 3400000 \
    --value-coef 0.5 --return-scale 0.25 --threads 8 --note read-ab-$arm 2>&1 | grep -E "epoch 4|saved"
done
export RAYON_NUM_THREADS=8
for arm in a b; do
  for opp in efficiency-v2 efficiency; do
    printf "arm %s vs %-14s " "$arm" "$opp"
    ./target/release/mmj-eval --a /tmp/ck-read-$arm.bin --b "$opp" --games 9600 --seed 4242 --json | \
      python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
  done
done
echo "READ AB DONE"
