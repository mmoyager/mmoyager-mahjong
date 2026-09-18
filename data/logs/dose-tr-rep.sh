#!/bin/bash
# Replication of the DAgger dose that took ck-tr from -265 to +22 against the
# stronger rule bot. Two more independent runs: different games for the labels
# and a different training shuffle seed.
cd ~/Documents/mmoyager/mmoyager_mahjong
CK=data/checkpoints/ck-tr.bin
for tag in rep1 rep2; do
  case $tag in
    rep1) gseed=6666; tseed=11 ;;
    rep2) gseed=7777; tseed=22 ;;
  esac
  echo "=== dose $tag (games seed $gseed, train seed $tseed) ==="
  ./target/release/mmj-selfplay generate --games 6000 --out /tmp/dose-$tag.bin \
    --checkpoint "$CK" --seats learner,learner,learner,learner \
    --greedy --epsilon 0 --batch 256 --seed $gseed \
    --dagger --teacher-v2 --label-kind imitation > /dev/null
  .venv/bin/python -u python/trainer/train.py --data /tmp/dose-$tag.bin --init "$CK" \
    --out /tmp/ck-tr-dose-$tag.bin --epochs 2 --lr 5e-5 --max-records 900000 --seed $tseed \
    --entropy-coef 0.005 --value-coef 0.25 --return-scale 0.25 --threads 8 \
    --note dagger-dose-$tag 2>&1 | grep -E "epoch 2|saved"
  printf "  $tag vs v2: "
  ./target/release/mmj-eval --a /tmp/ck-tr-dose-$tag.bin --b efficiency-v2 --games 4800 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
  printf "  $tag vs v1: "
  ./target/release/mmj-eval --a /tmp/ck-tr-dose-$tag.bin --b efficiency --games 4800 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "DOSE REP DONE"
