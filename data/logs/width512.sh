#!/bin/bash
# Throughput experiment: the inference kernel is memory-bandwidth-bound
# (~24 GB/s of weight traffic), so width is close to a linear cost. The project
# once measured 768 vs 512 as "slightly better accuracy, equal strength", but
# that was one run at 2400 games. This trains two independent 512 replicas from
# the same imitation data and recipe as the 768 reference, so the comparison is
# like for like, and verifies each at three seeds.
cd ~/Documents/mmoyager/mmoyager_mahjong
P=.venv/bin/python
T=python/trainer/train.py
for seed in 31 47; do
  echo "=== training 512 replica (seed $seed) ==="
  $P $T --data data/selfplay/im-v9.bin --out /tmp/ck-512-$seed.bin \
    --epochs 4 --hidden 512,512 --lr 1e-3 --lr-decay 0.8 --seed $seed \
    --max-records 2400000 --value-coef 0.5 --return-scale 0.25 \
    --note width512-replica-$seed
done
echo "=== evaluations (reference: ck-best 768 = 25575 / 25568 / 25604) ==="
for seed in 31 47; do
  for ev in 4242 5150 31337; do
    echo "--- 512-$seed vs efficiency, eval seed $ev"
    ./target/release/mmj-eval --a /tmp/ck-512-$seed.bin --b efficiency --games 4800 --seed $ev --json
  done
done
echo "=== speed: forwards per second for the 512 net ==="
./target/release/mmj-selfplay bench --checkpoint /tmp/ck-512-31.bin
./target/release/mmj-selfplay bench-encode --checkpoint /tmp/ck-512-31.bin 2>&1 | grep -E "encode:|forward:"
echo "WIDTH512 DONE"
