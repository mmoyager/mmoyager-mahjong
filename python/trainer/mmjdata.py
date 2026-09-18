"""Reader for the experience files written by `mmj-selfplay`.

The on-disk format is documented in `rust/mmj-nn/src/data.rs`; this module is
its Python counterpart. Records are kept as raw ``uint8`` and only converted to
float in batches, so a few hundred thousand decisions cost little memory.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

import numpy as np

MAGIC = b"MMJDATA1"
HEADER_BYTES = 1024


@dataclass
class Dataset:
    """One experience file, memory-mapped into numpy views."""

    path: Path
    kind: str
    meta: dict
    feature_dim: int
    policy_dim: int
    mask_bytes: int
    count: int
    features: np.ndarray  # uint8 [count, feature_dim]
    masks: np.ndarray  # bool  [count, policy_dim]
    actions: np.ndarray  # int64 [count]
    returns: np.ndarray  # float32 [count]
    seats: np.ndarray  # uint8 [count]
    # Outcome decomposition, present in files written after round 19. Each row
    # satisfies ``returns == won_value - dealt_value + other_value``.
    won_value: np.ndarray | None = None  # float32 [count]
    dealt_value: np.ndarray | None = None  # float32 [count]
    other_value: np.ndarray | None = None  # float32 [count]
    # Feature columns that are forced to zero when a batch is materialised. Used
    # as a control arm for a new feature block: the same data, the same
    # architecture and the same recipe, with the block made unavailable.
    zero_slice: slice | None = None

    def __len__(self) -> int:
        return self.count

    def batch(self, idx: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]:
        """Return ``(features_f32, masks_bool, actions_i64, returns_f32)``."""
        feats = self.features[idx].astype(np.float32) / 255.0
        if self.zero_slice is not None:
            feats[:, self.zero_slice] = 0.0
        return feats, self.masks[idx], self.actions[idx], self.returns[idx]


def _read_header(raw: bytes) -> dict:
    if not raw.startswith(MAGIC):
        raise ValueError("not an mmj experience file")
    start = len(MAGIC) + 1
    header = json.loads(raw[start : start + HEADER_BYTES])
    return header


def load(path: str | Path, max_records: int | None = None) -> Dataset:
    # Only the header is read eagerly; the records stay in the file and are
    # memory-mapped, so a multi-hundred-megabyte file costs almost no RAM.
    path = Path(path)
    head = path.open("rb").read(len(MAGIC) + 1 + HEADER_BYTES)
    header = _read_header(head)
    feature_dim = int(header["feature_dim"])
    policy_dim = int(header["policy_dim"])
    mask_bytes = int(header["mask_bytes"])
    record_bytes = int(header["record_bytes"])
    header_end = len(MAGIC) + 1 + HEADER_BYTES + 1
    total_bytes = path.stat().st_size - header_end
    count = total_bytes // record_bytes
    if header.get("count") not in (None, count):
        # The writer patches the header at the end; a mismatch means the file
        # was truncated mid-run.
        print(f"  note: {path.name} header says {header['count']} records, found {count}")
    if max_records is not None:
        count = min(count, max_records)

    body = np.memmap(path, dtype=np.uint8, mode="r", offset=header_end, shape=(count * record_bytes,))
    arr = body.reshape(count, record_bytes)
    features = arr[:, :feature_dim]
    masks = (
        np.unpackbits(arr[:, feature_dim : feature_dim + mask_bytes], axis=1, bitorder="little")[
            :, :policy_dim
        ].astype(bool)
    )
    action_bytes = arr[:, feature_dim + mask_bytes : feature_dim + mask_bytes + 2]
    actions = (
        action_bytes[:, 0].astype(np.int64) | (action_bytes[:, 1].astype(np.int64) << 8)
    )
    returns = (
        arr[:, feature_dim + mask_bytes + 2 : feature_dim + mask_bytes + 6]
        .copy()
        .view("<f4")
        .ravel()
        .astype(np.float32)
    )
    seats = arr[:, -1].astype(np.uint8)
    # Files written before the decomposition landed have a shorter record and no
    # component columns; those datasets simply report `None`.
    won_value = dealt_value = other_value = None
    if record_bytes >= feature_dim + mask_bytes + 2 + 4 + 12 + 1:
        base = feature_dim + mask_bytes + 2 + 4
        def f32(col0: int) -> np.ndarray:
            return (
                arr[:, col0 : col0 + 4].copy().view("<f4").ravel().astype(np.float32)
            )
        won_value = f32(base)
        dealt_value = f32(base + 4)
        other_value = f32(base + 8)
    return Dataset(
        path=path,
        kind=str(header.get("kind", "unknown")),
        meta=header.get("meta", {}) or {},
        feature_dim=feature_dim,
        policy_dim=policy_dim,
        mask_bytes=mask_bytes,
        count=count,
        features=features,
        masks=masks,
        actions=actions,
        returns=returns,
        seats=seats,
        won_value=won_value,
        dealt_value=dealt_value,
        other_value=other_value,
    )


def load_all(paths: list[str], max_records: int | None = None) -> list[Dataset]:
    return [load(p, max_records) for p in paths if Path(p).exists()]


if __name__ == "__main__":  # a quick sanity report
    import sys

    for arg in sys.argv[1:]:
        ds = load(arg)
        legal = ds.masks.sum(axis=1)
        bad = int((~ds.masks[np.arange(ds.count), ds.actions]).sum())
        print(
            f"{ds.path.name}: kind={ds.kind} records={ds.count} features={ds.feature_dim} "
            f"legal/decision={legal.mean():.2f} illegal_actions={bad} "
            f"return mean={ds.returns.mean():.3f} sd={ds.returns.std():.3f}"
        )
