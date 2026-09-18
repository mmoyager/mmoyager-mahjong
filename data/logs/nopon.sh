#!/bin/bash
# Round 36: round 35 showed that *forcing* the teacher's pon labels costs ~1700
# points, and the per-class split shows pon is the only label class the model
# fails to reproduce (0.137 recall against 0.89-1.00 for every other class).
# If those labels are actively harmful, removing them from the imitation loss
# entirely (-weight-calls 0) should be at least neutral and possibly better than
# the baseline, which still spends 1.15% of its gradient on them.
cd ~/Documents/mmoyager/mmoyager_mahjong
E=./target/release/mmj-eval
for p in $(pgrep -f "loop.py") $(pgrep -f "train.py") $(pgrep -f "mmj-selfplay generate"); do
  kill -9 $p 2>/dev/null
done
sleep 3
for seed in 1 2 3; do
  echo "=== nopon replica $seed (call rows dropped from the loss) ==="
  .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v12.bin \
    --hidden 768,768 --epochs 4 --lr 3e-4 --lr-decay 0.8 --max-records 900000 \
    --value-coef 0.5 --return-scale 0.25 --seed $seed --threads 8 --weight-calls 0 \
    --out /tmp/np-$seed.bin --note nopon-$seed 2>&1 | grep -E "epoch 4|saved"
done
echo "=== diagnostics ==="
for seed in 1 2 3; do
  echo "  --- np-$seed ---"
  .venv/bin/python python/trainer/diagnose_calls.py /tmp/np-$seed.bin data/selfplay/im-v12.bin 2>&1 | grep -E "all rows|teacher called|per label|pon "
done
echo "=== style (hanchan, four seats) ==="
for seed in 1 2 3; do
  printf "  np-%s " "$seed"
  ./target/release/mmj-selfplay stats --checkpoint /tmp/np-$seed.bin --games 800 --seed 11 --hanchan 2>/dev/null | python3 -c "
import sys, json
t = sys.stdin.read(); d = json.loads(t[t.index('{'):t.rindex('}')+1])
print(f\"call_rate {d['call_rate']:.4f}  riichi {d['riichi_rate']:.3f}  win {d['win_rate']:.3f}  deal_in {d['deal_in_rate']:.3f}  draw {d['draw_rate']:.3f}  avg_win {d['avg_win_points']:.0f}\")"
done
echo "=== strength: one batch (9600 games, seed 4242) ==="
printf "  %-10s " "ck-ab"
$E --a data/checkpoints/ck-ab.bin --b efficiency-v2 --games 9600 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"vs v2 {d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
for seed in 1 2 3; do
  printf "  %-10s " "np-$seed"
  $E --a /tmp/np-$seed.bin --b efficiency-v2 --games 9600 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"vs v2 {d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "=== direct: against the incumbent ==="
for seed in 1 2 3; do
  printf "  %-10s " "np-$seed vs ck-ab"
  $E --a /tmp/np-$seed.bin --b data/checkpoints/ck-ab.bin --games 9600 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"margin {d['avg_a']-d['avg_b']:+8.1f}  firsts {d['first_rate_a']:.4f}\")"
done
echo "=== restarting the loop ==="
nohup .venv/bin/python -u python/trainer/loop.py --resume --forever --dagger-loop --search-modes \
  --primary-spec efficiency-v2 --yardstick2 efficiency --eval-games 9600 --accept-margin 60 \
  --confirm-runs 2 --early-stop-slack 100 --bootstrap-lr 3e-4 --bootstrap-records 900000 \
  --bootstrap-epochs 4 --bootstrap-data data/selfplay/im-v12.bin --value-coef 0.25 \
  --entropy-coef 0.005 --max-records 400000 --threads 4 --accept-on direct --abs-every 5 \
  > data/logs/loop-v30.out 2>&1 &
echo "loop pid=$!"
echo "NOPON DONE"
