#!/bin/bash
# Matched-pair evaluation of the tie-aware arm: replicate k of the tie-aware arm
# is compared with replicate k of the control arm (same data, same base, same
# shuffle seed), so the comparison cancels every source of variation except the
# objective function itself.
export RAYON_NUM_THREADS=4
cd ~/Documents/mmoyager/mmoyager_mahjong
while pgrep -f "tieaware.sh" > /dev/null; do
  for seed in 202 303 404; do
    if [ -f /tmp/tie-$seed.bin ] && [ ! -f /tmp/tie-$seed.done ]; then
      for ev in 4242 5150; do
        echo "--- tie-aware $seed, eval seed $ev"
        ./target/release/mmj-eval --a /tmp/tie-$seed.bin --b efficiency --games 4800 --seed $ev --json
      done
      touch /tmp/tie-$seed.done
    fi
  done
  sleep 30
done
echo "TIE EVAL DONE"
