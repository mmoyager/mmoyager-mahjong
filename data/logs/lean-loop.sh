#!/bin/bash
# Round 34: make the loop's iterations cheaper on evidence rather than on hope.
#
# Iteration 51 (6000-game recipe) took 777.9 s: self-play ~175 s, training 2 epochs
# on 2.29M decisions ~375 s, evaluation ~230 s. Rounds 26, 28 and 33 all say that
# fitting harder buys nothing (3.4M rows was *worse* than 900k; 8 epochs no better
# than 4; +54% capacity no better) -- so the training step is paying for something
# the measurements say is worthless. This run halves the per-file record cap
# (900k -> 400k, i.e. ~1.2M decisions per iteration instead of ~2.4M) and measures
# the iteration time and the candidate scores against the previous iterations.
cd ~/Documents/mmoyager/mmoyager_mahjong
for p in $(pgrep -f "loop.py") $(pgrep -f "train.py") $(pgrep -f "mmj-selfplay generate"); do
  kill -9 $p 2>/dev/null
done
sleep 3
nohup .venv/bin/python -u python/trainer/loop.py --resume --forever --dagger-loop --search-modes \
  --primary-spec efficiency-v2 --yardstick2 efficiency --eval-games 9600 --accept-margin 60 \
  --confirm-runs 2 --early-stop-slack 100 --bootstrap-lr 3e-4 --bootstrap-records 900000 \
  --bootstrap-epochs 4 --bootstrap-data data/selfplay/im-v12.bin --value-coef 0.25 \
  --entropy-coef 0.005 --max-records 400000 --threads 4 --accept-on direct --abs-every 5 \
  > data/logs/loop-v27.out 2>&1 &
echo "loop pid=$! (max-records 400000)"
echo "LEAN LOOP STARTED"
