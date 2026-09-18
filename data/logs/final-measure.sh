#!/bin/bash
# Round 34, second track: tighten the numbers the user actually reads. The
# reported strength so far rests on two seeds against the strong bot and one
# against the weak one; this adds two more seeds each, plus a refresh of the
# style statistics against the Tenhou reference from round 24.
cd ~/Documents/mmoyager/mmoyager_mahjong
E=./target/release/mmj-eval
echo "=== ck-ab vs efficiency-v2 (the stronger bot), 9600 games ==="
for seed in 4242 5150 31337 777; do
  printf "  seed %-6s " "$seed"
  $E --a data/checkpoints/ck-ab.bin --b efficiency-v2 --games 9600 --seed $seed --json | python3 -c "
import json,sys;d=json.load(sys.stdin)
print(f\"margin {d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}  firsts {d['first_rate_a']:.4f}\")"
done
echo "=== ck-ab vs efficiency (the frozen baseline), 4800 games ==="
for seed in 4242 5150 31337; do
  printf "  seed %-6s " "$seed"
  $E --a data/checkpoints/ck-ab.bin --b efficiency --games 4800 --seed $seed --json | python3 -c "
import json,sys;d=json.load(sys.stdin)
print(f\"margin {d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "=== style statistics (hanchan, teacher opponents) ==="
./target/release/mmj-selfplay stats --checkpoint data/checkpoints/ck-ab.bin --games 1500 --seed 11 --hanchan 2>&1 | tail -12
echo "FINAL MEASURE DONE"
