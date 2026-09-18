#!/bin/bash
# Does the reinforcement-learning half of the training mixture help?
# Both runs start from the same checkpoint with identical hyper-parameters;
# only the data mixture differs.
set -e
cd ~/Documents/mmoyager/mmoyager_mahjong
P=.venv/bin/python
T=python/trainer/train.py
COMMON="--init data/checkpoints/ck-best.bin --epochs 2 --lr 1e-4 --entropy-coef 0.005 \
  --value-coef 0.25 --return-scale 0.25 --ppo --clip 0.2 --kl-coef 0.5 --max-records 900000"
echo "=== A: pure imitation (teacher labels only) ==="
$P $T --data data/selfplay/im-v9.bin --out /tmp/rl-abl-A.bin $COMMON --note rl-ablation-imitation-only
echo "=== B: the loop's mixture (imitation + self-play RL) ==="
$P $T --data data/selfplay/im-v9.bin data/selfplay/sp-0017.bin data/selfplay/sp-0017-mixed.bin \
  --out /tmp/rl-abl-B.bin $COMMON --note rl-ablation-loop-mixture
echo "TRAINING DONE"
