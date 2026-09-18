"""Average several checkpoints that share an architecture ("model soup").

Averaging weights of independently trained models is a cheap regulariser: each
run overfits the teacher slightly differently, and the average is closer to the
rule they all approximate. No inference change is needed because the averaged
checkpoint has the same shape as its inputs.

    python trainer/soup.py --in a.bin b.bin c.bin --out soup.bin
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np

MAGIC = b"MMJNN1"


def read(path: Path):
    raw = path.read_bytes()
    if not raw.startswith(MAGIC):
        raise ValueError(f"{path} is not a checkpoint")
    first = raw.index(b"\n")
    second = raw.index(b"\n", first + 1)
    header = json.loads(raw[first + 1 : second])
    body = np.frombuffer(raw[second + 1 :], dtype="<f4")
    return header, body


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--in", dest="inputs", nargs="+", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    headers, bodies = [], []
    for p in args.inputs:
        h, b = read(Path(p))
        headers.append(h)
        bodies.append(b)
        print(f"  {p}: {len(b)} params, val_acc {h.get('info', {}).get('val_policy_acc', '?')}")

    shapes = {tuple(l["out"] for l in h["layers"]) for h in headers}
    if len(shapes) != 1:
        raise SystemExit(f"architectures differ: {shapes}")
    n = min(len(b) for b in bodies)
    stacked = np.stack([b[:n] for b in bodies])
    mean = stacked.mean(axis=0).astype("<f4")

    header = dict(headers[0])
    info = dict(header.get("info") or {})
    info["soup"] = [str(p) for p in args.inputs]
    header["info"] = info
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    with open(out, "wb") as f:
        f.write(MAGIC + b"\n")
        f.write(json.dumps(header).encode() + b"\n")
        f.write(mean.tobytes())
    print(f"wrote {out} ({len(mean)} params averaged from {len(bodies)} checkpoints)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
