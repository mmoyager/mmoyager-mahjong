#!/bin/bash
# The teacher's calling rule is the crudest part of it (`call_eagerness` gates
# chi, plus a "do not open a hand without a yaku path" check), and the student's
# self-play call rate is the largest remaining gap to its teacher. Re-tune the
# parameter in a *strong* field, where the metric now points.
export RAYON_NUM_THREADS=4
cd ~/Documents/mmoyager/mmoyager_mahjong
for eager in 0 1 2; do
  printf "eager=%s  " "$eager"
  ./target/release/mmj-eval --a "efficiency-v2?eager=$eager" --b efficiency-v2 \
    --games 4800 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"vs v2 {d['avg_a']-d['avg_b']:+8.1f}   rank {d['avg_rank_a']:.4f}\")"
done
echo "EAGER SWEEP DONE"
