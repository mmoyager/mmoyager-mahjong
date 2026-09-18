#!/bin/bash
# The alignment diagnostic says the remaining self-play disagreement lives in two
# places: call windows (the student passes where the teacher calls, 2621 of 3457
# call errors) and 1-shanten own-turn discards (agreement 0.950 there against
# 0.99+ at every other shanten). A dose that labels *those* states plus the usual
# full coverage is compared with the plain full dose, both from ck-dagger.
cd ~/Documents/mmoyager/mmoyager_mahjong
CK=data/checkpoints/ck-dagger.bin
export RAYON_NUM_THREADS=8
# arm 1: full coverage (reference shape)
./target/release/mmj-selfplay generate --games 6000 --out /tmp/dose-full.bin --checkpoint "$CK" \
  --seats learner,learner,learner,learner --greedy --epsilon 0 --batch 256 --seed 8111 \
  --dagger --teacher-v2 --label-kind imitation > /dev/null
# arm 2: calls + anything at 1 shanten or worse
./target/release/mmj-selfplay generate --games 6000 --out /tmp/dose-target.bin --checkpoint "$CK" \
  --seats learner,learner,learner,learner --greedy --epsilon 0 --batch 256 --seed 8111 \
  --dagger --dagger-shanten 1 --dagger-calls --teacher-v2 --label-kind imitation > /dev/null
for arm in full target; do
  .venv/bin/python -u python/trainer/train.py --data /tmp/dose-$arm.bin --init "$CK" \
    --out /tmp/ck-dose2-$arm.bin --epochs 2 --lr 5e-5 --max-records 900000 \
    --entropy-coef 0.005 --value-coef 0.25 --return-scale 0.25 --threads 8 \
    --note second-dose-$arm 2>&1 | grep -E "epoch 2|saved"
  printf "  second dose ($arm) vs v2: "
  ./target/release/mmj-eval --a /tmp/ck-dose2-$arm.bin --b efficiency-v2 --games 4800 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "TARGETED DOSE DONE"
