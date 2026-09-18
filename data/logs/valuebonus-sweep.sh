#!/bin/bash
# The teacher's discard value bonus is `yaku_weight * yaku + dora_weight * dora`
# with 2.5 / 0.8 hardcoded since round 5 and never swept. Now that the student
# reproduces the teacher faithfully (0.969), a stronger teacher should transfer,
# so it is worth checking the knobs in a strong field (the metric that matters).
export RAYON_NUM_THREADS=6
cd ~/Documents/mmoyager/mmoyager_mahjong
run() {
  printf "%-34s " "$1"
  ./target/release/mmj-eval --a "$1" --b efficiency-v2 --games 4800 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"vs v2 {d['avg_a']-d['avg_b']:+8.1f}   rank {d['avg_rank_a']:.4f}\")"
}
run "efficiency-v2"
run "efficiency-v2?yaku_w=0"
run "efficiency-v2?yaku_w=1.5"
run "efficiency-v2?yaku_w=4"
run "efficiency-v2?dora_w=0"
run "efficiency-v2?dora_w=2"
run "efficiency-v2?yaku_w=0&dora_w=0"
echo "VALUEBONUS SWEEP DONE"
