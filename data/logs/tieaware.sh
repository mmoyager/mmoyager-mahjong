#!/bin/bash
# Tie-aware imitation arm. The variance study currently running trains four
# replicas with seeds 101/202/303/404 on this same data, base and recipe, so it
# doubles as the control arm; this script mirrors it exactly with --tie-aware.
#
# Rationale: 53.9% of imitation rows are exact ties (same shanten, same tile
# acceptance as the teacher's choice), and the old objective pushed all of the
# probability onto the teacher's arbitrary pick. The comparison therefore also
# tests whether the project's "accuracy does not mean strength" observation is
# partly explained by the policy being trained on noise in half its rows.
cd ~/Documents/mmoyager/mmoyager_mahjong
while pgrep -f "variance.sh" > /dev/null; do sleep 20; done
BASE=data/reference/ck-v9.bin
DATA=/tmp/attr-region.bin
for seed in 101 202 303 404; do
  echo "=== tie-aware replica seed $seed ==="
  .venv/bin/python -u python/trainer/train.py --data "$DATA" --init "$BASE" \
    --out /tmp/tie-$seed.bin --epochs 2 --lr 5e-5 --seed $seed --threads 4 \
    --entropy-coef 0.005 --value-coef 0.25 --return-scale 0.25 --tie-aware \
    --max-records 900000 --note tie-aware-$seed 2>&1 | grep -E "epoch 2|val_acc|ties"
done
export RAYON_NUM_THREADS=4
for seed in 101 202 303 404; do
  for ev in 4242 5150; do
    echo "--- tie-aware $seed, eval seed $ev"
    ./target/release/mmj-eval --a /tmp/tie-$seed.bin --b efficiency --games 4800 --seed $ev --json
  done
done
echo "TIEAWARE DONE"
