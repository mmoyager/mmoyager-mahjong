#!/bin/bash
# Is the model's under-calling costing points? Same-batch control included
# (bias 1.0 is the plain model) so the comparison cannot repeat round 20's
# mistake of testing candidates against a stale baseline measurement.
export RAYON_NUM_THREADS=8
cd ~/Documents/mmoyager/mmoyager_mahjong
CK=data/checkpoints/ck-dagger.bin
for b in 1.0 1.5 3.0 6.0; do
  printf "call bias %-4s vs v2 @4800  " "$b"
  ./target/release/mmj-eval --a "bias:$CK?calls=$b" --b efficiency-v2 --games 4800 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "CALLBIAS DONE"
