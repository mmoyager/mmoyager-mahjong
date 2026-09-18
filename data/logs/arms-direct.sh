#!/bin/bash
# Round 31, the control comparison, done directly: the feature arm and the
# zero-block control arm are the same data, same recipe, same seeds -- only the
# new block differs. Playing them against each other at the same tables is the
# cleanest available test of whether the block carries anything.
cd ~/Documents/mmoyager/mmoyager_mahjong
E=/tmp/mmj-perf2/release/mmj-eval
for seed in 1 2 3; do
  printf "  feature-$seed vs control-$seed (9600 games): "
  $E --a /tmp/tr-$seed.bin --b /tmp/ctl-$seed.bin --games 9600 --seed 4242 --json | python3 -c "
import json,sys
d=json.load(sys.stdin)
print(f\"margin {d['avg_a']-d['avg_b']:+8.1f}  firsts {d['first_rate_a']:.4f}  rankA {d['avg_rank_a']:.4f}\")"
done
echo "ARMS DIRECT DONE"
