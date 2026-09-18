"""Trainer for the mmoyager mahjong policy/value network.

It reads experience files produced by `mmj-selfplay` and writes checkpoints that
the Rust engine loads for self-play and for the web UI.

Two kinds of data are mixed freely:

* ``imitation`` — actions taken by the tile-efficiency baseline. Trained with
  plain cross-entropy; this bootstraps the very first policy.
* ``selfplay`` — actions taken by an earlier version of the network, each with
  its Monte-Carlo return. Trained with an advantage-weighted policy gradient
  plus value regression (A2C with Monte-Carlo returns).

Both kinds also train the value head, which predicts the acting player's final
score change from that point on (in thousands of points).

Example
-------
    python trainer/train.py --data ../data/selfplay/im-0000.bin \
        --out ../data/checkpoints/ck-0000.bin --epochs 4 --hidden 512,512
"""

from __future__ import annotations

import argparse
import json
import math
import os
import time
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F

import mmjdata

MAGIC = "MMJNN1"


DECOMPOSED_HEADS = ("p_win", "v_win", "p_deal", "v_deal", "v_other")


def build_net(
    feature_dim: int, policy_dim: int, hidden: list[int], decomposed: bool = False
) -> nn.Module:
    layers: list[nn.Module] = []
    prev = feature_dim
    for h in hidden:
        layers.append(nn.Linear(prev, h))
        layers.append(nn.ReLU())
        prev = h
    body = nn.Sequential(*layers)
    modules = {
        "body": body,
        "policy": nn.Linear(prev, policy_dim),
        "value": nn.Linear(prev, 1),
    }
    if decomposed:
        # Low-variance pieces of the same expectation: whether the hand was won
        # and for how much, whether it was dealt into and for how much, and the
        # remainder (riichi sticks, tenpai payments). Each is far easier to
        # predict than the whole hand swing, and they recombine into it.
        for name in DECOMPOSED_HEADS:
            modules[name] = nn.Linear(prev, 1)
    return nn.ModuleDict(modules)


def load_checkpoint(path: Path) -> tuple[nn.Module, dict]:
    raw = path.read_bytes()
    if not raw.startswith(MAGIC.encode()):
        raise ValueError(f"{path} is not a mmj checkpoint")
    first_nl = raw.index(b"\n")
    second_nl = raw.index(b"\n", first_nl + 1)
    header = json.loads(raw[first_nl + 1 : second_nl])
    layers = header["layers"] + [header["policy"], header["value"]]
    feature_dim = int(header["feature_dim"])

    hidden = [int(l["out"]) for l in header["layers"]]
    decomposed = list(header.get("decomposed") or [])
    net = build_net(
        feature_dim, int(header["policy"]["out"]), hidden, decomposed=bool(decomposed)
    )
    offset = second_nl + 1
    seq = (
        [net["body"][2 * i] for i in range(len(hidden))]
        + [net["policy"], net["value"]]
        + [net[h] for h in DECOMPOSED_HEADS if h in net]
    )
    layers = layers + [{"in": hidden[-1], "out": 1}] * len(decomposed)
    with torch.no_grad():
        for layer, spec in zip(seq, layers):
            out, inp = int(spec["out"]), int(spec["in"])
            n_w = out * inp
            w = np.frombuffer(raw[offset : offset + 4 * n_w], dtype="<f4").reshape(out, inp)
            offset += 4 * n_w
            b = np.frombuffer(raw[offset : offset + 4 * out], dtype="<f4")
            offset += 4 * out
            layer.weight.copy_(torch.from_numpy(w.copy()))
            layer.bias.copy_(torch.from_numpy(b.copy()))
    return net, header


