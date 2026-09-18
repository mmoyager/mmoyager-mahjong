#!/usr/bin/env python3
"""Long-running training loop for the mahjong bots.

One iteration is:

1. **bootstrap** (first run only) — record the tile-efficiency baseline playing
   itself, and imitate it. This gives a policy that is already reasonable,
   without any human game records.
2. **self-play** — play the current checkpoint against itself and against the
   baseline, recording every decision of the learning seats with its
   Monte-Carlo return.
3. **train** — one pass over the fresh data plus a replay window of recent
   iterations, updating the policy and value heads.
4. **evaluate** — play the new checkpoint against the baseline and against the
   previous checkpoint. A checkpoint that fails to beat the best one is
   discarded, so the loop never drifts downwards.

State lives in `data/training_state.json`, so the loop can be stopped and
resumed at any time (`Ctrl-C` is safe: the state is written after every step).

    python trainer/loop.py --iterations 20
    python trainer/loop.py --forever --eval-games 200
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BIN_SELFPLAY = ROOT / "target" / "release" / "mmj-selfplay"
BIN_EVAL = ROOT / "target" / "release" / "mmj-eval"
PYTHON = ROOT / ".venv" / "bin" / "python"
STATE_PATH = ROOT / "data" / "training_state.json"
LOG_PATH = ROOT / "data" / "logs" / "train.log"

_stop = False

# Fixed evaluation seeds: comparing checkpoints against each other is only
# meaningful when they face the same deals.
EVAL_SEED_ABS = 4242
EVAL_SEED_H2H = 5150


def log(message: str) -> None:
    stamp = time.strftime("%Y-%m-%d %H:%M:%S")
    line = f"[{stamp}] {message}"
    print(line, flush=True)
    LOG_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(LOG_PATH, "a") as f:
        f.write(line + "\n")


def request_stop(signum, frame) -> None:  # noqa: ARG001
    global _stop
    _stop = True
    log("stop requested — finishing the current step")


def run(cmd: list[str], env: dict | None = None, quiet: bool = False) -> int:
    full_env = dict(os.environ)
    # Keep child output unbuffered so the log shows progress live.
    full_env["PYTHONUNBUFFERED"] = "1"
    if env:
        full_env.update(env)
    if quiet:
        result = subprocess.run(cmd, env=full_env, capture_output=True, text=True)
        if result.returncode != 0:
            log(f"command failed: {' '.join(cmd)}")
            log(result.stdout[-2000:])
            log(result.stderr[-4000:])
        return result.returncode
    result = subprocess.run(cmd, env=full_env)
    return result.returncode


def load_state() -> dict:
    if STATE_PATH.exists():
        return json.loads(STATE_PATH.read_text())
    return {
        "iteration": 0,
        "base": None,
        "best": None,
        "best_score": None,
        "history": [],
        "data_files": [],
        "checkpoints": [],
    }


def save_state(state: dict) -> None:
    STATE_PATH.parent.mkdir(parents=True, exist_ok=True)
    STATE_PATH.write_text(json.dumps(state, indent=2))


def ckpt_path(iteration: int) -> Path:
    return ROOT / "data" / "checkpoints" / f"ck-{iteration:04d}.bin"


def evaluate(
    a: str, b: str, games: int, hanchan: bool, seed: int, game_offset: int = 0
) -> dict | None:
    """Pair a checkpoint against a baseline.

    `game_offset` shifts the deal stream so a second call continues where the
    first stopped: game `i` of a run started at `seed + offset` is the same deal
    as game `offset + i` of a run started at `seed`.
    """
    cmd = [
        str(BIN_EVAL),
        "--a",
        a,
        "--b",
        b,
        "--games",
        str(games),
        "--seed",
        str(seed + game_offset),
        "--json",
    ]
    if hanchan:
        cmd.append("--hanchan")
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        log(f"eval failed: {result.stderr[-1000:]}")
        return None
    for line in result.stdout.splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                continue
    return None


def trim(state: dict, args, bootstrap: Path, iteration: int) -> None:
    """Delete the self-play files and checkpoints an iteration has outlived.

    Kept as a function because both the normal exit and the screened-out exit
    need it: a screened-out iteration produces no checkpoint, but it does produce
    self-play data, and the replay window has to keep moving.
    """
    # Trim expired self-play files: keep the replay window plus one spare
    # iteration, so a resumed run still has its mixture.
    keep_files = set(state["data_files"][-(max(2, args.replay_window * 2) + 2):])
    keep_files.add(str(bootstrap))
    state["data_files"] = [f for f in state["data_files"] if f in keep_files]
    for path in sorted((ROOT / "data" / "selfplay").glob("sp-*.bin")):
        if str(path) not in keep_files:
            path.unlink(missing_ok=True)

    # Trim old checkpoint files to save disk (keep the best, the base, the
    # last 3 iterations, and every file marked as a reference).
    #
    # Reference checkpoints are protected by name because they are the
    # yardsticks that later analyses compare against: an earlier version of
    # this cleanup deleted `ck-best.bin` the moment the best pointer moved to
    # its successor, which silently invalidated every comparison that still
    # referred to it.
    keep = (
        {Path(state["best"]).name, Path(state["base"]).name}
        | {ckpt_path(i).name for i in range(max(0, iteration - 2), iteration + 1)}
        | set(state.get("protected", []))
    )
    for path in sorted((ROOT / "data" / "checkpoints").glob("ck-*.bin")):
        if path.name not in keep and not path.name.startswith("ck-ref-"):
            path.unlink(missing_ok=True)


def main() -> int:
    ap = argparse.ArgumentParser(description="run the self-play training loop")
    ap.add_argument("--iterations", type=int, default=10, help="iterations to run")
    ap.add_argument("--forever", action="store_true", help="keep iterating until stopped")
    ap.add_argument("--games-per-iter", type=int, default=3000, help="self-play games per iteration")
    ap.add_argument("--imitate-games", type=int, default=6000, help="baseline games for the bootstrap")
    ap.add_argument("--bootstrap-data", default="data/selfplay/im-v8.bin",
                    help="imitation dataset used to bootstrap (reused when present)")
    ap.add_argument("--bootstrap-epochs", type=int, default=4)
    ap.add_argument("--bootstrap-records", type=int, default=2_400_000,
                    help="how many imitation records the bootstrap may train on")
    ap.add_argument("--rebootstrap", action="store_true", help="regenerate imitation data")
    ap.add_argument("--primary-spec", default="efficiency",
                    help="the opponent the acceptance decision is made against. Round 13 "
                         "showed that optimising the margin against the weakest baseline "
                         "produces a specialist: the served checkpoint beat it by 1386 while "
                         "losing to the improved rule bot by 493. Point this at the stronger "
                         "bot to make the search target general strength instead.")
    ap.add_argument("--yardstick-league", default="",
                    help="comma-separated extra opponents that a candidate must not get "
                         "worse against (e.g. 'efficiency,efficiency-v3'). Round 23 showed "
                         "one yardstick can improve while another degrades sharply (+52 "
                         "against the strong bot, -289 against the weak one), so a candidate "
                         "is now judged across a league.")
    ap.add_argument("--yardstick2", default="",
                    help="a second opponent to guard against, e.g. efficiency-v2. Measured "
                         "result: a checkpoint that beats the frozen baseline by 1386 loses "
                         "to the improved rule bot by 493, so a single weak yardstick rewards "
                         "style that does not transfer. When set, a candidate that degrades "
                         "play against this opponent is rejected however well it scores "
                         "against the first.")
    ap.add_argument("--early-stop-slack", type=float, default=100.0,
                    help="evaluate candidates in two halves: if the first half is more than "
                         "this far below the acceptance bar, stop and reject without spending "
                         "the second half or the head-to-head eval. Candidates near the bar "
                         "still get the full measurement.")
    ap.add_argument("--accept-on", choices=["direct", "absolute"], default="direct",
                    help="what the accept decision is measured on. 'direct' (round 30) plays the "
                         "candidate against the current best at the same tables: one evaluation "
                         "instead of two (the incumbent's re-measurement is no longer needed, as "
                         "it is sitting in the same games), and the scale is 'margin over the "
                         "incumbent', so the bar is the accept margin itself. 'absolute' is the "
                         "previous protocol: both the candidate and the incumbent measured "
                         "against --primary-spec, at twice the games. Measured spreads at 9600 "
                         "games are comparable (direct: +87.7 / -0.3 on two seeds; the yardstick "
                         "margin: 60-95), so the direct form buys the same precision for half the "
                         "evaluation time. Either way the yardstick league still guards a "
                         "promotion, and --abs-every keeps the absolute trend on record.")
    ap.add_argument("--abs-every", type=int, default=5,
                    help="with --accept-on direct, re-measure the incumbent against "
                         "--primary-spec every N iterations so the absolute trend and the "
                         "league references stay anchored (0 disables)")
    ap.add_argument("--accept-margin", type=float, default=0.0,
                    help="how far above the historical best a candidate must score before it "
                         "is promoted. With one replica per candidate the measured spread of "
                         "a single run is 200-400 points, so a zero margin promotes luck "
                         "(about half of all zero-effect candidates).")
    ap.add_argument("--replicate-runs", type=int, default=1,
                    help="train this many independent replicas of the candidate recipe and "
                         "judge it by their mean score. Two identical recipes were measured "
                         "373 points apart (round 10), so judging from one run mostly "
                         "measures that run's luck.")
    ap.add_argument("--tie-aware", action="store_true",
                    help="pass --tie-aware to the trainer (imitate the set of equivalent "
                         "actions rather than the teacher's arbitrary tie-break)")
    ap.add_argument("--confirm-runs", type=int, default=2,
                    help="a candidate that beats the best is re-run once with a fresh data seed "
                         "before it is promoted. Three evaluation seeds only verify the "
                         "*measurement*; the +193 dose of round 8 did not reproduce when the "
                         "recipe was re-run with different deals (it came out +-0), so a single "
                         "training run's margin is not evidence on its own.")
    ap.add_argument("--search-modes", action="store_true",
                    help="cycle a small, evidence-motivated set of training recipes instead "
                         "of repeating one. Every iteration is still judged by the paired "
                         "evaluation and rolled back on a regression, so this can only spend "
                         "time, never make the served checkpoint worse.")
    ap.add_argument("--dagger-region-shanten", type=int, default=-1,
                    help="with --dagger-loop, label only own-turn discards at this many "
                         "shanten or worse. The student's measured deficit against its "
                         "teacher is concentrated there, and a two-epoch fine-tune on such "
                         "rows alone was worth about +190 points against the fixed baseline "
                         "(three seeds, matched pairs).")
    ap.add_argument("--dagger-loop", action="store_true",
                    help="run DAgger iterations: the current checkpoint plays, the teacher "
                         "labels every decision the learner reaches, and the iteration trains "
                         "by imitation only. Measured: the reinforcement-learning half of the "
                         "mixture is neutral at best (4800-game paired eval, two independent "
                         "measures, both about -100 points).")
    ap.add_argument("--eval-games", type=int, default=4800, help="games per evaluation; paired fixed seeds, so more games means a tighter accept decision (4800 games gives a standard error near 100 points)")
    ap.add_argument("--epochs", type=int, default=1, help="training epochs per iteration")
    ap.add_argument("--entropy-coef", type=float, default=0.005)
    ap.add_argument("--value-coef", type=float, default=0.25)
    ap.add_argument("--hidden", default="768,768")
    ap.add_argument("--lr", type=float, default=2e-5, help="learning rate for self-play iterations")
    ap.add_argument("--ppo", action="store_true", help="use a clipped (trust region) update")
    ap.add_argument("--clip", type=float, default=0.2)
    ap.add_argument("--kl-coef", type=float, default=0.5)
    ap.add_argument("--mixed-fraction", type=float, default=0.66,
                    help="share of self-play games played against the baseline rather than mirrored")
    ap.add_argument("--bootstrap-lr", type=float, default=1e-3)
    ap.add_argument("--no-anchor", action="store_true",
                    help="do not keep the bootstrap imitation data in the mixture")
    ap.add_argument("--replay-window", type=int, default=2, help="iterations of recent self-play kept in the mix")
    ap.add_argument("--max-records", type=int, default=900_000, help="cap per data file when training")
    ap.add_argument("--threads", type=int, default=0,
                    help="torch threads for the training step (0 = torch's default, which is "
                         "every logical core). Round 29 measured the training step on this "
                         "machine (4 physical cores, 8 logical) with the loop paused: a 300k-row "
                         "epoch took 12/11 s with 8 threads and 9/10 s with 4, i.e. "
                         "hyperthreading costs ~20 percent here. The loop has always been "
                         "training-bound (650-720 s of a 1050-1200 s iteration), so this is "
                         "passed as --threads 4 when the machine has 4 physical cores.")
    ap.add_argument("--hanchan", action="store_true", help="train on full hanchan")
    ap.add_argument("--epsilon", type=float, default=0.03, help="exploration rate for self-play")
    ap.add_argument("--resume", action="store_true", help="continue from data/training_state.json")
    ap.add_argument("--eval-only", action="store_true", help="skip training, just re-measure the best")
    args = ap.parse_args()

    for binary in (BIN_SELFPLAY, BIN_EVAL):
        if not binary.exists():
            log(f"missing {binary}; run: cargo build --release")
            return 1

    signal.signal(signal.SIGINT, request_stop)
    signal.signal(signal.SIGTERM, request_stop)

    state = load_state() if args.resume else {
        "iteration": 0,
        "base": None,
        "best": None,
        "best_score": None,
        "history": [],
        "data_files": [],
        "checkpoints": [],
    }
    job_env = {}
    if args.threads > 0:
        job_env["RAYON_NUM_THREADS"] = str(args.threads)
    log_env = ["--threads", str(args.threads)] if args.threads > 0 else []

    length_flag = ["--hanchan"] if args.hanchan else []

    # ---------------------------------------------------------------- bootstrap
    bootstrap = ROOT / args.bootstrap_data
    if state["best"] is None and not args.eval_only:
        if args.rebootstrap or not bootstrap.exists() or bootstrap.stat().st_size < 10_000_000:
            log(f"bootstrap: recording {args.imitate_games} baseline games")
            if run(
                [
                    str(BIN_SELFPLAY), "imitate",
                    "--games", str(args.imitate_games),
                    "--out", str(bootstrap),
                    "--seed", "11",
                    "--batch", "384",
                    *length_flag,
                ],
                env=job_env, quiet=True,
            ) != 0:
                return 1
        else:
            log(f"bootstrap: reusing {bootstrap.name} ({bootstrap.stat().st_size // 1_000_000} MB)")
        target = ckpt_path(0)
        log("bootstrap: imitating the baseline")
        if run(
            [
                str(PYTHON),
                str(ROOT / "python" / "trainer" / "train.py"),
                "--data",
                str(bootstrap),
                "--out",
                str(target),
                "--epochs", str(args.bootstrap_epochs),
                "--hidden", args.hidden,
                "--lr", str(args.bootstrap_lr),
                "--lr-decay", "0.8",
                "--max-records", str(args.bootstrap_records),
                "--value-coef", "0.5",
                "--return-scale", "0.25",
                *(["--ppo", "--clip", str(args.clip), "--kl-coef", str(args.kl_coef)]
                  if args.ppo else []),
                "--note", "bootstrap-imitation",
                *log_env,
            ],
            quiet=False,
        ) != 0:
            return 1
        state["checkpoints"].append(str(target))
        state["base"] = str(target)
        state["best"] = str(target)
        state["data_files"] = [str(bootstrap)]
        state["iteration"] = 0
        # Measure the bootstrap so later iterations are compared against a real
        # number rather than a guess.
        boot = evaluate(str(target), "efficiency", args.eval_games, args.hanchan, EVAL_SEED_ABS)
        if boot:
            state["best_score"] = boot["avg_a"]
            log(
                f"bootstrap vs efficiency: avg {boot['avg_a']:.0f} "
                f"(baseline {boot['avg_b']:.0f}), hands {boot['wins_a']}:{boot['wins_b']}"
            )
        state["history"].append(
            {"iteration": 0, "stage": "bootstrap", "checkpoint": str(target), "vs_efficiency": boot}
        )
        save_state(state)
        log(f"bootstrap done -> {target.name}")

    # A stored best_score is only comparable with a score measured the same way.
    # Mixing protocols is not hypothetical: a 9600-game score was once compared
    # against a 4800-game best_score and a checkpoint that had just *regressed*
    # passed the check by three points because its number looked bigger.
    if (
        not args.eval_only
        and state.get("best")
        and Path(state["best"]).exists()
        and state.get("eval_games") != args.eval_games
    ):
        log(
            f"re-measuring the stored best at {args.eval_games} games "
            f"(stored score came from {state.get('eval_games')} games)"
        )
        fresh = evaluate(state["best"], "efficiency", args.eval_games, args.hanchan, EVAL_SEED_ABS)
        if fresh:
            log(f"  {Path(state['best']).name}: avg {fresh['avg_a']:.1f} (was {state.get('best_score')})")
            state["best_score"] = fresh["avg_a"]
            state["eval_games"] = args.eval_games
            save_state(state)

    if args.eval_only:
        log("eval-only mode")
        result = evaluate(state["best"], "efficiency", args.eval_games, args.hanchan, 999)
        if result:
            log(f"best {Path(state['best']).name} vs efficiency: {json.dumps(result)}")
        return 0

    # ------------------------------------------------------------- main loop
    iteration = state["iteration"]
    state["eval_games"] = args.eval_games
    while True:
        if _stop:
            break
        if not args.forever and iteration >= args.iterations:
            break
        iteration += 1
        started = time.time()
        current = Path(state.get("base") or state["best"])
        fresh = ROOT / "data" / "selfplay" / f"sp-{iteration:04d}.bin"
        fresh_mixed = ROOT / "data" / "selfplay" / f"sp-{iteration:04d}-mixed.bin"
        target = ckpt_path(iteration)

        # A recipe is (which decisions the teacher labels) x (how hard we fit
        # them). The first dose of the region recipe was worth about +190 points;
        # later doses of the *same* recipe regress, so the loop cycles through a
        # few variants and lets the accept rule pick the ones that work.
        # (name, shanten filter, call windows, games, lr, epochs, keep anchor)
        #
        # "region-noanchor" is the exact shape of the one dose that measured
        # +193, and it is the one shape the loop had never actually run: the
        # loop always keeps the 900k imitation anchor in the mixture, so the
        # earlier "a second dose regresses" result was confounded with that.
        RECIPES = [
            # The shape that measured +287 against the stronger opponent after the
            # teacher-ranking feature landed: labels on *every* decision the
            # current policy reaches, no imitation anchor, two gentle epochs.
            ("full-noanchor", None, False, 6000, 5e-5, 2, False),
            ("region-noanchor", 2, False, 6000, 5e-5, 2, False),
            ("region-sh2", 2, False, 3000, 5e-5, 2, True),
            ("region-sh3", 3, False, 6000, 5e-5, 2, False),
            ("full-dagger", None, False, 3000, 5e-5, 2, True),
            ("region-sh2-calls", 2, True, 3000, 3e-5, 2, True),
        ]
        recipe = RECIPES[iteration % len(RECIPES)] if (args.dagger_loop and args.search_modes) else None
        if recipe:
            name, shanten, calls, games_per_iter, lr, epochs, keep_anchor = recipe
            log(f"iter {iteration}: recipe {name} (shanten={shanten}, calls={calls}, "
                f"games={games_per_iter}, lr={lr:g}, epochs={epochs}, anchor={keep_anchor})")
        else:
            name, shanten, calls = "fixed", args.dagger_region_shanten, False
            games_per_iter, lr, epochs, keep_anchor = (
                args.games_per_iter, args.lr, args.epochs, True
            )

        # Under --dagger-loop the recorded target for every learner decision is
        # the teacher's action on the state the learner reached, so the data is
        # imitation data and no policy-gradient row enters training.
        dagger_flags: list[str] = (
            ["--dagger", "--label-kind", "imitation"] if args.dagger_loop else []
        )
        if args.dagger_loop and shanten is not None and shanten >= 0:
            dagger_flags += ["--dagger-shanten", str(shanten)]
        if args.dagger_loop and calls:
            dagger_flags += ["--dagger-calls"]

        # ---- 1. self-play -------------------------------------------------
        against = max(1, int(games_per_iter * args.mixed_fraction))
        half = max(1, games_per_iter - against)
        log(
            f"iter {iteration}: self-play {half} games (mirror) + {against} games (vs baseline)"
        )
        if run(
            [
                str(BIN_SELFPLAY), "generate",
                "--games", str(half),
                "--out", str(fresh),
                "--checkpoint", str(current),
                "--seats", "learner,learner,learner,learner",
                "--epsilon", str(args.epsilon),
                "--seed", str(1000 + iteration * 7 + int(state.get("confirm_offset", 0))),
                "--batch", "256",
                *length_flag,
                *dagger_flags,
                # Under --dagger-loop the labels come from the rule teacher, so
                # it must be the *improved* one: labelling with the frozen v1
                # baseline would pull the student back towards it (measured:
                # a v1-labelled mixture is worth about -1000 points).
                *(["--teacher-v2"] if args.dagger_loop else []),
            ],
            env=job_env, quiet=True,
        ) != 0:
            log("self-play failed; stopping")
            break
        if run(
            [
                str(BIN_SELFPLAY), "generate",
                "--games", str(against),
                "--out", str(fresh_mixed),
                "--checkpoint", str(current),
                "--seats", "learner,efficiency,learner,efficiency",
                # Spar against the *improved* teacher: the learner now outgrows the
                # frozen baseline, and practising against a weaker opponent teaches
                # nothing. Evaluation still uses the frozen one for comparability.
                "--teacher-v2",
                "--epsilon", str(args.epsilon),
                "--seed", str(5000 + iteration * 13 + int(state.get("confirm_offset", 0))),
                "--batch", "256",
                *length_flag,
                *dagger_flags,
            ],
            env=job_env, quiet=True,
        ) != 0:
            log("mixed self-play failed; stopping")
            break

        # ---- 2. train -----------------------------------------------------
        state["data_files"].append(str(fresh))
        state["data_files"].append(str(fresh_mixed))
        window = state["data_files"][-max(2, args.replay_window * 2):]
        data_args: list[str] = []
        # The bootstrap file stays in the mixture as a weak anchor on "sane
        # mahjong"; without it the first RL steps can collapse the policy.
        if not args.no_anchor and keep_anchor and bootstrap.exists():
            data_args.append(str(bootstrap))
        for path in window:
            if Path(path).exists() and str(path) not in data_args:
                data_args.append(str(path))
        log(
            f"iter {iteration}: training on {len(data_args)} data file(s)"
            + (" [dagger/imitation]" if args.dagger_loop else " [imitation + RL]")
            + (f" x{args.replicate_runs} replicas" if args.replicate_runs > 1 else "")
        )
        # Independent replicas of the same recipe. Two identical recipes were
        # measured 373 points apart (round 10), so a candidate judged from a
        # single run is mostly measuring the run's luck.
        replicas: list[Path] = [
            target
            if args.replicate_runs <= 1
            else target.with_name(f"{target.stem}-r{rep}{target.suffix}")
            for rep in range(max(1, args.replicate_runs))
        ]
        failed = False
        for rep, rep_target in enumerate(replicas):
            if run(
                [
                    str(PYTHON), str(ROOT / "python" / "trainer" / "train.py"),
                    "--data", *data_args,
                    "--out", str(rep_target),
                    "--init", str(current),
                    "--seed", str(7 + rep * 1009),
                    "--epochs", str(epochs),
                    "--lr", str(lr),
                    "--entropy-coef", str(args.entropy_coef),
                    "--value-coef", str(args.value_coef),
                    "--return-scale", "0.25",
                    *(["--ppo", "--clip", str(args.clip), "--kl-coef", str(args.kl_coef)]
                      if args.ppo else []),
                    "--max-records", str(args.max_records),
                    "--note", f"selfplay-iteration-{iteration}-{name}-r{rep}",
                    *(["--tie-aware"] if args.tie_aware else []),
                    *log_env,
                ],
                quiet=False,
            ) != 0:
                log("training failed; stopping")
                failed = True
                break
            state["checkpoints"].append(str(rep_target))
        if failed:
            break

        # ---- 3. evaluate --------------------------------------------------
        score = None
        if _stop:
            log("stopped before evaluation")
            state["iteration"] = iteration
            save_state(state)
            break
        guard_ok = True
        margin2 = None

        direct = args.accept_on == "direct" and bool(state.get("best"))
        primary_opponent = str(state["best"]) if direct else args.primary_spec
        opponent_label = (
            f"incumbent {Path(state['best']).name}" if direct else args.primary_spec
        )

        def primary_score(r: dict) -> float:
            """The number the accept decision is made on.

            In `direct` mode the opponent sits at the same tables, so the score is
            the *margin* (the incumbent's own margin is 0); in `absolute` mode it
            is the candidate's average score against the yardstick and the bar is
            the incumbent's own average plus the accept margin.
            """
            return (r["avg_a"] - r["avg_b"]) if direct else r["avg_a"]

        log(
            f"iter {iteration}: evaluating {len(replicas)} replica(s) vs {opponent_label}"
        )
        # A *fixed* seed for every iteration: all checkpoints then face exactly
        # the same deals, which removes the seed-to-seed noise that made earlier
        # comparisons meaningless.
        #
        # The evaluation is split in two halves and can stop after the first one.
        # Most candidates land well below the historical best, and the bar is
        # known before the games are played, so measuring the second half (and
        # the head-to-head) for a candidate that is already 100+ points short
        # buys nothing. Candidates near the bar still get the full measurement.
        scores: list[float] = []
        results: dict[Path, dict] = {}
        best_replica: Path | None = None
        half = max(1, args.eval_games // 2)

        # Re-measure the incumbent **in this same batch** before judging anything.
        # Round 20 lost a round to comparing three fresh candidate measurements
        # against a single stale baseline measurement: the incumbent's own noise
        # does not average out, so a stored number can be 200-300 points off and
        # turn noise into an apparent gain. Measuring both here costs one extra
        # evaluation per iteration and removes the whole failure mode.
        # Which opponent the candidates are measured against, and what the bar
        # means, depends on --accept-on. In `direct` mode the opponent *is* the
        # incumbent, so the scale is "margin over the incumbent": the incumbent's
        # own score is 0 by construction, the bar is the accept margin, and the
        # separate re-measurement of the incumbent below costs nothing because it
        # is already sitting in the same games.
        incumbent = None
        if direct:
            state["best_score"] = 0.0
            bar = args.accept_margin
            anchor = (
                args.abs_every > 0
                and state.get("best")
                and iteration % args.abs_every == 0
            )
            if anchor:
                # Keep the historical scale (margin against the rule bot) on
                # record: it is what the README reports, and it is the number a
                # promotion is ultimately judged by through the league guards.
                incumbent = evaluate(
                    state["best"], args.primary_spec, args.eval_games, args.hanchan,
                    EVAL_SEED_ABS,
                )
                if incumbent:
                    anchor_margin = incumbent["avg_a"] - incumbent["avg_b"]
                    state["absolute_anchor"] = {
                        "iteration": iteration,
                        "checkpoint": state["best"],
                        "avg": incumbent["avg_a"],
                        "margin": anchor_margin,
                    }
                    # `best_score` is in "margin over the incumbent" units under
                    # --accept-on direct (the incumbent is 0 by definition); this
                    # keeps the historical absolute number alive for the README
                    # and for anyone reading the state file.
                    state["best_absolute"] = anchor_margin
                    log(
                        f"  anchor {Path(state['best']).name} vs {args.primary_spec}: "
                        f"margin {incumbent['avg_a'] - incumbent['avg_b']:+.0f} "
                        f"(absolute {incumbent['avg_a']:.0f})"
                    )
        else:
            incumbent = (
                evaluate(
                    state["best"], args.primary_spec, args.eval_games, args.hanchan, EVAL_SEED_ABS
                )
                if state.get("best")
                else None
            )
            if incumbent:
                state["best_score"] = incumbent["avg_a"]
                log(
                    f"  incumbent {Path(state['best']).name}: avg {incumbent['avg_a']:.0f} "
                    f"(opponent {incumbent['avg_b']:.0f})"
                )
            bar = (
                None
                if incumbent is None
                else incumbent["avg_a"] + args.accept_margin
            )
        for rep_target in replicas:
            r = evaluate(str(rep_target), primary_opponent, half, args.hanchan, EVAL_SEED_ABS)
            if not r:
                continue
            if (
                args.early_stop_slack > 0
                and bar is not None
                and len(replicas) == 1
                and primary_score(r) < bar - args.early_stop_slack
            ):
                log(
                    f"  first {half} games: score {primary_score(r):+.0f} (bar {bar:+.0f}) — "
                    f"more than {args.early_stop_slack:.0f} short, stopping early"
                )
                scores.append(primary_score(r))
                results[rep_target] = r
                best_replica = rep_target
                state["early_stops"] = int(state.get("early_stops", 0)) + 1
                continue
            second = evaluate(
                str(rep_target), primary_opponent, half, args.hanchan, EVAL_SEED_ABS,
                game_offset=half,
            )
            if second:
                # Both halves are the same deal stream, so their mean is exactly
                # what one full run would have reported (up to the seat rotation,
                # which is even across an even offset).
                r = dict(r)
                r["avg_a"] = (r["avg_a"] + second["avg_a"]) / 2
                r["avg_b"] = (r["avg_b"] + second["avg_b"]) / 2
                r["games"] = r.get("games", half) + second.get("games", half)
            results[rep_target] = r
            scores.append(primary_score(r))
            if best_replica is None or primary_score(r) >= max(scores):
                best_replica = rep_target
            if len(replicas) > 1:
                log(f"  {rep_target.name}: score {primary_score(r):+.0f}")
            else:
                log(
                    f"  vs {opponent_label}: margin {r['avg_a'] - r['avg_b']:+.0f} "
                    f"(avg {r['avg_a']:.0f} vs {r['avg_b']:.0f}), "
                    f"firsts {r['first_rate_a']:.3f}, hands {r['wins_a']}:{r['wins_b']}"
                )
        if scores:
            score = sum(scores) / len(scores)
            if len(replicas) > 1:
                spread = max(scores) - min(scores)
                log(
                    f"  group mean {score:.0f} over {len(scores)} replicas "
                    f"(spread {spread:.0f})"
                )
        # The lineage continues from the replica that measured best, while the
        # accept decision above uses the group mean (an unbiased estimate of the
        # recipe rather than of its luckiest draw).
        target = best_replica or target
        vs_base = results.get(target)

        # Second yardstick: a candidate may not buy points against the first
        # opponent by getting worse against a stronger one.
        guard_ok = True
        margin2 = None
        guards = [g for g in (args.yardstick2, *args.yardstick_league.split(",")) if g.strip()]
        guards = [g.strip() for g in guards if g.strip() and g.strip() != args.primary_spec]
        if score is not None and bar is not None and score >= bar:
            for guard in guards:
                paired = evaluate(
                    str(target), guard, max(200, args.eval_games // 2),
                    args.hanchan, EVAL_SEED_H2H,
                )
                if not paired:
                    continue
                margin = paired["avg_a"] - paired["avg_b"]
                known = state.setdefault("best_guards", {}).get(guard)
                ok = known is None or margin >= known - 50.0
                guard_ok = guard_ok and ok
                if margin2 is None:
                    margin2 = margin
                log(
                    f"  vs {guard}: {margin:+.0f} "
                    f"(best {known if known is None else round(known)}); "
                    + ("guard ok" if ok else "guard FAILED — rejecting")
                )
        # The head-to-head is a secondary guard; when the absolute score already
        # fails there is nothing for it to decide, so the games are not played.
        if direct or (score is not None and bar is not None and score < bar):
            # In direct mode the primary *is* the head-to-head against the best,
            # so playing it again would only spend games re-measuring the same
            # comparison.
            vs_prev = None
        else:
            vs_prev = evaluate(
                str(target), str(current), max(200, args.eval_games // 2), args.hanchan, EVAL_SEED_H2H
            )
        if vs_prev:
            log(
                f"  vs {Path(current).name}: avg {vs_prev['avg_a']:.0f} "
                f"(previous {vs_prev['avg_b']:.0f}), firsts {vs_prev['first_rate_a']:.3f}"
            )

        # ---- 4. decide what the next iteration continues from ------------
        # Progress is judged mainly by the *head-to-head* against the current
        # base: both sides play the same deals, so that comparison is far less
        # noisy than an absolute score against the baseline. `best` still tracks
        # the highest absolute score, because that is the number the user cares
        # about and what the UI loads.
        head_to_head = vs_prev["avg_a"] if vs_prev else None
        # The paired absolute score against the baseline is the ground truth;
        # beating the previous self has proven to be a misleading signal on its
        # own (a checkpoint can win the mirror match and still score worse
        # against the baseline), so it is only used as a secondary guard.
        # The margin is now one-sided: a checkpoint has to be *at least* as good
        # as the best to become the base. Accepting a slightly worse one (the old
        # -150 tolerance) let a regressed checkpoint inherit the search and the
        # next iteration then started from a worse point.
        absolute_ok = (
            score is None
            or state.get("best_score") is None
            or score >= state["best_score"] + args.accept_margin
        ) and guard_ok
        h2h_ok = head_to_head is None or head_to_head >= 25000.0 - 300.0
        confirming = int(state.get("confirm_offset", 0)) != 0
        if absolute_ok and h2h_ok:
            becomes_best = score is not None and (
                state.get("best_score") is None or score > state["best_score"]
            )
            if becomes_best and args.confirm_runs > 1 and not confirming:
                # Re-run the identical recipe on fresh deals before believing it.
                log(
                    f"  candidate scores {score:+.0f} against the best "
                    f"({state['best_score']:+.0f} on this scale); re-running the same recipe "
                    "with a fresh data seed before promoting it"
                )
                state["confirm_offset"] = 7331
                state["pending"] = {
                    "iteration": iteration,
                    "recipe": name,
                    "score": score,
                    "checkpoint": str(target),
                }
                state["base"] = str(state.get("best") or target)
                state["iteration"] = iteration
                save_state(state)
                iteration -= 1  # repeat this iteration number with the offset seed
                continue
            state["base"] = str(target)
            if becomes_best:
                state["best_score"] = score
                state["best"] = str(target)
                if not confirming and score is not None:
                    # Refresh the guard references with this checkpoint's own
                    # measurements so the league always compares like with like.
                    for guard in guards:
                        paired = evaluate(
                            str(target), guard, max(200, args.eval_games // 2),
                            args.hanchan, EVAL_SEED_H2H,
                        )
                        if paired:
                            state.setdefault("best_guards", {})[guard] = (
                                paired["avg_a"] - paired["avg_b"]
                            )
                log(f"  new best: {score:.0f}" + (" (confirmed)" if confirming else ""))
            else:
                log(
                    f"  accepted as base ({opponent_label}: "
                    f"{'n/a' if score is None else f'{score:+.0f}'})"
                )
            state.pop("pending", None)
            state["confirm_offset"] = 0
        else:
            state["base"] = state["best"]
            if confirming:
                log(
                    f"  confirmation run scored {score if score is not None else -1:.0f} against the best "
                    f"{state['best_score']:.0f} — the gain did not reproduce, keeping the previous best"
                )
            else:
                shown = "n/a" if score is None else f"{score:+.0f}"
                log(
                    f"  rejected ({opponent_label}: {shown}, bar {bar}); "
                    f"continuing from {Path(state['best']).name}"
                )
            state.pop("pending", None)
            state["confirm_offset"] = 0

        record = {
            "iteration": iteration,
            "recipe": name,
            "anchor": bool(not args.no_anchor and keep_anchor),
            "replica_scores": [round(x, 1) for x in scores],
            "margin2": None if margin2 is None else round(margin2, 1),
            "partial": bool(vs_base and vs_base.get("games", args.eval_games) < args.eval_games),
            "group_mean": None if score is None else round(score, 1),
            "checkpoint": str(target),
            "seconds": round(time.time() - started, 1),
            "vs_efficiency": vs_base,
            "vs_previous": vs_prev,
            "best": state["best"],
            "best_score": state["best_score"],
        }
        state["history"].append(record)
        state["iteration"] = iteration
        save_state(state)
        log(f"iter {iteration} done in {record['seconds']}s; best = {Path(state['best']).name}")
        trim(state, args, bootstrap, iteration)

    log("training loop finished")
    log(f"best checkpoint: {state['best']} (score {state['best_score']})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
