"""Why does the student call ten times less often than its teacher?

The style statistics (round 34) say `ck-ab` takes a call in ~1.9% of hands while
the rule teacher it was trained from calls in ~18.5%. Fidelity in aggregate hides
this because call windows are a small share of all decisions. This splits the
held-out agreement by decision type and, crucially, asks what the model does on
exactly those rows where the *teacher* chose to call.

    python/trainer/diagnose_calls.py CHECKPOINT [DATA]
"""
import os
import sys
from pathlib import Path

import numpy as np
import torch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import mmjdata
from train import load_checkpoint

ROOT = Path(__file__).resolve().parents[2]
DATA = sys.argv[2] if len(sys.argv) > 2 else os.environ.get(
    "MMJ_DIAG_DATA", str(ROOT / "data/selfplay/im-v12.bin")
)
CKPT = sys.argv[1]

CALLS = slice(105, 109)  # chi (105..107) and pon (108)
PASS = 104

ds = mmjdata.load(DATA, 400000)
rng = np.random.default_rng(2024)
idx = rng.choice(len(ds), 40000, replace=False)
feats, masks, actions, returns = ds.batch(idx)
mask = torch.from_numpy(masks).to(torch.bool)
a = actions
x = torch.from_numpy(feats)

call_offered = masks[:, CALLS].any(axis=1)
discard_offered = masks[:, 0:34].any(axis=1)
riichi_offered = masks[:, 34:68].any(axis=1)
teacher_called = (a >= 105) & (a <= 108)

net, header = load_checkpoint(Path(CKPT))
with torch.no_grad():
    logits = net["policy"](net["body"](x)).masked_fill(~mask, -1e9)
    pred = logits.argmax(dim=1).numpy()
model_called = (pred >= 105) & (pred <= 108)

print(f"checkpoint {CKPT}  data {Path(DATA).name}  rows {len(a)}")


def report(name, sel):
    if sel.sum() == 0:
        print(f"  {name:<26} (none)")
        return
    agree = (pred[sel] == a[sel]).mean()
    print(
        f"  {name:<26} rows {sel.sum():>6}  teacher calls {teacher_called[sel].mean():.3f}"
        f"  model calls {model_called[sel].mean():.3f}  top1 {agree:.3f}"
    )


report("all rows", np.ones(len(a), bool))
report("call offered", call_offered)
report("  ... teacher called", call_offered & teacher_called)
report("  ... teacher passed", call_offered & ~teacher_called)
report("discard offered", discard_offered & ~call_offered)
report("riichi offered", riichi_offered)

# Round 35 found that one *label class* (the teacher's calls) was almost never
# reproduced while aggregate fidelity hid it. The same question is worth asking
# of every class, so the split below is by the teacher's own action kind: a class
# whose recall collapses is a place where the imitation target is either
# unlearnable from these features or simply ignored by the model.
CLASSES = [
    ("discard", 0, 33),
    ("riichi discard", 34, 67),
    ("kan", 68, 101),
    ("tsumo", 102, 102),
    ("ron/kyuushu", 103, 103),
    ("pass", 104, 104),
    ("chi", 105, 107),
    ("pon", 108, 108),
]
# Within the discard class, the same question: is the 3% disagreement
# concentrated somewhere? The encoder stores the danger of every kind in a known
# slice, so the teacher's own discard can be bucketed by how dangerous it was.
DANGER_SLICE = slice(594, 628)
print("  discard rows by the danger of the teacher's own discard:")
disc = (a >= 0) & (a <= 33)
if disc.sum() > 0:
    danger = feats[disc, DANGER_SLICE.start + a[disc]]
    for lo, hi, name in [(0.0, 0.05, "safe (<0.05)"), (0.05, 0.3, "0.05-0.3"), (0.3, 1.01, "dangerous (>0.3)")]:
        sel = (danger >= lo) & (danger < hi)
        if sel.sum() == 0:
            continue
        sub = np.flatnonzero(disc)[sel]
        print(
            f"    {name:<16} rows {sel.sum():>6}  recall {(pred[sub] == a[sub]).mean():.3f}"
        )

print("  per label class:")
for name, lo, hi in CLASSES:
    sel = (a >= lo) & (a <= hi)
    if sel.sum() == 0:
        continue
    recall = (pred[sel] == a[sel]).mean()
    any_pred = ((pred >= lo) & (pred <= hi)).mean()
    print(
        f"    {name:<14} rows {sel.sum():>6}  recall {recall:.3f}  model uses this class "
        f"{any_pred:.3f}"
    )