def save_checkpoint(net: nn.Module, feature_dim: int, policy_dim: int, path: Path, info: dict) -> None:
    """Write the format documented in `rust/mmj-nn/src/lib.rs`."""
    path.parent.mkdir(parents=True, exist_ok=True)
    body_layers = [m for m in net["body"] if isinstance(m, nn.Linear)]
    specs = []
    for layer in body_layers:
        specs.append({"in": layer.in_features, "out": layer.out_features, "act": "relu"})
    specs_policy = {
        "in": net["policy"].in_features,
        "out": net["policy"].out_features,
        "act": "linear",
    }
    specs_value = {"in": net["value"].in_features, "out": net["value"].out_features, "act": "linear"}
    header = {
        "layers": specs,
        "policy": specs_policy,
        "value": specs_value,
        "feature_dim": feature_dim,
        "info": info,
    }
    # The head list must be in the header *before* it is written: an earlier
    # version appended the weights but mutated the header afterwards, so the
    # reader never learned the extra heads existed.
    extra = [net[h] for h in DECOMPOSED_HEADS if h in net]
    if extra:
        header["decomposed"] = [h for h in DECOMPOSED_HEADS if h in net]
    tmp = path.with_suffix(path.suffix + ".tmp")
    with open(tmp, "wb") as f:
        f.write(MAGIC.encode() + b"\n")
        f.write(json.dumps(header).encode() + b"\n")
        for layer in body_layers + [net["policy"], net["value"]] + extra:
            w = layer.weight.detach().cpu().numpy().astype("<f4")
            b = layer.bias.detach().cpu().numpy().astype("<f4")
            f.write(w.tobytes())
            f.write(b.tobytes())
    os.replace(tmp, path)


