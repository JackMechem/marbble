#!/usr/bin/env bash
# Opens VST3 editors on a private Xvfb display, at scale 1 and again with GDK_SCALE=2, and checks
# that each editor's window doubles: the host passed the scale (IPlugViewContentScaleSupport)
# and sized the window from the plug-in's answer. Then opens each at scale 1 and sets Xft.dpi to
# 192 with xrdb while it is open, and back to 96: the open window must double and come back.
# Uses vst3-host's `editor` example, not the app.
#
# usage: scripts/editor-scale-check.sh [plug-in name ...]
#   default: LSP Compressor Mono, Dragonfly Hall Reverb, CHOWTapeModel and TONE3000; each one
#   that isn't installed is skipped.
# Needs Xvfb, xwininfo and xrdb, e.g.
# `nix shell nixpkgs#xorg.xorgserver nixpkgs#xorg.xwininfo nixpkgs#xorg.xrdb`.
set -euo pipefail

cd "$(dirname "$0")/.."
PLUGINS=("$@")
[ ${#PLUGINS[@]} -gt 0 ] || PLUGINS=("Compressor Mono" "Dragonfly Hall Reverb" "CHOWTapeModel" "TONE3000")
for tool in Xvfb xwininfo xrdb; do
  command -v "$tool" >/dev/null || { echo "needs $tool (see the comment at the top)" >&2; exit 2; }
done

cargo build -q -p vst3-host --example editor
EXAMPLE="${CARGO_TARGET_DIR:-target}/debug/examples/editor"
SCRATCH=$(mktemp -d)
# A display number nobody uses; -noreset keeps it up between clients.
NUM=$((90 + RANDOM % 400))
Xvfb ":$NUM" -screen 0 3840x2160x24 -nolisten tcp -noreset >"$SCRATCH/xvfb.log" 2>&1 &
XVFB=$!
trap 'kill $XVFB 2>/dev/null; rm -rf "$SCRATCH"' EXIT
export DISPLAY=":$NUM"
for _ in $(seq 50); do xwininfo -root >/dev/null 2>&1 && break; sleep 0.1; done

# The editor window's size now, or nothing.
geometry() {
  xwininfo -root -tree | grep '"marbble-plugin"' | grep -o '[0-9]*x[0-9]*+' | head -1 | tr -d + || true
}

# Waits for the editor window to show, then a second more for a plug-in that sizes itself after
# `attached` to do so.
settle() {
  for _ in $(seq 30); do
    sleep 0.1
    [ -n "$(geometry)" ] && break
  done
  sleep 1
}

# The size of the editor window while `editor` runs with the given environment, or nothing.
size() {
  local name=$1
  shift
  env "$@" "$EXAMPLE" "$name" 4 >"$SCRATCH/run.log" 2>&1 &
  local pid=$!
  settle
  local g
  g=$(geometry)
  wait $pid || true
  echo "$g"
}

# Whether `$2` is twice `$1` (WxH), within 2%: plug-ins round their own layouts.
doubled() {
  local w1=${1%x*} h1=${1#*x} w2=${2%x*} h2=${2#*x}
  [ -n "$1" ] && [ -n "$2" ] && [ $((w2 * 100 / w1)) -ge 198 ] && [ $((w2 * 100 / w1)) -le 202 ] && [ $((h2 * 100 / h1)) -ge 198 ] && [ $((h2 * 100 / h1)) -le 202 ]
}

# Whether `$2` is `$1` (WxH), within 1%.
same() {
  local w1=${1%x*} h1=${1#*x} w2=${2%x*} h2=${2#*x}
  [ -n "$1" ] && [ -n "$2" ] && [ $((w2 * 100 / w1)) -ge 99 ] && [ $((w2 * 100 / w1)) -le 101 ] && [ $((h2 * 100 / h1)) -ge 99 ] && [ $((h2 * 100 / h1)) -le 101 ]
}

# The editor's window sizes, open at scale 1, after `xrdb` sets Xft.dpi to 192 and after it sets
# it back to 96, separated by spaces.
live() {
  local name=$1
  xrdb -remove
  env -u GDK_SCALE "$EXAMPLE" "$name" 8 >"$SCRATCH/run.log" 2>&1 &
  local pid=$!
  settle
  local one two back
  one=$(geometry)
  echo "Xft.dpi: 192" | xrdb -merge
  sleep 1.5
  two=$(geometry)
  echo "Xft.dpi: 96" | xrdb -merge
  sleep 1.5
  back=$(geometry)
  wait $pid || true
  xrdb -remove
  echo "$one ${two:--} ${back:--}"
}

failed=0
for name in "${PLUGINS[@]}"; do
  one=$(size "$name" GDK_SCALE=1)
  if [ -z "$one" ]; then
    if grep -q "plugin not found" "$SCRATCH/run.log"; then echo "SKIP  $name (not installed)"; continue; fi
    echo "FAIL  $name: no editor window at scale 1"; failed=1; continue
  fi
  two=$(size "$name" GDK_SCALE=2)
  if doubled "$one" "$two"; then
    echo "ok    $name: $one at scale 1, $two at scale 2"
  else
    echo "FAIL  $name: $one at scale 1, ${two:-no window} at scale 2"
    failed=1
  fi
  read -r a b c <<<"$(live "$name")"
  if doubled "$a" "$b" && same "$a" "$c"; then
    echo "ok    $name while open: $a, $b at Xft.dpi 192, $c back at 96"
  else
    echo "FAIL  $name while open: $a, $b at Xft.dpi 192, $c back at 96"
    failed=1
  fi
done
exit $failed
