#!/bin/bash
# Which decisions actually cost ck-best points? Play the checkpoint against
# itself with the rule teacher taking over one region at a time.
cd ~/Documents/mmoyager/mmoyager_mahjong
CK=data/checkpoints/ck-best.bin
for spec in "sh=3" "sh=2" "sh=1" "sh=1&calls=1" "sh=off&calls=1" "sh=2&v2=0"; do
  echo "--- assisted:$spec"
  ./target/release/mmj-eval --a "assisted:$CK?$spec" --b "$CK" --games 4800 --seed 4242 --json
done
echo "SWEEP DONE"
