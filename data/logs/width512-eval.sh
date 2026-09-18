#!/bin/bash
export RAYON_NUM_THREADS=3
cd ~/Documents/mmoyager/mmoyager_mahjong
echo "=== speed: 512 vs 768 ==="
./target/release/mmj-selfplay bench --checkpoint /tmp/ck-512-31.bin | tail -2
./target/release/mmj-selfplay bench --checkpoint data/reference/ck-v9.bin | tail -2
echo "=== 512 replica 31 vs baseline (768 reference: 25575/25568/25604) ==="
for ev in 4242 5150 31337; do
  echo "--- eval seed $ev"
  ./target/release/mmj-eval --a /tmp/ck-512-31.bin --b efficiency --games 4800 --seed $ev --json
done
echo "--- 512 vs the 768 reference, paired"
./target/release/mmj-eval --a /tmp/ck-512-31.bin --b data/reference/ck-v9.bin --games 4800 --seed 5150 --json
echo "512 EVAL DONE"
