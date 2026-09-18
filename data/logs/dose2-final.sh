#!/bin/bash
cd ~/Documents/mmoyager/mmoyager_mahjong
CK=data/checkpoints/ck-dagger.bin
export RAYON_NUM_THREADS=8
for arm in full target; do
  if [ ! -f /tmp/ck-dose2-$arm.bin ]; then
    .venv/bin/python -u python/trainer/train.py --data /tmp/dose-$arm.bin --init "$CK" \
      --out /tmp/ck-dose2-$arm.bin --epochs 2 --lr 5e-5 --max-records 900000 \
      --entropy-coef 0.005 --value-coef 0.25 --return-scale 0.25 --threads 8 \
      --note second-dose-$arm 2>&1 | grep -E "epoch 2|saved"
  fi
  printf "second dose (%-6s) vs v2: " "$arm"
  ./target/release/mmj-eval --a /tmp/ck-dose2-$arm.bin --b efficiency-v2 --games 4800 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
  printf "second dose (%-6s) vs v1: " "$arm"
  ./target/release/mmj-eval --a /tmp/ck-dose2-$arm.bin --b efficiency --games 4800 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "DOSE2 FINAL DONE"
