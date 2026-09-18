#!/bin/bash
# Round 28: does the explicit AVX2 kernel actually speed the forward up, and does
# it speed the end-to-end pipelines up? Old = chunked scalar kernel, new = AVX2
# kernel; the two produce bit-identical records (verified separately). Runs are
# interleaved old/new so that load from the background training loop cancels.
cd ~/Documents/mmoyager/mmoyager_mahjong
OLD=./target/release/mmj-selfplay
NEW=/tmp/mmj-perf2/release/mmj-selfplay
export RAYON_NUM_THREADS=8
gen() { # $1 bin, $2 tag, $3 seed
  $1 generate --games 1200 --out /tmp/avx-$2.bin \
    --checkpoint data/checkpoints/ck-ab.bin --seats learner,learner,learner,learner \
    --greedy --epsilon 0 --batch 256 --seed $3 --dagger --teacher-v2 \
    --label-kind imitation 2>&1 | tail -1 | sed "s/^/  $2/seed$3: /"
}
for round in 1 2; do
  echo "--- round $round ---"
  printf "  bench old: "; $OLD bench --checkpoint data/checkpoints/ck-ab.bin | sed -n 2p
  printf "  bench new: "; $NEW bench --checkpoint data/checkpoints/ck-ab.bin | sed -n 2p
  gen $OLD old 4242
  gen $NEW new 4242
  gen $OLD old 5150
  gen $NEW new 5150
done
echo "AVX2 AB DONE"
