"""Uniform fidelity measure: top-1 agreement with the stored label on the same
held-out slice of the newest imitation dataset, for any list of checkpoints.

Comparable across checkpoints because every one is scored on identical rows.
"""
"""How closely does a checkpoint reproduce the teacher's decisions?

Usage: ``python/fidelity.py CHECKPOINT [CHECKPOINT ...]``

Every checkpoint is scored on **the same held-out rows**, which is what makes the
numbers comparable: run-specific ``val_acc`` values come from different splits and
different data volumes, and were used for years to argue about imitation quality
without ever being comparable. Round 14 used this to show that fidelity does not
predict how a checkpoint does against a stronger opponent.
"""
import os
import sys
from pathlib import Path

import numpy as np
import torch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import mmjdata
from train import load_checkpoint

DATA = os.environ.get(
    "MMJ_FIDELITY_DATA",
    str(Path(__file__).resolve().parents[2] / "data/selfplay/im-v9.bin"),
)
CHECKPOINTS = sys.argv[1:]

ds = mmjdata.load(DATA, 400000)
rng = np.random.default_rng(12345)
idx = rng.choice(len(ds), 20000, replace=False)
feats, masks, actions, returns = ds.batch(idx)
x = torch.from_numpy(feats)
mask = torch.from_numpy(masks).to(torch.bool)
a = torch.from_numpy(actions)
print(f"held-out rows: {len(actions)} from {DATA}")
rows = []
for path in CHECKPOINTS:
    net, header = load_checkpoint(__import__("pathlib").Path(path))
    with torch.no_grad():
        body = net["body"](x)
        logits = net["policy"](body).masked_fill(~mask, -1e9)
        pred = logits.argmax(dim=1)
        agree = (pred == a).float().mean().item()
        # how often the teacher's action is in the model's top-3
        top3 = logits.topk(3, dim=1).indices
        in3 = (top3 == a.unsqueeze(1)).any(dim=1).float().mean().item()
    rows.append((path, agree, in3))
    print(f"  {path:<34} top1 {agree:.4f}  top3 {in3:.4f}")
