#!/bin/bash
# Clean version: the loop is paused for the whole measurement and restarted at
# the end, so these numbers are comparable to each other.
cd ~/Documents/mmoyager/mmoyager_mahjong
for p in $(pgrep -f "loop.py") $(pgrep -f "mmj-selfplay generate"); do kill -9 $p 2>/dev/null; done
sleep 3
run() { # threads, batch
  printf "  threads=%-2s batch=%-5s " "$1" "$2"
  .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v12.bin \
    --out /tmp/kb.bin --max-records 300000 --epochs 1 --lr 3e-4 --batch-size "$2" \
    --threads "$1" --seed 1 --note kb 2>&1 | grep -oE "\(2[0-9]+ imitation.*, [0-9]+s\)" | tail -1
}
for r in 1 2; do
  echo "--- round $r ---"
  run 8 512
  run 4 512
  run 8 1024
  run 4 1024
done
nohup .venv/bin/python -u python/trainer/loop.py --resume --forever --dagger-loop --search-modes \
  --primary-spec efficiency-v2 --yardstick2 efficiency --eval-games 9600 --accept-margin 60 \
  --confirm-runs 2 --early-stop-slack 100 --bootstrap-lr 3e-4 --bootstrap-records 900000 \
  --bootstrap-epochs 4 --bootstrap-data data/selfplay/im-v12.bin --value-coef 0.25 \
  --entropy-coef 0.005 --max-records 900000 > data/logs/loop-v18.out 2>&1 &
echo "loop pid=$!"
echo "THREADS BENCH2 DONE"
