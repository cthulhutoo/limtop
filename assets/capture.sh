#!/usr/bin/env bash
# Capture limtop views to ANSI text (with colors) via tmux, then render to PNG.
set -euo pipefail
cd "$(dirname "$0")/.."
BIN=./target/release/limtop
OUT=assets
SESSION=limshot
W=110; H=32

mkdir -p "$OUT/captures"
tmux kill-session -t $SESSION 2>/dev/null || true
tmux new-session -d -s $SESSION -x $W -y $H "$BIN"
sleep 1.2

cap() { # name, keys...
  local name=$1; shift
  for k in "$@"; do tmux send-keys -t $SESSION "$k"; sleep 0.35; done
  sleep 0.5
  tmux capture-pane -t $SESSION -e -p > "$OUT/captures/$name.txt"
  echo "captured $name"
}

# 1. main dashboard (24h default)
cap dashboard

# 2. rate window hero — span all, look for the gauge
tmux send-keys -t $SESSION 5; sleep 0.5
cap rate-window

# 3. project drill-down list
tmux send-keys -t $SESSION p; sleep 0.5
cap projects-list

# 4. project detail (select 2nd project = most active)
tmux send-keys -t $SESSION Down; sleep 0.3
tmux send-keys -t $SESSION Enter; sleep 0.6
cap project-detail

tmux send-keys -t $SESSION q; sleep 0.2
tmux kill-session -t $SESSION 2>/dev/null || true

# render all
for f in "$OUT"/captures/*.txt; do
  name=$(basename "$f" .txt)
  python3 "$OUT/render_ansi.py" "$f" "$OUT/$name.png" 1
done
ls -la "$OUT"/*.png
