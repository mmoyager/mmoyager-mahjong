#!/bin/bash
# Same question for the evaluation path: does the encoder change speed up eval too?
# 4800 paired games each, old then new, margins must be *identical* (the change is
# value-preserving), so this doubles as a second correctness check.
cd ~/Documents/mmoyager/mmoyager_mahjong
for tag in old new; do
  bin=./target/release/mmj-eval; [ $tag = new ] && bin=/tmp/mmj-perf/release/mmj-eval
  start=$(date +%s.%N)
  out=$("$bin" --a data/checkpoints/ck-ab.bin --b efficiency-v2 --games 4800 --seed 4242 --json)
  end=$(date +%s.%N)
  echo "$out" | python3 -c "
import json,sys
d=json.load(sys.stdin)
open('/tmp/eval-$tag.json','w').write(json.dumps(d))
print(f\"  $tag: {d['avg_a']-d['avg_b']:+.1f}  wall {float('$end')-float('$start'):.1f}s\")"
done
echo "EVAL THROUGHPUT DONE"
