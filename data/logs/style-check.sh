#!/bin/bash
# Round 34: sanity-check the style numbers before reporting them. The current
# model's call rate (melds per player-hand) came out at 0.019, against 0.205 for
# the round-24 lineage and 0.326 for humans, so first confirm the measurement
# itself by running the same statistic on the rule teacher.
cd ~/Documents/mmoyager/mmoyager_mahjong
M=./target/release/mmj-selfplay
show() {
  python3 -c "
import sys, json
txt = sys.stdin.read()
i = txt.index('{'); j = txt.rindex('}') + 1
d = json.loads(txt[i:j])
keys = ['win_rate','deal_in_rate','riichi_rate','call_rate','draw_rate','tsumo_share','avg_win_points','rounds_per_game']
print('   ' + '  '.join(f'{k}={d[k]:.4f}' if isinstance(d[k], float) else f'{k}={d[k]}' for k in keys))"
}
echo "=== rule teacher v2 (all four seats) ==="
$M stats --seats efficiency,efficiency,efficiency,efficiency --teacher-v2 --games 1200 --seed 11 --hanchan 2>/dev/null | show
echo "=== rule teacher v1 (all four seats) ==="
$M stats --seats efficiency,efficiency,efficiency,efficiency --games 1200 --seed 11 --hanchan 2>/dev/null | show
echo "=== ck-ab (learner seats) ==="
$M stats --checkpoint data/checkpoints/ck-ab.bin --games 1200 --seed 11 --hanchan 2>/dev/null | show
echo "STYLE CHECK DONE"
