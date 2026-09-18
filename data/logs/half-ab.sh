#!/bin/bash
# Round 28: half-precision weights. The forward is bound by weight bytes, so
# halving the width of every weight is the one lever that does not shrink the
# network. Older binary = f32 weights, newer = binary16 weights (F16C kernel).
# The loop is paused for the duration so the numbers mean something, and
# restarted at the end. Everything is interleaved old/new.
cd ~/Documents/mmoyager/mmoyager_mahjong
OLD=./target/release/mmj-selfplay
NEW=/tmp/mmj-perf2/release/mmj-selfplay
OLDE=./target/release/mmj-eval
NEWE=/tmp/mmj-perf2/release/mmj-eval
for p in $(pgrep -f "loop.py") $(pgrep -f "train.py") $(pgrep -f "mmj-selfplay generate"); do
  kill -9 $p 2>/dev/null
done
sleep 3
echo "=== forward cost (interleaved, 4 rounds) ==="
for i in 1 2 3 4; do
  printf "  f32  %s\n" "$($OLD bench --checkpoint data/checkpoints/ck-ab.bin | sed -n 2p)"
  printf "  half %s\n" "$($NEW bench --checkpoint data/checkpoints/ck-ab.bin | sed -n 2p)"
done
echo "=== generation throughput (1200 games, interleaved) ==="
for seed in 4242 5150; do
  for tag in old new; do
    bin=$OLD; [ $tag = new ] && bin=$NEW
    printf "  %-4s seed %s: " $tag $seed
    $bin generate --games 1200 --out /tmp/h-$tag-$seed.bin --checkpoint data/checkpoints/ck-ab.bin \
      --seats learner,learner,learner,learner --greedy --epsilon 0 --batch 256 --seed $seed \
      --dagger --teacher-v2 --label-kind imitation 2>&1 | tail -1
  done
done
echo "=== play strength: ck-ab vs efficiency-v2, 4800 paired games, seed 4242 ==="
for tag in old new; do
  e=$OLDE; [ $tag = new ] && e=$NEWE
  printf "  %-4s " $tag
  $e --a data/checkpoints/ck-ab.bin --b efficiency-v2 --games 4800 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"margin {d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "=== restarting the loop ==="
nohup .venv/bin/python -u python/trainer/loop.py --resume --forever --dagger-loop --search-modes \
  --primary-spec efficiency-v2 --yardstick2 efficiency --eval-games 9600 --accept-margin 60 \
  --confirm-runs 2 --early-stop-slack 100 --bootstrap-lr 3e-4 --bootstrap-records 900000 \
  --bootstrap-epochs 4 --bootstrap-data data/selfplay/im-v12.bin --value-coef 0.25 \
  --entropy-coef 0.005 --max-records 900000 > data/logs/loop-v16.out 2>&1 &
echo "loop pid=$!"
echo "HALF AB DONE"
