#!/bin/bash
# Same data, same recipe, same seed as the 512 replica — only the width differs.
# Without this control the -195 paired gap could just as well come from the
# training recipe, since the 768 reference (ck-v9) was produced by a different
# run than the loop's bootstrap.
cd ~/Documents/mmoyager/mmoyager_mahjong
.venv/bin/python python/trainer/train.py --data data/selfplay/im-v9.bin \
  --out /tmp/ck-768ctl.bin --epochs 4 --hidden 768,768 --lr 1e-3 --lr-decay 0.8 \
  --seed 31 --max-records 2400000 --value-coef 0.5 --return-scale 0.25 \
  --note width768-control > /dev/null 2>&1
echo "=== control trained ==="
echo "--- 768 control vs baseline, seed 4242 (ck-v9 reference +1150)"
./target/release/mmj-eval --a /tmp/ck-768ctl.bin --b efficiency --games 4800 --seed 4242 --json
echo "--- 768 control vs the 512 replica, paired (positive = 768 is stronger)"
./target/release/mmj-eval --a /tmp/ck-768ctl.bin --b /tmp/ck-512-31.bin --games 4800 --seed 5150 --json
echo "--- 768 control vs ck-v9 reference, paired"
./target/release/mmj-eval --a /tmp/ck-768ctl.bin --b data/reference/ck-v9.bin --games 4800 --seed 5150 --json
echo "WIDTH768CTL DONE"
