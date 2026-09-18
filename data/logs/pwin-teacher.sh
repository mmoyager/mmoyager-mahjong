#!/bin/bash
# Round 19 measured the `p_win` head at AUC 0.787 (against a base rate of 0.186),
# the one reliable learned signal the project has, while the scalar value head
# explains only 14% of the hand swing and failed completely as a fold criterion.
# This sweeps "fold when the chance of completing the hand is below X" in a
# strong field.
export RAYON_NUM_THREADS=8
cd ~/Documents/mmoyager/mmoyager_mahjong
CK=/tmp/ck-dec.bin
run() {
  printf "%-52s " "$1"
  ./target/release/mmj-eval --a "$1" --b efficiency-v2 --games 4800 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"vs v2 {d['avg_a']-d['avg_b']:+8.1f}   rank {d['avg_rank_a']:.4f}\")"
}
run "efficiency-v2"
for p in 0.05 0.10 0.15 0.20 0.30; do
  run "efficiency-v2?value=$CK&pwin=$p"
done
echo "PWIN SWEEP DONE"
