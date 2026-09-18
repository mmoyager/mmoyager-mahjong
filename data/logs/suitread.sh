#!/bin/bash
# Does reading the *order* of a threat's discards make the teacher stronger?
# Same-batch control: the plain v2 is measured alongside the variants.
export RAYON_NUM_THREADS=8
cd ~/Documents/mmoyager/mmoyager_mahjong
run() {
  printf "%-42s " "$1"
  ./target/release/mmj-eval --a "$1" --b efficiency-v2 --games 4800 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"vs v2 {d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
}
run "efficiency-v2"
run "efficiency-v2?suitread=1"
run "efficiency-v2?suitread=1&open=1"
run "efficiency-v2?open=1"
echo "SUITREAD DONE"
