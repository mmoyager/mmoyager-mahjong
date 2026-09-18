#!/bin/bash
# fold_strength=2.5 was tuned in round 5 by measuring against the *frozen v1*
# baseline. That is exactly the kind of tuning round 13 showed can be
# yardstick-specific, so re-tune it in a strong field: a differently tuned v2
# against the standard v2, with the other two seats also v2.
export RAYON_NUM_THREADS=4
cd ~/Documents/mmoyager/mmoyager_mahjong
for fold in 0.8 1.2 1.6 2.0 2.5 3.2; do
  printf "fold=%-4s " "$fold"
  ./target/release/mmj-eval --a "efficiency-v2?fold=$fold" --b efficiency-v2 \
    --games 4800 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"vs v2 {d['avg_a']-d['avg_b']:+8.1f}   rank {d['avg_rank_a']:.4f}\")"
done
echo "FOLD SWEEP DONE"
