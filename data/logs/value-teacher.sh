#!/bin/bash
# Does a *learned* value improve the teacher? The push/fold decision is the one
# part of the teacher that was always a handcrafted heuristic, and the value head
# is the one learned signal the project has. Sweep the fold threshold in a strong
# field (baseline = the standard v2 against itself, which is pure noise, so its
# value calibrates the noise for this comparison).
export RAYON_NUM_THREADS=8
cd ~/Documents/mmoyager/mmoyager_mahjong
CK=data/checkpoints/ck-dagger.bin
run() {
  printf "%-46s " "$1"
  ./target/release/mmj-eval --a "$1" --b efficiency-v2 --games 4800 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"vs v2 {d['avg_a']-d['avg_b']:+8.1f}   rank {d['avg_rank_a']:.4f}\")"
}
run "efficiency-v2"
for thr in -0.5 0.0 0.5 1.0 1.5; do
  run "efficiency-v2?value=$CK&vthr=$thr"
done
echo "VALUE TEACHER SWEEP DONE"
