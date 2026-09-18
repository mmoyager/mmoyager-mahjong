#!/bin/bash
# Does imitation fidelity actually buy strength against a *strong* opponent?
# The project concluded "higher accuracy does not mean stronger" in round 3, but
# every measurement behind that conclusion used the frozen v1 baseline, which
# round 13 showed rewards a specialised style. Here two models are trained on the
# same recipe with different data volumes, and both are measured against v1 and
# against the much stronger v2 rule bot.
cd ~/Documents/mmoyager/mmoyager_mahjong
P=.venv/bin/python
T=python/trainer/train.py
echo "=== hf-a: 2.4M records x 4 epochs ==="
$P -u $T --data data/selfplay/im-v10.bin --out /tmp/hf-a.bin --hidden 768,768 \
  --epochs 4 --lr 1e-3 --lr-decay 0.8 --max-records 2400000 --value-coef 0.5 \
  --return-scale 0.25 --threads 6 --note hf-a-2.4M 2>&1 | grep -E "epoch 4|error"
echo "=== hf-b: 6.9M records x 2 epochs ==="
$P -u $T --data data/selfplay/im-v10.bin --out /tmp/hf-b.bin --hidden 768,768 \
  --epochs 2 --lr 1e-3 --lr-decay 0.8 --max-records 6900000 --value-coef 0.5 \
  --return-scale 0.25 --threads 6 --note hf-b-6.9M 2>&1 | grep -E "epoch 2|error"
echo "=== margins: v1 then v2 ==="
for ck in /tmp/hf-a.bin /tmp/hf-b.bin; do
  for opp in efficiency efficiency-v2; do
    printf "%-14s vs %-14s " "$(basename $ck)" "$opp"
    ./target/release/mmj-eval --a "$ck" --b "$opp" --games 4800 --seed 4242 --json | \
      $P -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}\")"
  done
done
echo "FIDELITY DONE"
