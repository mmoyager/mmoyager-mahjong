#!/bin/bash
# Round 22's feature A/B (+198) was one training run per arm, and round 24 showed
# that recipe's run-to-run spread is ~124 points. Redone here with three replicas
# per arm on a faster, gentler recipe so the group means can actually settle it.
# The two arms differ in exactly one thing: the encoder's danger block.
cd ~/Documents/mmoyager/mmoyager_mahjong
export RAYON_NUM_THREADS=8
for arm in old new; do
  case $arm in
    old) data=data/selfplay/im-v12.bin ;;
    new) data=data/selfplay/im-v13.bin ;;
  esac
  for seed in 1 2 3; do
    echo "=== arm $arm replica $seed ==="
    .venv/bin/python -u python/trainer/train.py --data "$data" --out /tmp/ab-$arm-$seed.bin \
      --hidden 768,768 --epochs 4 --lr 3e-4 --lr-decay 0.8 --max-records 900000 \
      --value-coef 0.5 --return-scale 0.25 --seed $seed --threads 8 \
      --note ab-$arm-$seed 2>&1 | grep -E "epoch 4|saved"
    printf "  $arm-$seed vs v2 @9600  "
    ./target/release/mmj-eval --a /tmp/ab-$arm-$seed.bin --b efficiency-v2 --games 9600 --seed 4242 --json | \
      python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
  done
done
echo "AB FEATURE DONE"
