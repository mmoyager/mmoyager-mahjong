#!/bin/bash
# Round 31: a new information channel. The network has always seen each
# opponent's discards as a *cumulative* count, so it cannot tell an early throw
# from one made after a riichi declaration -- the classical defence read. The new
# block exposes, for each opponent, how many discards they have made since
# declaring, split by suit, plus how many that is (and the same count for
# oneself). 737 -> 753 features.
#
# An older checkpoint still loads (its first layer has 737 inputs and the forward
# only reads that many), so the incumbent can *play* with the new encoder and
# generate labelled data that already carries the new block -- no from-scratch
# bootstrap model needed. That compatibility is checked first, because if it is
# wrong nothing downstream means anything.
cd ~/Documents/mmoyager/mmoyager_mahjong
OLD=./target/release/mmj-selfplay
NEW=/tmp/mmj-perf2/release/mmj-selfplay
OLDE=./target/release/mmj-eval
NEWE=/tmp/mmj-perf2/release/mmj-eval
for p in $(pgrep -f "loop.py") $(pgrep -f "train.py") $(pgrep -f "mmj-selfplay generate"); do
  kill -9 $p 2>/dev/null
done
sleep 3
echo "=== 1. compatibility: a 737-dim checkpoint under the 753-dim build ==="
for tag in old new; do
  e=$OLDE; [ "$tag" = new ] && e=$NEWE
  printf "  %-4s ck-ab vs v2, 2400 games: " "$tag"
  "$e" --a data/checkpoints/ck-ab.bin --b efficiency-v2 --games 2400 --seed 4242 --json | python3 -c "
import json,sys
d=json.load(sys.stdin)
print(f\"margin {d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "=== 2. self-play with the new encoder (incumbent plays, teacher labels) ==="
$NEW generate --games 6000 --out data/selfplay/im-v15.bin --checkpoint data/checkpoints/ck-ab.bin \
  --seats learner,learner,learner,learner --greedy --epsilon 0 --batch 256 --seed 6161 \
  --dagger --teacher-v2 --label-kind imitation
echo "=== 3. three from-scratch replicas on the new data (the round-26 recipe) ==="
for seed in 1 2 3; do
  echo "  --- replica $seed ---"
  .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v15.bin \
    --hidden 768,768 --epochs 4 --lr 3e-4 --lr-decay 0.8 --max-records 900000 \
    --value-coef 0.5 --return-scale 0.25 --seed $seed --threads 8 \
    --out /tmp/tr-$seed.bin --note threatread-$seed 2>&1 | grep -E "feature_dim|epoch 4|saved"
done
echo "=== 4. same-batch measurements (incumbent first) ==="
printf "  %-12s " "ck-ab(737)"
$NEWE --a data/checkpoints/ck-ab.bin --b efficiency-v2 --games 9600 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
for seed in 1 2 3; do
  printf "  %-12s " "tr-$seed(753)"
  $NEWE --a /tmp/tr-$seed.bin --b efficiency-v2 --games 9600 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "=== 5. direct: each new model against the incumbent ==="
for seed in 1 2 3; do
  printf "  %-12s " "tr-$seed vs ck-ab"
  $NEWE --a /tmp/tr-$seed.bin --b data/checkpoints/ck-ab.bin --games 9600 --seed 4242 --json | python3 -c "
import json,sys;d=json.load(sys.stdin);print(f\"margin {d['avg_a']-d['avg_b']:+8.1f}  firsts {d['first_rate_a']:.4f}\")"
done
echo "=== 6. restarting the loop ==="
nohup .venv/bin/python -u python/trainer/loop.py --resume --forever --dagger-loop --search-modes \
  --primary-spec efficiency-v2 --yardstick2 efficiency --eval-games 9600 --accept-margin 60 \
  --confirm-runs 2 --early-stop-slack 100 --bootstrap-lr 3e-4 --bootstrap-records 900000 \
  --bootstrap-epochs 4 --bootstrap-data data/selfplay/im-v12.bin --value-coef 0.25 \
  --entropy-coef 0.005 --max-records 900000 --threads 4 --accept-on direct --abs-every 5 \
  > data/logs/loop-v24.out 2>&1 &
echo "loop pid=$!"
echo "THREATREAD DONE"
