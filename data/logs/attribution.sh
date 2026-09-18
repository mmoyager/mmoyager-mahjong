#!/bin/bash
# Is the +193 dose's active ingredient the *region*, or just one more clean pass
# over teacher labels? Both arms start from the same reference checkpoint, use
# the same games (same seed), the same shape (labels on the states the learner
# reaches, no anchor, 2 epochs at lr 5e-5) and differ only in the label filter.
export RAYON_NUM_THREADS=4
cd ~/Documents/mmoyager/mmoyager_mahjong
BASE=data/reference/ck-v9.bin   # == the ck-best that measured +1150 at seed 4242
for arm in region full; do
  if [ "$arm" = "region" ]; then FILTER="--dagger-shanten 2"; else FILTER=""; fi
  echo "=== arm $arm: generate labels ==="
  ./target/release/mmj-selfplay generate --games 6000 --out /tmp/attr-$arm.bin \
    --checkpoint "$BASE" --seats learner,learner,learner,learner \
    --greedy --epsilon 0 --batch 256 --seed 7331 \
    --dagger $FILTER --teacher-v2 --label-kind imitation
  echo "=== arm $arm: train ==="
  .venv/bin/python python/trainer/train.py --data /tmp/attr-$arm.bin \
    --init "$BASE" --out /tmp/attr-$arm.ckpt --epochs 2 --lr 5e-5 \
    --entropy-coef 0.005 --value-coef 0.25 --return-scale 0.25 \
    --max-records 900000 --note attribution-$arm
done
echo "=== arm region vs frozen baseline (reference ck-best +1150, ck-region +1386) ==="
./target/release/mmj-eval --a /tmp/attr-region.ckpt --b efficiency --games 4800 --seed 4242 --json
echo "=== arm full vs frozen baseline ==="
./target/release/mmj-eval --a /tmp/attr-full.ckpt --b efficiency --games 4800 --seed 4242 --json
echo "=== region arm vs full arm, paired ==="
./target/release/mmj-eval --a /tmp/attr-region.ckpt --b /tmp/attr-full.ckpt --games 4800 --seed 5150 --json
echo "=== each arm vs the untouched reference ==="
./target/release/mmj-eval --a /tmp/attr-region.ckpt --b "$BASE" --games 4800 --seed 5150 --json
./target/release/mmj-eval --a /tmp/attr-full.ckpt --b "$BASE" --games 4800 --seed 5150 --json
echo "ATTRIBUTION DONE"
