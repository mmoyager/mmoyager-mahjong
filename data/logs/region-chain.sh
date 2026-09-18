#!/bin/bash
# After the RL ablation finishes: can the mid-game deficit be learned away?
# Generate teacher labels *only* on the region the assisted-play experiment
# showed is still losing points (own turn, 2+ shanten), then fine-tune on it.
cd ~/Documents/mmoyager/mmoyager_mahjong
while pgrep -f "rl-ablation.sh" > /dev/null; do sleep 15; done
CK=data/checkpoints/ck-best.bin
echo "=== generating region labels ==="
./target/release/mmj-selfplay generate --games 6000 --out /tmp/dagger-sh2.bin \
  --checkpoint "$CK" --seats learner,learner,learner,learner \
  --greedy --epsilon 0 --batch 256 --seed 7777 \
  --dagger --dagger-shanten 2 --teacher-v2 --label-kind imitation
echo "=== fine-tuning on the region only ==="
.venv/bin/python python/trainer/train.py --data /tmp/dagger-sh2.bin \
  --init "$CK" --out /tmp/ck-region.bin --epochs 2 --lr 5e-5 \
  --entropy-coef 0.005 --value-coef 0.25 --return-scale 0.25 \
  --max-records 900000 --note region-finetune-sh2
echo "=== evals ==="
echo "--- A (imitation only) vs B (loop mixture)"
./target/release/mmj-eval --a /tmp/rl-abl-A.bin --b /tmp/rl-abl-B.bin --games 4800 --seed 5150 --json
echo "--- A vs frozen baseline"
./target/release/mmj-eval --a /tmp/rl-abl-A.bin --b efficiency --games 4800 --seed 4242 --json
echo "--- B vs frozen baseline"
./target/release/mmj-eval --a /tmp/rl-abl-B.bin --b efficiency --games 4800 --seed 4242 --json
echo "--- region-tuned vs ck-best"
./target/release/mmj-eval --a /tmp/ck-region.bin --b "$CK" --games 4800 --seed 5150 --json
echo "--- region-tuned vs frozen baseline"
./target/release/mmj-eval --a /tmp/ck-region.bin --b efficiency --games 4800 --seed 4242 --json
echo "REGION CHAIN DONE"
