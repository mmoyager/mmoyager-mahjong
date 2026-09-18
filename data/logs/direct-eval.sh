#!/bin/bash
# Round 30: `mmj-eval` can already put a *checkpoint* on either side, so the loop
# does not have to compare two separate margins against a third party -- it can
# measure the candidate against the incumbent at the same tables. That is a
# paired comparison on identical deals, so it should be far less noisy. The
# indirect numbers for the same pair (both measured in the loop's own batch
# against efficiency-v2) were candidate 25044.8 and incumbent 25153.6, i.e. an
# implied difference of -108.8. This measures the difference directly, on two
# seeds, to check both its value and its spread.
cd ~/Documents/mmoyager/mmoyager_mahjong
for seed in 4242 5150; do
  printf "  direct ck-0049 vs ck-ab, seed %s: " $seed
  ./target/release/mmj-eval --a data/checkpoints/ck-0049.bin --b data/checkpoints/ck-ab.bin \
    --games 9600 --seed $seed --json | python3 -c "
import json,sys
d=json.load(sys.stdin)
print(f\"margin {d['avg_a']-d['avg_b']:+8.1f}  rankA {d['avg_rank_a']:.4f}  firsts {d['first_rate_a']:.4f}\")"
done
echo "DIRECT EVAL DONE"
