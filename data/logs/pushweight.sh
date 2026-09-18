#!/bin/bash
# Round 37: the diagnostic said the model is NOT more conservative than the
# teacher -- when the teacher pushed, the model pushed too 93% of the time and
# its overall dangerous-discard rate is 0.054 against the teacher's 0.058. What
# differs is *which* risky tile (14% of those rows disagree). This arm upweights
# exactly those rows 10x, asking whether copying the teacher's choice in
# threatened positions is worth more weight than the baseline gives it. Round 35
# (same treatment on the call labels) is the precedent: if the student's own
# choice there is better, this should hurt.
cd ~/Documents/mmoyager/mmoyager_mahjong
E=./target/release/mmj-eval
for p in $(pgrep -f "loop.py") $(pgrep -f "train.py") $(pgrep -f "mmj-selfplay generate"); do
  kill -9 $p 2>/dev/null
done
sleep 3
for seed in 1 2 3; do
  echo "=== push10 replica $seed ==="
  .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v12.bin \
    --hidden 768,768 --epochs 4 --lr 3e-4 --lr-decay 0.8 --max-records 900000 \
    --value-coef 0.5 --return-scale 0.25 --seed $seed --threads 8 \
    --weight-push 10 --push-threshold 0.3 \
    --out /tmp/pw-$seed.bin --note pushweight10-$seed 2>&1 | grep -E "epoch 4|saved"
done
echo "=== diagnostics: dangerous-discard agreement ==="
for seed in 1 2 3; do
  echo "  --- pw-$seed ---"
  .venv/bin/python python/trainer/diagnose_calls.py /tmp/pw-$seed.bin data/selfplay/im-v12.bin 2>&1 | grep -E "safe|dangerous|all rows"
done
echo "=== strength: one batch (9600 games, seed 4242) ==="
printf "  %-10s " "ck-ab"
$E --a data/checkpoints/ck-ab.bin --b efficiency-v2 --games 9600 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"vs v2 {d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
for seed in 1 2 3; do
  printf "  %-10s " "pw-$seed"
  $E --a /tmp/pw-$seed.bin --b efficiency-v2 --games 9600 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"vs v2 {d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "=== direct: against the incumbent ==="
for seed in 1 2 3; do
  printf "  %-10s " "pw-$seed vs ck-ab"
  $E --a /tmp/pw-$seed.bin --b data/checkpoints/ck-ab.bin --games 9600 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"margin {d['avg_a']-d['avg_b']:+8.1f}  firsts {d['first_rate_a']:.4f}\")"
done
echo "=== style (one replica) ==="
./target/release/mmj-selfplay stats --checkpoint /tmp/pw-1.bin --games 800 --seed 11 --hanchan 2>/dev/null | python3 -c "
import sys, json
t = sys.stdin.read(); d = json.loads(t[t.index('{'):t.rindex('}')+1])
print(f\"  pw-1 call_rate {d['call_rate']:.4f}  riichi {d['riichi_rate']:.3f}  win {d['win_rate']:.3f}  deal_in {d['deal_in_rate']:.3f}  draw {d['draw_rate']:.3f}  avg_win {d['avg_win_points']:.0f}\")"
echo "=== restarting the loop ==="
nohup .venv/bin/python -u python/trainer/loop.py --resume --forever --dagger-loop --search-modes \
  --primary-spec efficiency-v2 --yardstick2 efficiency --eval-games 9600 --accept-margin 60 \
  --confirm-runs 2 --early-stop-slack 100 --bootstrap-lr 3e-4 --bootstrap-records 900000 \
  --bootstrap-epochs 4 --bootstrap-data data/selfplay/im-v12.bin --value-coef 0.25 \
  --entropy-coef 0.005 --max-records 400000 --threads 4 --accept-on direct --abs-every 5 \
  > data/logs/loop-v31.out 2>&1 &
echo "loop pid=$!"
echo "PUSHWEIGHT DONE"
