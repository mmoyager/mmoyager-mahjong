#!/bin/bash
# The ladder's next rung: labels from the teacher that *itself* uses the
# discard-sequence read. Round 22 exposed the read as a feature (+198 for the
# student) but the labels still came from the shape-only teacher; the teacher
# gained +136 from the read on its own, so those decisions are the last piece
# that was still missing.
cd ~/Documents/mmoyager/mmoyager_mahjong
echo "=== labels from teacher v4 (sequence-read safety model) ==="
./target/release/mmj-selfplay imitate --games 8000 --out data/selfplay/im-v14.bin \
  --seed 97 --batch 384 --teacher-v4
echo "=== train, same recipe as ck-read ==="
.venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v14.bin \
  --out /tmp/ck-v4.bin --hidden 768,768 --epochs 4 --lr 1e-3 --lr-decay 0.8 \
  --max-records 3400000 --value-coef 0.5 --return-scale 0.25 --threads 8 \
  --note teacher-v4-labels 2>&1 | grep -E "epoch 4|saved"
export RAYON_NUM_THREADS=8
for opp in efficiency-v2 efficiency; do
  for s in 4242 5150; do
    printf "ck-v4 vs %-14s @9600 seed %s  " "$opp" "$s"
    ./target/release/mmj-eval --a /tmp/ck-v4.bin --b "$opp" --games 9600 --seed $s --json | \
      python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
  done
done
echo "LADDER V4 DONE"
