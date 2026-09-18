#!/bin/bash
# Does a DAgger dose on the *student's own* states fix the call-decision shift?
# Round 15 measured `ck-tr` calling on 10.3% of hands where its teacher calls on
# 18.2%, even though it reproduces the teacher's decisions on teacher-visited
# states 96.9% of the time. The dose uses the validated shape (labels on the
# states the current policy reaches, all decisions, no anchor, 2 epochs at
# lr 5e-5) and is judged on the strong-opponent margin.
cd ~/Documents/mmoyager/mmoyager_mahjong
CK=data/checkpoints/ck-tr.bin
echo "=== generate DAgger labels on ck-tr's own states ==="
./target/release/mmj-selfplay generate --games 6000 --out /tmp/dose-tr.bin \
  --checkpoint "$CK" --seats learner,learner,learner,learner \
  --greedy --epsilon 0 --batch 256 --seed 5555 \
  --dagger --teacher-v2 --label-kind imitation
echo "=== train the dose ==="
.venv/bin/python -u python/trainer/train.py --data /tmp/dose-tr.bin --init "$CK" \
  --out /tmp/ck-tr-dose.bin --epochs 2 --lr 5e-5 --max-records 900000 \
  --entropy-coef 0.005 --value-coef 0.25 --return-scale 0.25 --threads 8 \
  --note dagger-dose-on-cktr 2>&1 | grep -E "epoch |saved"
export RAYON_NUM_THREADS=8
echo "=== margins ==="
for opp in efficiency-v2 efficiency; do
  printf "dose vs %-14s " "$opp"
  ./target/release/mmj-eval --a /tmp/ck-tr-dose.bin --b "$opp" --games 4800 --seed 4242 --json | \
    python3 -c "import json,sys;d=json.load(sys.stdin);print(f\"{d['avg_a']-d['avg_b']:+8.1f}  rank {d['avg_rank_a']:.4f}\")"
done
echo "=== call rate after the dose ==="
./target/release/mmj-selfplay stats --games 200 --hanchan --seats learner,learner,learner,learner \
  --checkpoint /tmp/ck-tr-dose.bin --seed 4242 | python3 -c "
import json,sys;d=json.load(sys.stdin)
print(f\"  和了率 {d['win_rate']:.3f}  放銃率 {d['deal_in_rate']:.3f}  立直率 {d['riichi_rate']:.3f}  副露率 {d['call_rate']:.3f}  流局率 {d['draw_rate']:.3f}\")"
echo "DOSE TR DONE"
