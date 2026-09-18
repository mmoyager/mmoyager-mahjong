#!/bin/bash
# Round 29: `is_agari` used to try every pair position and recurse into the set
# search; it now reads four cached decomposition masks and combines them with a
# bitmask sum. Same answer (verified against the recursion on 17k hands), much
# less work. Old = ./target/release (f16 + old win check), new = /tmp/mmj-perf2.
# The loop is paused for the measurement and restarted at the end.
cd ~/Documents/mmoyager/mmoyager_mahjong
OLD=./target/release/mmj-selfplay
NEW=/tmp/mmj-perf2/release/mmj-selfplay
OLDE=./target/release/mmj-eval
NEWE=/tmp/mmj-perf2/release/mmj-eval
for p in $(pgrep -f "loop.py") $(pgrep -f "train.py") $(pgrep -f "mmj-selfplay generate"); do
  kill -9 $p 2>/dev/null
done
sleep 3
echo "=== generation 1200 games, interleaved ==="
for seed in 4242 5150; do
  for tag in old new; do
    bin=$OLD; [ $tag = new ] && bin=$NEW
    printf "  %-4s seed %s: " $tag $seed
    $bin generate --games 1200 --out /tmp/ag-$tag-$seed.bin --checkpoint data/checkpoints/ck-ab.bin \
      --seats learner,learner,learner,learner --greedy --epsilon 0 --batch 256 --seed $seed \
      --dagger --teacher-v2 --label-kind imitation 2>&1 | tail -1
  done
done
echo "=== evaluation 2400 paired games, interleaved ==="
for round in 1 2; do
  for tag in old new; do
    e=$OLDE; [ $tag = new ] && e=$NEWE
    start=$(date +%s)
    "$e" --a data/checkpoints/ck-ab.bin --b efficiency-v2 --games 2400 --seed 4242 --json > /tmp/ag-ev-$tag.json
    end=$(date +%s)
    echo "  $tag round $round: wall $((end-start))s  margin $(python3 -c "import json;d=json.load(open('/tmp/ag-ev-$tag.json'));print(f\"{d['avg_a']-d['avg_b']:+.1f}\")")"
  done
done
echo "=== restarting the loop ==="
nohup .venv/bin/python -u python/trainer/loop.py --resume --forever --dagger-loop --search-modes \
  --primary-spec efficiency-v2 --yardstick2 efficiency --eval-games 9600 --accept-margin 60 \
  --confirm-runs 2 --early-stop-slack 100 --bootstrap-lr 3e-4 --bootstrap-records 900000 \
  --bootstrap-epochs 4 --bootstrap-data data/selfplay/im-v12.bin --value-coef 0.25 \
  --entropy-coef 0.005 --max-records 900000 --threads 4 > data/logs/loop-v20.out 2>&1 &
echo "loop pid=$!"
echo "AGARI AB DONE"
