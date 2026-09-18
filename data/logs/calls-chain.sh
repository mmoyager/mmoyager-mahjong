#!/bin/bash
cd ~/Documents/mmoyager/mmoyager_mahjong
while pgrep -f "region-chain.sh" > /dev/null; do sleep 15; done
CK=data/checkpoints/ck-best.bin
echo "=== generating call-window labels ==="
./target/release/mmj-selfplay generate --games 6000 --out /tmp/dagger-calls.bin \
  --checkpoint "$CK" --seats learner,learner,learner,learner \
  --greedy --epsilon 0 --batch 256 --seed 7778 \
  --dagger --dagger-calls --teacher-v2 --label-kind imitation
echo "=== fine-tuning on call windows only ==="
.venv/bin/python python/trainer/train.py --data /tmp/dagger-calls.bin \
  --init "$CK" --out /tmp/ck-calls.bin --epochs 2 --lr 5e-5 \
  --entropy-coef 0.005 --value-coef 0.25 --return-scale 0.25 \
  --max-records 900000 --note call-region-finetune
echo "--- calls-tuned vs ck-best"
./target/release/mmj-eval --a /tmp/ck-calls.bin --b "$CK" --games 4800 --seed 5150 --json
echo "--- calls-tuned vs frozen baseline"
./target/release/mmj-eval --a /tmp/ck-calls.bin --b efficiency --games 4800 --seed 4242 --json
echo "CALLS CHAIN DONE"
