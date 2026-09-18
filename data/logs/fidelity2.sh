#!/bin/bash
# Fidelity gradient from *data volume*, using the existing 3.44M-row imitation
# set: same recipe, same epochs, 900k vs 3.44M rows. Each model is then measured
# with (a) a uniform top-1 agreement score on identical held-out rows and (b) the
# paired margin against both the frozen v1 baseline and the stronger v2 bot.
cd ~/Documents/mmoyager/mmoyager_mahjong
P=.venv/bin/python
for cfg in "a 900000 4" "b 3400000 4"; do
  set -- $cfg
  echo "=== hf-$1: max-records $2, epochs $3 ==="
  $P -u python/trainer/train.py --data data/selfplay/im-v9.bin --out /tmp/hf-$1.bin \
    --hidden 768,768 --epochs $3 --lr 1e-3 --lr-decay 0.8 --max-records $2 \
    --value-coef 0.5 --return-scale 0.25 --threads 6 --note hf-$1 2>&1 | grep -E "epoch |val_acc|error"
done
echo "FIDELITY2 DONE"
