#!/bin/bash
# Refinements of the sequence read that round 22 proved valuable (+136 for the
# teacher, +198 for the student). Same-batch control, 9600 games.
export RAYON_NUM_THREADS=8
cd ~/Documents/mmoyager/mmoyager_mahjong
run() {
  printf "%-52s " "$1"
  ./target/release/mmj-eval --a "$1" --b efficiency-v2 --games 9600 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"vs v2 {d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
}
run "efficiency-v2"
run "efficiency-v2?suitread=1"
run "efficiency-v2?suitread=1&readtime=1"
run "efficiency-v2?suitread=1&readmeld=1"
run "efficiency-v2?suitread=1&readtime=1&readmeld=1"
echo "READS2 DONE"
