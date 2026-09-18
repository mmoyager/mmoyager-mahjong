#!/bin/bash
# Eval-path counterpart of the half-precision A/B: 2400 paired games each,
# interleaved, so the background loop's load cancels out (same interval).
cd ~/Documents/mmoyager/mmoyager_mahjong
for round in 1 2; do
  for tag in f32 half; do
    e=/tmp/mmj-perf/release/mmj-eval
    [ "$tag" = half ] && e=./target/release/mmj-eval
    start=$(date +%s)
    ./"$e" --a data/checkpoints/ck-ab.bin --b efficiency-v2 --games 2400 --seed 4242 --json > /tmp/ev-$tag.json 2>/dev/null \
      || "$e" --a data/checkpoints/ck-ab.bin --b efficiency-v2 --games 2400 --seed 4242 --json > /tmp/ev-$tag.json
    end=$(date +%s)
    margin=$(python3 -c "import json;d=json.load(open('/tmp/ev-$tag.json'));print(f\"{d['avg_a']-d['avg_b']:+.1f}\")")
    echo "  $tag round $round: margin $margin  wall $((end-start))s"
  done
done
echo "EVAL HALF DONE"
