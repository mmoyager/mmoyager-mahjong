#!/bin/bash
# Round 38: the final strength profile of the served checkpoint. Everything the
# project claims rests on comparisons against two rule bots; this widens it to
# every opponent the engine can field, adds a mirror-match control (a checkpoint
# against itself must measure 0, which calibrates how much of any number is the
# evaluation itself), and repeats the two headline numbers on fresh seeds.
cd ~/Documents/mmoyager/mmoyager_mahjong
E=./target/release/mmj-eval
CK=data/checkpoints/ck-ab.bin
probe() { # name, opponent spec, games, seed
  printf "  %-26s " "$1"
  $E --a "$CK" --b "$2" --games "$3" --seed "$4" --json | python3 -c "
import json,sys
d=json.load(sys.stdin)
print(f\"margin {d['avg_a']-d['avg_b']:+9.1f}  rank {d['avg_rank_a']:.4f}  firsts {d['first_rate_a']:.4f}  hands {d['wins_a']}:{d['wins_b']}  games {d['games']}\")"
}
echo "=== mirror control (the same checkpoint on both sides) ==="
probe "ck-ab vs ck-ab" "$CK" 9600 4242
echo "=== against the rule bots ==="
probe "vs efficiency-v2 (v2)" "efficiency-v2" 9600 4242
probe "vs efficiency (v1)" "efficiency" 4800 4242
probe "vs efficiency-v3 (smarter)" "efficiency-v3" 4800 4242
probe "vs efficiency-yaku" "efficiency-yaku" 4800 4242
echo "=== against degenerate opponents ==="
probe "vs random" "random" 4800 4242
echo "=== a second seed for the headline numbers ==="
probe "vs efficiency-v2 (seed 2024)" "efficiency-v2" 9600 2024
probe "vs efficiency (seed 2024)" "efficiency" 4800 2024
echo "FINAL PROFILE DONE"