class Batcher:
    """Shuffles a mix of datasets and yields training batches."""

    def __init__(self, datasets: list[mmjdata.Dataset], batch_size: int, rng: np.random.Generator):
        self.datasets = datasets
        self.batch_size = batch_size
        self.rng = rng
        offsets = np.cumsum([0] + [len(d) for d in datasets])
        self.offsets = offsets
        self.total = int(offsets[-1])

    def __len__(self) -> int:
        return max(1, self.total // self.batch_size)

    def epoch(self):
        order = self.rng.permutation(self.total)
        for start in range(0, self.total - self.batch_size + 1, self.batch_size):
            yield order[start : start + self.batch_size]

    def fetch(self, flat_idx: np.ndarray):
        """Split flat indices into per-dataset batches, keeping the data kind."""
        groups = []
        for i, ds in enumerate(self.datasets):
            lo, hi = self.offsets[i], self.offsets[i + 1]
            local = flat_idx[(flat_idx >= lo) & (flat_idx < hi)] - lo
            if len(local) == 0:
                continue
            feats, masks, actions, returns = ds.batch(local)
            parts = None
            if ds.won_value is not None:
                parts = (
                    ds.won_value[local],
                    ds.dealt_value[local],
                    ds.other_value[local],
                )
            groups.append((ds.kind, feats, masks, actions, returns, parts))
        return groups


# Layout of the derived blocks inside an observation (kept in sync with
# `mmj-nn::encode`). `LOOKAHEAD` holds (8 - shanten after discarding kind) / 9 and
# `UKEIRE` holds the tile acceptance of that discard divided by 24.
LOOKAHEAD = slice(492, 526)
UKEIRE = slice(526, 560)
KEEP_VALUE = slice(560, 594)
DANGER = slice(594, 628)
IMITATION_KINDS = 34


def tie_aware_targets(
    feats: torch.Tensor, actions: torch.Tensor, mask: torch.Tensor
) -> torch.Tensor | None:
    """A distribution over the actions that are *equivalent* to the teacher's.

    Roughly half of the decisions where the student disagrees with the teacher
    are exact ties: same shanten after the discard, same tile acceptance. The
    teacher breaks those with a crude tie-break, so training the policy to put
    all of its mass on that arbitrary choice injects noise into the gradient and
    teaches a preference that has no basis in the game. Spreading the target
    uniformly over the tie set says "any of these is right" instead.

    The tie set is rebuilt from the encoded lookahead blocks, so no data format
    change is needed. Rows whose teacher action is not a discard, or whose tie
    set cannot be determined, fall back to the one-hot target.
    """
    b = feats.shape[0]
    if feats.shape[1] < UKEIRE.stop:
        return None
    sh = 8.0 - 9.0 * feats[:, LOOKAHEAD]      # shanten after discarding each kind
    uke = 24.0 * feats[:, UKEIRE]             # acceptance of that discard
    keep = feats[:, KEEP_VALUE]
    danger = feats[:, DANGER]
    kind = torch.where(
        actions < IMITATION_KINDS, actions, actions - IMITATION_KINDS
    ).clamp(0, IMITATION_KINDS - 1)
    row = torch.arange(b)
    sh_here = sh[row, kind]
    uke_here = uke[row, kind]
    is_discard = actions < 68
    # A kind whose lookahead entry is all zero is either absent from the hand or
    # a hopeless discard; either way it is not part of the tie set.
    valid = (sh < 8.0) & (uke > 0.0)
    # Equivalence must cover everything the teacher itself looks at, otherwise
    # the target would erase a *deliberate* preference (a slightly worse tile
    # kept for safety or for a yaku) rather than an arbitrary tie-break.
    tie = (
        (sh == sh_here.unsqueeze(1))
        & (uke == uke_here.unsqueeze(1))
        & (keep == keep[row, kind].unsqueeze(1))
        & ((danger - danger[row, kind].unsqueeze(1)).abs() < 1e-6)
        & valid
        & is_discard.unsqueeze(1)
        & (sh_here < 8.0).unsqueeze(1)
        & (uke_here > 0.0).unsqueeze(1)
    )
    # Only same-group equivalents: a riichi discard is a different action from a
    # plain discard of the same tile.
    offset = torch.where(actions < IMITATION_KINDS, 0, IMITATION_KINDS)
    slots = torch.arange(IMITATION_KINDS).unsqueeze(0) + offset.unsqueeze(1)
    targets = torch.zeros(b, mask.shape[1], dtype=torch.float32)
    targets.scatter_(1, slots, tie.float())
    size = targets.sum(dim=1, keepdim=True)
    empty = size.squeeze(1) <= 0.0
    if empty.any():
        targets[empty] = 0.0
        targets[empty, actions[empty]] = 1.0
    else:
        targets = targets / size
    # Legal-slot guard: never put target mass on an illegal action.
    targets = targets * mask.float()
    total = targets.sum(dim=1, keepdim=True).clamp(min=1e-6)
    return targets / total


def decomposed_value_loss(
    net: nn.Module,
    body: torch.Tensor,
    parts: tuple[np.ndarray, np.ndarray, np.ndarray],
    return_scale: float,
) -> torch.Tensor:
    """Fit the low-variance pieces of the hand outcome instead of its total.

    The target being replaced has a standard deviation of ~4.7 thousand points
    and the best single-head model explains 14% of it. Whether a hand was won is
    a binary event and its payout is a much tighter quantity, so each piece is
    learnable; ``p_win * v_win - p_deal * v_deal + v_other`` reconstructs the same
    expectation with far less noise.
    """
    won = torch.from_numpy(parts[0]) * return_scale
    dealt = torch.from_numpy(parts[1]) * return_scale
    other = torch.from_numpy(parts[2]) * return_scale
    p_win = net["p_win"](body).squeeze(1)
    v_win = net["v_win"](body).squeeze(1)
    p_deal = net["p_deal"](body).squeeze(1)
    v_deal = net["v_deal"](body).squeeze(1)
    v_other = net["v_other"](body).squeeze(1)
    won_mask = (won > 0).float()
    dealt_mask = (dealt > 0).float()
    bce = F.binary_cross_entropy_with_logits
    loss = bce(p_win, won_mask) + bce(p_deal, dealt_mask)
    # Payouts are only observable on hands where the event happened.
    if won_mask.sum() > 0:
        loss = loss + (((v_win - won) * won_mask) ** 2).sum() / won_mask.sum()
    if dealt_mask.sum() > 0:
        loss = loss + (((v_deal - dealt) * dealt_mask) ** 2).sum() / dealt_mask.sum()
    loss = loss + ((v_other - other) ** 2).mean()
    return loss


def value_error(pred: torch.Tensor, target: torch.Tensor, kind: str) -> torch.Tensor:
    if kind == "huber":
        return F.smooth_l1_loss(pred, target)
    return F.mse_loss(pred, target)


def evaluate(
    net: nn.Module,
    batcher: Batcher,
    batches: int,
    value_coef: float,
    rng,
    return_scale: float = 1.0,
    value_loss_kind: str = "huber",
    body_forward=None,
) -> dict:
    """`body_forward` is the (possibly torch.compiled) body callable; it defaults
    to the plain module so callers that do not compile need not pass anything."""
    if body_forward is None:
        body_forward = net["body"]
    net.eval()
    totals = {
        "policy_acc": 0.0,
        "value_loss": 0.0,
        "value_mse": 0.0,
        "loss": 0.0,
        "n": 0,
        "target_var": 0.0,
        "pred_var": 0.0,
        "dec_mse": 0.0,
        "dec_pred_var": 0.0,
        "dec_samples": 0,
    }
    with torch.no_grad():
        for i, idx in enumerate(batcher.epoch()):
            if i >= batches:
                break
            for kind, feats, masks, actions, returns, parts in batcher.fetch(idx):
                x = torch.from_numpy(feats)
                mask = torch.from_numpy(masks)
                a = torch.from_numpy(actions)
                ret = torch.from_numpy(returns) * return_scale
                body = body_forward(x)
                logits = net["policy"](body)
                mask_f = mask.to(torch.bool)
                masked = logits.masked_fill(~mask_f, -1e9)
                logp = F.log_softmax(masked, dim=1)
                nll = -logp.gather(1, a.unsqueeze(1)).squeeze(1)
                pred = net["value"](body).squeeze(1)
                vloss = value_error(pred, ret, value_loss_kind)
                acc = (masked.argmax(dim=1) == a).float().mean()
                loss = nll.mean() + value_coef * vloss
                n = len(a)
                totals["policy_acc"] += float(acc) * n
                # Keep the optimization loss and the *error* separate: with
                # `--value-loss huber` the loss is not a squared error, and
                # feeding it into an R^2 formula inflates the reported figure
                # several-fold. Every checkpoint of this project was reported as
                # `val_r2 ~ 0.72` on that mistake; the honest number is 0.14.
                if "p_win" in net and parts is not None:
                    with torch.no_grad():
                        dec = (
                            torch.sigmoid(net["p_win"](body).squeeze(1))
                            * net["v_win"](body).squeeze(1)
                            - torch.sigmoid(net["p_deal"](body).squeeze(1))
                            * net["v_deal"](body).squeeze(1)
                            + net["v_other"](body).squeeze(1)
                        )
                        totals["dec_mse"] += float(((dec - ret) ** 2).mean()) * n
                        totals["dec_pred_var"] += float(dec.var(unbiased=False)) * n
                        totals["dec_samples"] += n
                totals["value_loss"] += float(vloss) * n
                totals["value_mse"] += float(((pred - ret) ** 2).mean()) * n
                totals["pred_var"] += float(pred.var(unbiased=False)) * n
                totals["target_var"] += float(ret.var(unbiased=False)) * n
                totals["loss"] += float(loss) * n
                totals["n"] += n
    net.train()
    n = max(1, totals["n"])
    mse = totals["value_mse"] / n
    var = totals["target_var"] / n
    return {
        "policy_acc": totals["policy_acc"] / n,
        "value_loss": totals["value_loss"] / n,
        "value_mse": mse,
        # Explained variance is the number that says whether the value head is
        # usable for search or as an RL baseline. It must come from the *squared*
        # error, and `pred_var / target_var` exposes the other failure mode: a
        # head that predicts the mean has a high R^2 on nothing and a tiny spread.
        "value_r2": 1.0 - mse / var if var > 1e-9 else 0.0,
        "pred_var": totals["pred_var"] / n,
        "target_var": var,
        # `nan` rather than 1.0 when the branch never ran: an accumulator that
        # stays at zero would otherwise look like a perfect fit. (Round 18's
        # lesson was the same mistake in the other direction.)
        "decomposed_r2": (
            1.0 - (totals["dec_mse"] / n) / var
            if var > 1e-9 and totals["dec_samples"] > 0
            else float("nan")
        ),
        "decomposed_pred_var": totals["dec_pred_var"] / n,
        "loss": totals["loss"] / n,
        "n": totals["n"],
    }


def main() -> int:
    ap = argparse.ArgumentParser(description="train the mmj policy/value network")
    ap.add_argument("--data", nargs="+", required=True, help="experience files")
    ap.add_argument("--out", required=True, help="checkpoint to write")
    ap.add_argument("--init", default=None, help="continue from this checkpoint")
    ap.add_argument("--epochs", type=int, default=3)
    ap.add_argument("--batch-size", type=int, default=512)
    ap.add_argument("--reward", choices=["return", "win"], default="return",
                    help="reward for the self-play rows. 'win' replaces the hand's point "
                         "swing with the binary outcome of the hand, whose value function is "
                         "learnable (AUC 0.787) whereas the swing's is not (R2 0.14) -- the "
                         "first RL setup here with a baseline that carries signal.")
    ap.add_argument("--decomposed-value", action="store_true",
                    help="train the value as low-variance pieces (win / deal-in / rest) "
                         "instead of the whole hand swing. Requires a dataset written with "
                         "outcome components (round 19 onward).")
    ap.add_argument("--tie-aware", action="store_true",
                    help="imitate a *distribution* over equivalent actions instead of the "
                         "teacher's arbitrary tie-break (about half of all disagreements "
                         "are exact ties in shanten and tile acceptance)")
    ap.add_argument("--lr", type=float, default=1e-3)
    ap.add_argument("--value-coef", type=float, default=0.5)
    ap.add_argument("--return-scale", type=float, default=0.1,
                    help="scale applied to stored returns before value regression; "
                         "the stored unit is one thousand points, and scaling keeps the "
                         "value loss comparable to the policy loss instead of dominating it")
    ap.add_argument("--value-loss", default="huber", choices=["huber", "mse"])
    ap.add_argument("--entropy-coef", type=float, default=0.002)
    ap.add_argument("--max-records", type=int, default=900_000, help="cap per file")
    ap.add_argument("--weight-push", type=float, default=1.0,
                    help="multiply the loss of rows whose label is a *dangerous* discard (the "
                         "encoder's own danger for that tile exceeds --push-threshold) by this "
                         "factor. 1.0 leaves it alone; 10.0 asks whether the teacher's exact "
                         "choice in threatened positions deserves more weight than the student "
                         "currently gives it.")
    ap.add_argument("--push-threshold", type=float, default=0.3)
    ap.add_argument("--weight-calls", type=float, default=1.0,
                    help="multiply the imitation loss of rows whose label is a call (chi/pon) "
                         "by this factor. 1.0 leaves the loss untouched; 10.0 makes the 1.2%% of "
                         "decisions where the teacher called count for about a tenth of the "
                         "gradient, testing whether the student's ten-fold lower call rate is a "
                         "class-imbalance artefact.")
    ap.add_argument("--compile", action="store_true",
                    help="wrap the body in torch.compile. The training step dominates a loop "
                         "iteration (375 s of a 780 s one) and is a pure MLP forward/backward, "
                         "which Inductor can fuse; the wrapper is functional so parameter "
                         "handling and checkpoint writing are untouched. NOTE: on this machine "
                         "Inductor cannot compile at all -- its generated C++ includes <omp.h> "
                         "and the CommandLineTools toolchain has no OpenMP headers -- so the "
                         "option falls back to eager here and is only useful where libomp is "
                         "installed.")
    ap.add_argument("--zero-features", default="",
                    help="zero a feature slice (e.g. '737:753') in every training batch, as the "
                         "control arm for a new feature block. WARNING (round 32): this zeroes the "
                         "*training* batches only. Inference still feeds the real values, so the "
                         "first-layer weights for that slice never receive a gradient (they stay at "
                         "their random initialisation) and are then multiplied by real inputs -- a "
                         "train/inference mismatch, not a clean control. For a clean control, "
                         "rewrite the dataset without the slice instead and train with the "
                         "narrower build (see docs/TRAINING.md section 43).")
    ap.add_argument("--hidden", default="512,512", help="hidden widths, only for a fresh net")
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--threads", type=int, default=0, help="torch threads (0 = default)")
    ap.add_argument("--val-frac", type=float, default=0.05)
    ap.add_argument("--lr-decay", type=float, default=0.7, help="per-epoch decay after epoch 1")
    ap.add_argument("--note", default="", help="free-form note stored in the checkpoint")
    ap.add_argument("--ppo", action="store_true",
                    help="clip the policy update against the checkpoint being improved "
                         "(a trust region). Without it, any step large enough to change "
                         "behaviour is large enough to collapse the policy")
    ap.add_argument("--clip", type=float, default=0.2, help="PPO ratio clip")
    ap.add_argument("--kl-coef", type=float, default=0.0,
                    help="extra penalty on the KL to the reference policy")
    ap.add_argument("--save-epochs", action="store_true",
                    help="also write a checkpoint after every epoch (used to measure how "
                         "much fitting the teacher actually helps)")
    args = ap.parse_args()

    if args.threads > 0:
        torch.set_num_threads(args.threads)
    torch.manual_seed(args.seed)
    rng = np.random.default_rng(args.seed)

    datasets = mmjdata.load_all(args.data, args.max_records)
    if args.zero_features:
        lo, _, hi = args.zero_features.partition(":")
        zs = slice(int(lo), int(hi) if hi else None)
        for ds in datasets:
            ds.zero_slice = zs
        print(f"control arm: features {zs.start}:{zs.stop} are forced to zero")
    if not datasets:
        print("no data found")
        return 1
    feature_dim = datasets[0].feature_dim
    policy_dim = datasets[0].policy_dim
    total = sum(len(d) for d in datasets)
    kinds = {d.kind: sum(len(x) for x in datasets if x.kind == d.kind) for d in datasets}
    print(f"loaded {total} decisions from {len(datasets)} file(s): {kinds}")
    print(f"feature_dim={feature_dim} policy_dim={policy_dim}")

    # Hold out a slice of every dataset for validation.
    train_sets, val_sets = [], []
    for ds in datasets:
        cut = int(len(ds) * (1.0 - args.val_frac))
        train_sets.append(_slice(ds, 0, cut))
        val_sets.append(_slice(ds, cut, len(ds)))

    reference = None
    if args.init:
        net, header = load_checkpoint(Path(args.init))
        print(f"continued from {args.init} (trained at {header.get('info', {}).get('step', '?')})")
        if args.ppo:
            # The reference policy defines the trust region: the batch was
            # generated by it, so it is the behaviour policy PPO needs.
            reference, _ = load_checkpoint(Path(args.init))
            reference.eval()
            for p in reference.parameters():
                p.requires_grad_(False)
            print(f"ppo enabled: clip {args.clip}, kl penalty {args.kl_coef}")
    else:
        hidden = [int(x) for x in args.hidden.split(",") if x.strip()]
        net = build_net(feature_dim, policy_dim, hidden, decomposed=args.decomposed_value)
        print(
            f"fresh network with hidden {hidden}"
            + (" (decomposed value)" if args.decomposed_value else "")
        )

    params = sum(p.numel() for p in net.parameters())
    print(f"parameters: {params:,}")

    body_forward = net["body"]
    if args.compile:
        try:
            # A failed compile is raised on the first forward, not here, so the
            # fallback has to be dynamo's own: without it an unsupported toolchain
            # (this one has no OpenMP headers) crashes the run instead of quietly
            # continuing eager.
            import torch._dynamo as dynamo

            dynamo.config.suppress_errors = True
            body_forward = torch.compile(net["body"])
            print("torch.compile: body wrapped (falls back to eager if Inductor cannot build)")
        except Exception as exc:  # pragma: no cover - depends on the torch build
            print(f"torch.compile unavailable, continuing eager: {type(exc).__name__}: {exc}")

    opt = torch.optim.Adam(net.parameters(), lr=args.lr)
    train_batcher = Batcher(train_sets, args.batch_size, rng)
    val_batcher = Batcher(val_sets, args.batch_size, np.random.default_rng(args.seed + 1))

    decomposed = bool(args.decomposed_value) and "p_win" in net
    if args.decomposed_value and not decomposed:
        print("warning: --decomposed-value needs a fresh network (or a checkpoint with the heads)")
    history = []
    for epoch in range(1, args.epochs + 1):
        start = time.time()
        stats = {"policy": 0.0, "value": 0.0, "imitation": 0, "rl": 0, "n": 0,
                 "clip_frac": 0.0, "kl": 0.0, "ties": 0}
        for idx in train_batcher.epoch():
            opt.zero_grad(set_to_none=True)
            policy_loss = None
            value_loss = None
            entropy_sum = None
            count = 0
            for kind, feats, masks, actions, returns, parts in train_batcher.fetch(idx):
                x = torch.from_numpy(feats)
                mask = torch.from_numpy(masks).to(torch.bool)
                a = torch.from_numpy(actions)
                ret = torch.from_numpy(returns) * args.return_scale
                body = body_forward(x)
                logits = net["policy"](body)
                masked = logits.masked_fill(~mask, -1e9)
                logp = F.log_softmax(masked, dim=1)
                nll = -logp.gather(1, a.unsqueeze(1)).squeeze(1)
                pred = net["value"](body).squeeze(1)
                vloss = value_error(pred, ret, args.value_loss)
                if decomposed and parts is not None:
                    dloss = decomposed_value_loss(net, body, parts, args.return_scale)
                    vloss = vloss + dloss
                if kind == "imitation":
                    if args.tie_aware:
                        targets = tie_aware_targets(x, a, mask)
                        if targets is None:
                            ploss = nll.mean()
                        else:
                            # Rows that are a genuine tie are no longer pushed
                            # towards one arbitrary member of the tie set.
                            ploss = -(targets * logp).sum(dim=1).mean()
                            stats["ties"] += int(
                                ((targets > 0).sum(dim=1) > 1).sum()
                            )
                    elif args.weight_calls != 1.0 or args.weight_push != 1.0:
                        # Two rare-or-hard label classes, weighted from the features
                        # rather than from the action space:
                        #  * calls: the teacher's chi/pon (rounds 35-36);
                        #  * "pushes": a discard the encoder itself rates as
                        #    dangerous, i.e. the decisions taken while a threat is
                        #    live. The model pushes about as often as the teacher
                        #    (93% overlap) but picks a different tile 14% of the
                        #    time, so this knob asks whether the teacher's exact
                        #    choice in those spots is worth more weight.
                        is_call = ((a >= 105) & (a <= 108)).to(nll.dtype)
                        disc = (a >= 0) & (a <= 33)
                        danger = torch.zeros_like(nll)
                        if disc.any():
                            danger[disc] = x[disc, DANGER.start + a[disc]]
                        is_push = (disc & (danger > args.push_threshold)).to(nll.dtype)
                        w = (1.0
                             + (args.weight_calls - 1.0) * is_call
                             + (args.weight_push - 1.0) * is_push)
                        ploss = (nll * w).sum() / w.sum().clamp(min=1e-6)
                    else:
                        ploss = nll.mean()
                    stats["imitation"] += len(a)
                else:
                    # Advantage-weighted policy gradient (A2C with MC returns).
                    adv = (ret - pred.detach())
                    if args.reward == "win" and parts is not None:
                        # Binary outcome: `parts[0] > 0` means this seat won the hand.
                        # The value head then predicts a probability, and its error is
                        # the honest kind of error (a learnable event), not the point
                        # swing's luck.
                        won = torch.from_numpy((parts[0] > 0).astype("float32"))
                        with torch.no_grad():
                            pass
                        adv = won - torch.sigmoid(pred.detach())
                        if adv.numel() > 1:
                            adv = (adv - adv.mean()) / (adv.std() + 1e-6)
                        vloss = F.binary_cross_entropy_with_logits(pred, won)
                    if adv.numel() > 1:
                        adv = (adv - adv.mean()) / (adv.std() + 1e-6)
                    if reference is not None:
                        with torch.no_grad():
                            old_body = reference["body"](x)
                            old_logits = reference["policy"](old_body)
                            old_logp = F.log_softmax(
                                old_logits.masked_fill(~mask, -1e9), dim=1
                            ).gather(1, a.unsqueeze(1)).squeeze(1)
                        new_logp = logp.gather(1, a.unsqueeze(1)).squeeze(1)
                        ratio = (new_logp - old_logp).exp()
                        clipped = torch.clamp(ratio, 1.0 - args.clip, 1.0 + args.clip)
                        ploss = -torch.min(ratio * adv, clipped * adv).mean()
                        if args.kl_coef > 0:
                            ploss = ploss + args.kl_coef * (old_logp - new_logp).mean()
                        stats["clip_frac"] += float(
                            ((ratio - 1.0).abs() > args.clip).float().mean()
                        )
                        stats["kl"] += abs(float((old_logp - new_logp).mean()))
                    else:
                        ploss = (nll * adv).mean()
                    stats["rl"] += len(a)
                # Entropy of the masked distribution, for exploration.
                probs = logp.exp()
                entropy = -(probs * logp).sum(dim=1).mean()
                policy_loss = ploss if policy_loss is None else policy_loss + ploss
                value_loss = vloss if value_loss is None else value_loss + vloss
                entropy_sum = entropy if entropy_sum is None else entropy_sum + entropy
                count += 1
                stats["n"] += len(a)
            if count == 0:
                continue
            policy_loss = policy_loss / count
            value_loss = value_loss / count
            entropy = entropy_sum / count
            loss = policy_loss + args.value_coef * value_loss - args.entropy_coef * entropy
            loss.backward()
            torch.nn.utils.clip_grad_norm_(net.parameters(), 5.0)
            opt.step()
            stats["policy"] += float(policy_loss) * count
            stats["value"] += float(value_loss) * count
        val = evaluate(
            net,
            val_batcher,
            batches=40,
            value_coef=args.value_coef,
            rng=rng,
            return_scale=args.return_scale,
            value_loss_kind=args.value_loss,
            body_forward=body_forward,
        )
        secs = time.time() - start
        batches = max(1, len(train_batcher))
        print(
            f"epoch {epoch}/{args.epochs}  policy_loss={stats['policy'] / batches:.4f}  "
            f"value_loss={stats['value'] / batches:.4f}  "
            f"val_acc={val['policy_acc']:.3f}  val_r2={val['value_r2']:.3f}  "
            f"pred_sd/target_sd={val['pred_var'] ** 0.5:.2f}/{val['target_var'] ** 0.5:.2f}  "
            + (
                f"dec_r2={val['decomposed_r2']:.3f} "
                f"dec_sd={val['decomposed_pred_var'] ** 0.5:.2f}  "
                if args.decomposed_value
                else ""
            )
            + (
                f"clip={stats['clip_frac'] / max(1, batches):.2f}  "
                f"kl={stats['kl'] / max(1, batches):.3f}  "
                if reference is not None
                else ""
            )
            + f"({stats['imitation']} imitation / {stats['rl']} self-play decisions, {secs:.0f}s)"
        )
        history.append(
            {
                "epoch": epoch,
                "val_policy_acc": round(val["policy_acc"], 4),
                "val_value_mse": round(val["value_mse"], 4),
                "val_value_r2": round(val["value_r2"], 4),
                "train_policy_loss": round(stats["policy"] / max(1, batches), 4),
                "train_value_loss": round(stats["value"] / max(1, batches), 4),
            }
        )
        if args.save_epochs:
            per_epoch = Path(args.out).with_name(
                f"{Path(args.out).stem}-e{epoch}{Path(args.out).suffix}"
            )
            save_checkpoint(
                net,
                feature_dim,
                policy_dim,
                per_epoch,
                {"step": f"{args.note}-epoch{epoch}", "epoch": epoch,
                 "val_policy_acc": round(val["policy_acc"], 4)},
            )
            print(f"  wrote {per_epoch.name} (val_acc {val['policy_acc']:.3f})")
        if epoch < args.epochs and args.lr_decay != 1.0:
            for g in opt.param_groups:
                g["lr"] *= args.lr_decay

    info = {
        "step": args.note or "",
        "return_scale": args.return_scale,
        "value_loss": args.value_loss,
        "records": total,
        "kinds": kinds,
        "history": history,
        "params": params,
        "hidden": [m.out_features for m in net["body"] if isinstance(m, nn.Linear)],
        "trained_at": time.strftime("%Y-%m-%d %H:%M:%S"),
    }
    save_checkpoint(net, feature_dim, policy_dim, Path(args.out), info)
    print(f"saved {args.out}")
    return 0


def _slice(ds: mmjdata.Dataset, lo: int, hi: int) -> mmjdata.Dataset:
    """A view over a contiguous slice of a dataset."""
    import copy

    out = copy.copy(ds)
    out.count = hi - lo
    out.features = ds.features[lo:hi]
    out.masks = ds.masks[lo:hi]
    out.actions = ds.actions[lo:hi]
    out.returns = ds.returns[lo:hi]
    out.seats = ds.seats[lo:hi]
    return out


if __name__ == "__main__":
    raise SystemExit(main())
