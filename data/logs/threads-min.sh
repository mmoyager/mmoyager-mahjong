#!/bin/bash
# Careful version of the thread measurement: alternating arms, four repeats
# each, reporting every sample so the min (the robust statistic for throughput)
# and the spread are both visible. Runs alongside the loop, so the ratio is the
# meaningful number.
cd ~/Documents/mmoyager/mmoyager_mahjong
for r in 1 2 3 4; do
  for t in 8 4; do
    s=$(date +%s.%N)
    .venv/bin/python -u python/trainer/train.py --data data/selfplay/im-v12.bin \
      --out /tmp/kb.bin --max-records 300000 --epochs 1 --lr 3e-4 --batch-size 512 \
      --threads $t --seed 1 --note kb > /tmp/kb.out 2>&1
    e=$(date +%s.%N)
    printf "  round %s threads=%s wall %.1fs  |" $r $t "$(python3 -c "print($e-$s)")"
  done
  echo
done
echo "THREADS MIN DONE"
