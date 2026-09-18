#!/bin/bash
# Round 35: the student reproduces only 13.7% of the teacher's calls (against
# 99.7% for the calls the teacher declines) -- aggregate fidelity hides it because
# calls are 1.2% of decisions. Two questions follow:
#   1. is the aversion a class-imbalance artefact of the imitation loss?
#   2. if we remove it, does play get better or worse?
# Three replicas of the round-26 recipe with the call rows upweighted 10x, then
# the same diagnostics (call agreement, style) and the same batch of strength
# measurements as everything else.
cd ~/Documents/mmoyager/mmoyager_mahjong
E=./target/release/mmj-eval
for p in $(pgrep -f "loop.py") $(pgrep -f "train.py") $(pgrep -f "mmj-selfplay generate"); do
  kill -9 $p 2>/dev/null
done
sleep 3
for seed in 1 2 3; do
  echo "=== wc10 replica $seed ==="
  .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v12.bin \
    --hidden 768,768 --epochs 4 --lr 3e-4 --lr-decay 0.8 --max-records 900000 \
    --value-coef 0.5 --return-scale 0.25 --seed $seed --threads 8 --weight-calls 10 \
    --out /tmp/wc10-$seed.bin --note callweight10-$seed 2>&1 | grep -E "epoch 4|saved"
done
echo "=== diagnostics: call agreement ==="
for seed in 1 2 3; do
  echo "  --- wc10-$seed ---"
  .venv/bin/python python/trainer/diagnose_calls.py /tmp/wc10-$seed.bin data/selfplay/im-v12.bin 2>&1 | grep -E "call offered|teacher called|all rows"
done
echo "=== style (hanchan, four seats) ==="
for seed in 1 2 3; do
  printf "  wc10-%s " "$seed"
  ./target/release/mmj-selfplay stats --checkpoint /tmp/wc10-$seed.bin --games 800 --seed 11 --hanchan 2>/dev/null | python3 -c "
import sys, json
t = sys.stdin.read(); d = json.loads(t[t.index('{'):t.rindex('}')+1])
print(f\"call_rate {d['call_rate']:.4f}  riichi {d['riichi_rate']:.3f}  win {d['win_rate']:.3f}  deal_in {d['deal_in_rate']:.3f}  draw {d['draw_rate']:.3f}  avg_win {d['avg_win_points']:.0f}\")"
done
echo "=== strength: one batch (9600 games, seed 4242) ==="
printf "  %-12s " "ck-ab"
$E --a data/checkpoints/ck-ab.bin --b efficiency-v2 --games 9600 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"vs v2 {d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
for seed in 1 2 3; do
  printf "  %-12s " "wc10-$seed"
  $E --a /tmp/wc10-$seed.bin --b efficiency-v2 --games 9600 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"vs v2 {d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "=== direct: wc10 against the incumbent ==="
for seed in 1 2 3; do
  printf "  %-12s " "wc10-$seed vs ck-ab"
  $E --a /tmp/wc10-$seed.bin --b data/checkpoints/ck-ab.bin --games 9600 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"margin {d['avg_a']-d['avg_b']:+8.1f}  firsts {d['first_rate_a']:.4f}\")"
done
echo "=== restarting the loop ==="
nohup .venv/bin/python -u python/trainer/loop.py --resume --forever --dagger-loop --search-modes \
  --primary-spec efficiency-v2 --yardstick2 efficiency --eval-games 9600 --accept-margin 60 \
  --confirm-runs 2 --early-stop-slack 100 --bootstrap-lr 3e-4 --bootstrap-records 900000 \
  --bootstrap-epochs 4 --bootstrap-data data/selfplay/im-v12.bin --value-coef 0.25 \
  --entropy-coef 0.005 --max-records 400000 --threads 4 --accept-on direct --abs-every 5 \
  > data/logs/loop-v29.out 2>&1 &
echo "loop pid=$!"
echo "CALLWEIGHT DONE"
