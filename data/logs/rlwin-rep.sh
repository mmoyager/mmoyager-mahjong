#!/bin/bash
# Replication of the binary-reward RL result: two more runs with fresh self-play
# data and different training seeds. A single run's margin is not evidence here
# (rounds 9-11), and this is the first time an RL update has moved the
# strong-opponent margin at all.
cd ~/Documents/mmoyager/mmoyager_mahjong
CK=data/checkpoints/ck-dagger.bin
for tag in r1 r2; do
  case $tag in
    r1) gseed=4242; tseed=5 ;;
    r2) gseed=5353; tseed=9 ;;
  esac
  echo "=== rlwin $tag (games seed $gseed, train seed $tseed) ==="
  ./target/release/mmj-selfplay generate --games 2000 --out /tmp/sp-win-$tag.bin \
    --checkpoint "$CK" --seats learner,learner,learner,learner \
    --epsilon 0.03 --batch 256 --seed $gseed --label-kind selfplay > /dev/null
  .venv/bin/python -u python/trainer/train.py \
    --data data/selfplay/im-v12.bin /tmp/sp-win-$tag.bin --init "$CK" \
    --out /tmp/ck-rlwin-$tag.bin --epochs 2 --lr 5e-5 --max-records 900000 --reward win \
    --ppo --clip 0.2 --kl-coef 0.5 --seed $tseed --threads 8 --note rl-win-$tag 2>&1 | grep -E "epoch 2|saved"
  printf "  $tag vs v2: "
  RAYON_NUM_THREADS=8 ./target/release/mmj-eval --a /tmp/ck-rlwin-$tag.bin --b efficiency-v2 \
    --games 4800 --seed 4242 --json | python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "RLWIN REP DONE"
