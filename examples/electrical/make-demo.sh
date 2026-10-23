#!/usr/bin/env bash
# Builds a three-sheet demo project in OUT_DIR: Demo.ecproj, Demo-01.dxf (a control circuit on a
# ladder), Demo-02.dxf (a motor power circuit with its cable) and Demo-03.dxf (sensors wired
# through a terminal strip to a PLC input card), made with the electrical commands themselves,
# then tagged, wire-numbered, cross-referenced and titled across the project.
# Usage: make-demo.sh PATH/TO/cadcraft-cli OUT_DIR
set -euo pipefail
cli="$1"
out="$2"
mkdir -p "$out"
proj="$out/Demo.ecproj"
rm -f "$proj" "$out"/Demo-0*.dxf "$out"/Demo.ecdb

"$cli" run --metric --cmd "eproject.new {\"path\": \"$proj\", \"name\": \"Demo plant\", \"sheets\": 3}"

# Sheet 1: control circuit. A ladder, then symbols dropped onto its rungs (each cuts its rung
# and is tagged from its column).
args=()
cmd() { args+=(--cmd "$1 $2"); }
sym() { cmd esymbol "{\"name\": \"$1\", \"at\": [$2, $3]${4:+, \"tag\": \"$4\"}}"; }
cmd eladder '{"top": 240, "bottom": 40}'
sym IEC_PB_NC 21 200        # stop
sym IEC_PB_NO 21 160        # start
sym IEC_CONTACTOR_COIL 21 90
sym IEC_NO 105 170 -Q101    # contactor auxiliary contact
sym IEC_LAMP 105 100        # "running" lamp
sym IEC_ESTOP 147 200
sym IEC_COIL 147 90
sym IEC_NO 189 170 -K104
sym IEC_LAMP 189 100
cmd text '{"at": [5, 244], "text": "+24V DC", "height": 3}'
cmd text '{"at": [5, 34], "text": "0V", "height": 3}'
"$cli" run "$out/Demo-01.dxf" "${args[@]}" --save "$out/Demo-01.dxf"

# Sheet 2: motor power circuit, three phases through breaker, main contacts and overload.
args=()
for x in 55 60 65; do
  cmd ewire "{\"from\": [$x, 250], \"to\": [$x, 65]}"
done
sym IEC_MCB_3P 60 210
sym IEC_MAIN_CONTACTS_3P 60 160 -Q101
sym IEC_OVERLOAD_3P 60 115
sym IEC_MOTOR_3PH 60 60
cmd text '{"at": [52, 254], "text": "L1  L2  L3", "height": 2.5}'
# The motor cable: a line across the three wires between overload and motor.
cmd ecable '{"from": [48, 85], "to": [72, 85], "type": "4G1.5", "description": "Pump motor"}'
"$cli" run "$out/Demo-02.dxf" "${args[@]}" --save "$out/Demo-02.dxf"

# Sheet 3: three sensors through terminals -X2 to an 8-point PLC input card.
args=()
cmd eplc '{"kind": "DI", "points": 8, "at": [60, 60], "description": "Pump inputs"}'
for x in 60 70 80; do
  cmd ewire "{\"from\": [$x, 230], \"to\": [$x, 65]}"
done
sym IEC_PROX_IND 60 200
sym IEC_LIMIT_NO 70 200
sym IEC_LEVEL 80 200
for x in 60 70 80; do
  cmd esymbol "{\"name\": \"IEC_TERMINAL\", \"at\": [$x, 130], \"tag\": \"-X2\"}"
done
cmd text '{"at": [52, 234], "text": "+24V DC", "height": 2.5}'
"$cli" run "$out/Demo-03.dxf" "${args[@]}" --save "$out/Demo-03.dxf"

# Across the project: tags, wire numbers, cross-references, title blocks, the database.
"$cli" run --metric \
  --cmd "eproject.open {\"path\": \"$proj\"}" \
  --cmd "eproject.tag {}" \
  --cmd "eproject.wires {}" \
  --cmd "eproject.xref {}" \
  --cmd "etitle {\"project\": true, \"set\": {\"TITLE\": \"Pump station\", \"CUSTOMER\": \"Demo\", \"DRAWNBY\": \"CADCraft\", \"REV\": \"A\"}}" \
  --cmd "eproject.sync {}"

echo "wrote $proj"
