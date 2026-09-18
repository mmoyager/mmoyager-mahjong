#!/bin/bash
# Round 34: the training step is the loop's largest phase (375 s of a 780 s
# iteration) and is a pure MLP forward/backward. Inductor should be able to fuse
# it. Alternating arms on a fixed slice, so the background loop's load cancels.
cd ~/Documents/mmoyager/mmoyager_mahjong
for round in 1 2; do
  for arm in eager compile; do
    extra=""; [ "$arm" = compile ] && extra="--compile"
    printf "  %-8s round %s: " "$arm" "$round"
    s=$(date +%s)
    .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v12.bin \
      --out /tmp/cmp-$arm.bin --hidden 768,768 --epochs 1 --lr 3e-4 --max-records 300000 \
      --seed 1 --threads 4 $extra 2>&1 | grep -oE "\(2[0-9]+ imitation.*, [0-9]+s\)" | tail -1
    e=$(date +%s)
    echo "      (wall $((e-s))s)"
  done
done
echo "COMPILE AB DONE"
