#!/bin/bash
# Round 33: is the plateau "information-limited" or "capacity-limited"? Every
# experiment so far that added information was null, and the only architecture
# test ever run was a *narrower* net (512 wide, -195 paired). A wider net asks the
# complementary question on exactly the same data, recipe and seeds as ck-ab:
# if a 1024x1024 body extracts more from the same 737 features, the ceiling is
# capacity; if it does not, the "information-limited" reading gets much stronger.
#
# Cost: a wider body is ~1.55x the parameters, so this is only worth adopting if
# it buys clearly more than the ~20% it would add to every inference.
cd ~/Documents/mmoyager/mmoyager_mahjong
E=./target/release/mmj-eval
for p in $(pgrep -f "loop.py") $(pgrep -f "train.py") $(pgrep -f "mmj-selfplay generate"); do
  kill -9 $p 2>/dev/null
done
sleep 3
for seed in 1 2 3; do
  echo "=== wide replica $seed ==="
  .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v12.bin \
    --hidden 1024,1024 --epochs 4 --lr 3e-4 --lr-decay 0.8 --max-records 900000 \
    --value-coef 0.5 --return-scale 0.25 --seed $seed --threads 8 \
    --out /tmp/wide-$seed.bin --note wide1024-$seed 2>&1 | grep -E "hidden|parameters|epoch 4|saved"
done
echo "=== same batch (9600 games, seed 4242): incumbent first ==="
printf "  %-12s " "ck-ab(768)"
$E --a data/checkpoints/ck-ab.bin --b efficiency-v2 --games 9600 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"vs v2 {d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
for seed in 1 2 3; do
  printf "  %-12s " "wide-$seed"
  $E --a /tmp/wide-$seed.bin --b efficiency-v2 --games 9600 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"vs v2 {d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "=== direct: wide models against the incumbent (9600 games) ==="
for seed in 1 2 3; do
  printf "  %-12s " "wide-$seed vs ck-ab"
  $E --a /tmp/wide-$seed.bin --b data/checkpoints/ck-ab.bin --games 9600 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"margin {d['avg_a']-d['avg_b']:+8.1f}  firsts {d['first_rate_a']:.4f}\")"
done
echo "=== restarting the loop ==="
nohup .venv/bin/python -u python/trainer/loop.py --resume --forever --dagger-loop --search-modes \
  --primary-spec efficiency-v2 --yardstick2 efficiency --eval-games 9600 --accept-margin 60 \
  --confirm-runs 2 --early-stop-slack 100 --bootstrap-lr 3e-4 --bootstrap-records 900000 \
  --bootstrap-epochs 4 --bootstrap-data data/selfplay/im-v12.bin --value-coef 0.25 \
  --entropy-coef 0.005 --max-records 900000 --threads 4 --accept-on direct --abs-every 5 \
  > data/logs/loop-v26.out 2>&1 &
echo "loop pid=$!"
echo "WIDER DONE"
