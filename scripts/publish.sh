#!/bin/bash
# Publish this repository to github.com/<you>/mmoyager-mahjong.
#
# Why a script: the repository side needs a token, and the only sane place for
# that token is the `gh` keychain entry — so the first command below is yours to
# run once. Everything after it is mechanical.
#
#   ./scripts/publish.sh            # private (default)
#   ./scripts/publish.sh --public   # public
#
# What gets pushed is whatever `git ls-files` shows: sources, docs, the experiment
# scripts, the training history, and the two reference checkpoints (~11 MB). The
# tens of gigabytes of self-play data are deliberately absent — see
# data/selfplay/README.md for how it is regenerated.

set -euo pipefail

cd "$(dirname "$0")/.."

VISIBILITY="--private"
[ "${1:-}" = "--public" ] && VISIBILITY="--public"

if ! gh auth status >/dev/null 2>&1; then
  cat <<'MSG'
尚未登录 GitHub。请先运行：

    gh auth login

（选 GitHub.com → HTTPS → 用浏览器登录一次即可，令牌会存进系统钥匙串。）
然后重新运行本脚本。
MSG
  exit 1
fi

if [ ! -d .git ]; then
  echo "这里不是 git 仓库；先在项目根目录 git init 并提交。"
  exit 1
fi

NAME=mmoyager-mahjong
if gh repo view "$NAME" >/dev/null 2>&1; then
  echo "远端仓库已存在，直接推送当前分支。"
  git remote get-url origin >/dev/null 2>&1 || \
    git remote add origin "$(gh repo view "$NAME" --json sshUrl -q .sshUrl)"
  git push -u origin main
else
  gh repo create "$NAME" "$VISIBILITY" --source . --remote origin --push \
    --description "日本麻将：完整规则引擎 + 从零自我博弈训练 + 可对战的 Web UI 与训练控制台"
fi

echo
echo "完成：$(gh repo view "$NAME" --json url -q .url 2>/dev/null || echo "https://github.com/$(gh api user -q .login)/$NAME")"
