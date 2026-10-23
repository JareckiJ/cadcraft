#!/usr/bin/env bash
# Builds examples/electrical/demo-schematic.dxf: two A3 sheets (420 x 297, 10 columns each) with
# untagged symbols and unnumbered wires, to try Tools > Electrical > Tag Devices / Number Wires.
# Usage: make-demo.sh PATH/TO/cadcraft-cli OUT.dxf
set -euo pipefail
cli="$1"
out="$2"
args=()
cmd() { args+=(--cmd "$1 $2"); }

# Symbols: a 10 x 10 box with a TAG attribute to its right, base point in the middle.
symbol() { # name extra-geometry-json...
  local name="$1"
  shift
  cmd rectang '{"p1": [-5, -5], "p2": [5, 5]}'
  for g in "$@"; do cmd "${g%% *}" "${g#* }"; done
  cmd attdef '{"tag": "TAG", "prompt": "Device tag", "default": "", "at": [7, -1.25], "height": 2.5}'
  cmd selectall '{}'
  cmd block "{\"name\": \"$name\", \"base\": [0, 0], \"keep\": \"delete\"}"
}
cmd layer.new '{"name": "SYMBOLS", "current": true}'
symbol PUSHBUTTON 'line {"points": [[-5, 5], [5, -5]]}'
symbol RELAY_COIL 'line {"points": [[-5, 0], [5, 0]]}'
symbol PILOT_LAMP 'line {"points": [[-5, -5], [5, 5]]}' 'line {"points": [[-5, 5], [5, -5]]}'
symbol FUSE 'line {"points": [[0, -5], [0, 5]]}'
symbol CONTACTOR 'line {"points": [[-5, -5], [5, 5]]}'
symbol RELAY_CONTACT_NO 'line {"points": [[-5, -5], [3, 5]]}'
symbol MOTOR 'circle {"center": [0, 0], "radius": 4}'

# Frames: two sheets side by side, column numbers along the top.
cmd layer.new '{"name": "FRAME", "current": true}'
for sheet in 0 1; do
  x0=$((sheet * 420))
  cmd rectang "{\"p1\": [$x0, 0], \"p2\": [$((x0 + 420)), 297]}"
  for c in $(seq 1 10); do
    cx=$((x0 + (c - 1) * 42))
    cmd line "{\"points\": [[$cx, 297], [$cx, 287]]}"
    cmd text "{\"at\": [$((cx + 21)), 290], \"text\": \"$c\", \"height\": 3.5, \"justify\": \"BC\"}"
  done
  cmd text "{\"at\": [$((x0 + 400)), 5], \"text\": \"Sheet $((sheet + 1))\", \"height\": 3.5, \"justify\": \"BR\"}"
done

# Symbols placed (untagged).
ins() { cmd insert "{\"name\": \"$1\", \"at\": [$2, $3], \"attribs\": {\"TAG\": \"\"}}"; }
ins PUSHBUTTON 63 200
ins RELAY_COIL 63 100
ins PILOT_LAMP 189 150
ins RELAY_CONTACT_NO 189 200
ins PUSHBUTTON 315 200
ins RELAY_COIL 315 100
ins FUSE 483 200
ins CONTACTOR 483 140
ins MOTOR 483 60

# Wires: supply rails, branches between symbols (ends on symbol tops and bottoms).
cmd layer.new '{"name": "WIRE", "current": true, "color": 1}'
w() { cmd line "{\"points\": [[$1, $2], [$3, $4]]}"; }
w 20 270 400 270 # +24V rail, sheet 1
w 20 30 400 30   # 0V rail, sheet 1
for x in 63 315; do
  w $x 270 $x 205
  w $x 195 $x 105
  w $x 95 $x 30
done
w 189 270 189 205
w 189 195 189 155
w 189 145 189 30
w 483 280 483 205 # sheet 2: supply to fuse
w 483 195 483 145
w 483 135 483 65
w 440 280 520 280 # sheet 2 supply rail

cmd layer.current '{"name": "0"}'
"$cli" run --metric "${args[@]}" --save "$out"
echo "wrote $out"
