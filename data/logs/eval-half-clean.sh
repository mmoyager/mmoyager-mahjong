#!/bin/bash
# Clean eval-path timing: the loop is paused so the two binaries see the same
# machine, then restarted. Two interleaved pairs of 2400 paired games.
cd ~/Documents/mmoyager/mmoyager_mahjong
for p in $(pgrep -f "loop.py") $(pgrep -f "train.py") $(pgrep -f "mmj-selfplay generate"); do kill -9 $p 2>/dev/null; done
sleep 3
for round in 1 2; do
  for tag in f32 half; do
    e=/tmp/mmj-perf/release/mmj-eval
    [ "$tag" = half ] && e=./target/release/mmj-eval
    start=$(date +%s)
    "$e" --a data/checkpoints/ck-ab.bin --b efficiency-v2 --games 2400 --seed 4242 --json > /tmp/evc-$tag.json
    end=$(date +%s)
    echo "  $tag round $round: wall $((end-start))s  margin $(python3 -c "import json;d=json.load(open('/tmp/evc-$tag.json'));print(f\"{d['avg_a']-d['avg_b']:+.1f}\")")"
  done
done
nohup .venv/bin/python -u python/trainer/loop.py --resume --forever --dagger-loop --search-modes \
  --primary-spec efficiency-v2 --yardstick2 efficiency --eval-games 9600 --accept-margin 60 \
  --confirm-runs 2 --early-stop-slack 100 --bootstrap-lr 3e-4 --bootstrap-records 900000 \
  --bootstrap-epochs 4 --bootstrap-data data/selfplay/im-v12.bin --value-coef 0.25 \
  --entropy-coef 0.005 --max-records 900000 > data/logs/loop-v17.out 2>&1 &
echo "loop pid=$!"
echo "EVAL HALF CLEAN DONE"
