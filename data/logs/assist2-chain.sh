#!/bin/bash
cd ~/Documents/mmoyager/mmoyager_mahjong
while pgrep -f "lab-chain.sh" > /dev/null; do sleep 15; done
CK=data/checkpoints/ck-best.bin
# The strongest combination suggested by the first sweep.
echo "--- assisted:sh=2&calls=1"
./target/release/mmj-eval --a "assisted:$CK?sh=2&calls=1" --b "$CK" --games 4800 --seed 4242 --json
echo "--- assisted sh=2 calls=1 vs frozen baseline"
./target/release/mmj-eval --a "assisted:$CK?sh=2&calls=1" --b efficiency --games 4800 --seed 4242 --json
echo "ASSIST2 DONE"
