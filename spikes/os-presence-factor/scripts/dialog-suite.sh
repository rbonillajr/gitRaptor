#!/bin/bash
# SPIKE-GRD-002 automated dialog suite (needs an UNLOCKED screen and nobody touching keyboard/mouse).
# Every dialog is opened by the probe and closed by the probe (invalidate) or by a synthetic attack.
# Nobody approves anything here: a "verified" outcome in an attack cell is a CRITICAL finding.
# Never types the real password: the only typed text is a fixed wrong string (max 2 attempts).
#
# usage: scripts/dialog-suite.sh [section...]   sections: discover e7 e8 e9 e15 attacks (default: all)
set -u
cd "$(dirname "$0")/.."
ROOT="$PWD"; P="$ROOT/variants/linker-adhoc"; [ -x "$P" ] || P="$ROOT/target/release/presence-probe"
TS="$(date +%Y%m%d-%H%M%S)"
OUT="$ROOT/results/dialog-suite-$TS.jsonl"; AXD="$ROOT/results/ax-$TS"; mkdir -p "$AXD"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/presence-suite.XXXXXX")"
UIDN="$(id -u)"; LABEL="dev.gitraptor.spike.presence.bundle"
trap 'launchctl bootout "gui/$UIDN/$LABEL" 2>/dev/null; rm -rf "$WORK"' EXIT
SECTIONS="${*:-discover e7 e8 e9 e15 attacks attacks-noax}"
WRONG_TRIES=0

emit() { [ -n "${SPIKE_NOAX:-}" ] && set -- "noax:$1" "$2"; python3 -c 'import json,sys
try: d=json.loads(sys.argv[2])
except Exception: d={"raw":sys.argv[2]}
d["cell"]=sys.argv[1]; print(json.dumps(d))' "$1" "$2" >> "$OUT"; echo "· $1"; }

locked() { "$P" info | python3 -c 'import json,sys; print(json.load(sys.stdin)["cg_session"]["screen_is_locked"])'; }
if [ "$(locked)" != "0" ]; then echo "La pantalla está bloqueada: desbloquéala y vuelve a lanzar." >&2; exit 3; fi

OWNERS_FILE="$ROOT/results/dialog-owner.txt"
owners() { cat "$OWNERS_FILE" 2>/dev/null || echo ""; }

# Background observer: waits for the dialog, dumps its AX tree and (if allowed) a window screenshot.
observe() { # tag
  local tag="$1" w id
  w="$("$P" wait-dialog --owners "$(owners)" --secs 8)"
  echo "$w" > "$AXD/$tag.window.json"
  if echo "$w" | grep -q '"found":true'; then
    osascript "$ROOT/attacks/ax-dump.applescript" "$(echo "$w" | python3 -c 'import json,sys;print(json.load(sys.stdin)["window"]["owner"])')" > "$AXD/$tag.ax.txt" 2>&1
    id="$(echo "$w" | python3 -c 'import json,sys;print(json.load(sys.stdin)["window"]["number"])')"
    screencapture -x -o -l "$id" "$AXD/$tag.png" 2>/dev/null || echo "screencapture failed" > "$AXD/$tag.png.err"
  fi
}

# ---------------------------------------------------------------- discover
if [[ " $SECTIONS " == *" discover "* ]]; then
  r="$("$P" evaluate --timeout 4 --late-wait 3 --with-context | head -1)"
  emit "discover:direct" "$r"
  echo "$r" | python3 -c 'import json,sys
d=json.load(sys.stdin); own=sorted({w["owner"] for w in d["dialog"]["windows"]})
print(",".join(own))' > "$OWNERS_FILE"
  echo "dialog owners: $(owners)"
  observe "discover" & OBS=$!
  emit "discover:ax" "$("$P" evaluate --timeout 6 --late-wait 3 | head -1)"; wait $OBS
fi

# ---------------------------------------------------------------- E7 deadline, invalidate, late reply, kill
if [[ " $SECTIONS " == *" e7 "* ]]; then
  emit "e7:invalidate-3s" "$("$P" evaluate --timeout 3 --late-wait 5)"
  emit "e7:drop-early" "$("$P" evaluate --timeout 3 --late-wait 4 --drop-early)"
  emit "e7:two-in-a-row" "$("$P" evaluate --timeout 3 --late-wait 3 --count 2 | python3 -c 'import json,sys; print(json.dumps({"runs":[json.loads(l) for l in sys.stdin]}))')"
  # Kill the daemon with the dialog open: does the dialog disappear?
  S="$WORK/kill.sock"; "$P" spawn --socket "$S" --log "$WORK/kill.log" > "$WORK/kill.spawn.json"
  SPID="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["pid"])' "$WORK/kill.spawn.json")"
  "$P" request --socket "$S" --timeout 30 > "$WORK/kill.req.json" 2>/dev/null & RQ=$!
  w="$("$P" wait-dialog --owners "$(owners)" --secs 8)"
  t0=$(python3 -c 'import time;print(time.time())'); kill -9 "$SPID"
  gone="null"
  for i in $(seq 1 100); do
    if ! "$P" windows | python3 -c 'import json,sys
o=set(sys.argv[1].split(",")); d=json.load(sys.stdin)
sys.exit(0 if any(w["owner"] in o for w in d["windows"]) else 1)' "$(owners)"; then
      gone=$(python3 -c "import time;print(int((time.time()-$t0)*1000))"); break
    fi; sleep 0.05
  done
  wait $RQ 2>/dev/null
  emit "e7:kill-daemon-with-dialog" "{\"dialog_seen\":$(echo "$w" | grep -c '"found":true'),\"dialog_gone_after_kill_ms\":$gone}"
fi

# ---------------------------------------------------------------- E8 concurrency
if [[ " $SECTIONS " == *" e8 "* ]]; then
  emit "e8:pair-same-process" "$("$P" pair --timeout 4)"
  S="$WORK/busy.sock"; "$P" spawn --socket "$S" --log "$WORK/busy.log" >/dev/null
  "$P" request --socket "$S" --timeout 5 > "$WORK/b1.json" & B1=$!
  "$P" wait-dialog --owners "$(owners)" --secs 8 >/dev/null
  emit "e8:busy-second-request" "$("$P" request --socket "$S" --timeout 5)"
  wait $B1; emit "e8:busy-first-request" "$(cat "$WORK/b1.json")"
  "$P" request --socket "$S" --op quit >/dev/null 2>&1
fi

# ---------------------------------------------------------------- E9 packaging + reason text
if [[ " $SECTIONS " == *" e9 "* ]]; then
  for v in linker-adhoc adhoc adhoc-hardened plist-adhoc plist-hardened plist-devsigned; do
    [ -x "$ROOT/variants/$v" ] || continue
    observe "e9-$v" & OBS=$!
    emit "e9:$v" "$("$ROOT/variants/$v" evaluate --timeout 5 --late-wait 3 | head -1)"; wait $OBS
  done
  LONG="añadir el repo /Users/spike/proyectos/un-nombre-de-carpeta-muy-largo-para-ver-si-el-dialogo-trunca/subcarpeta/otra-subcarpeta/y-otra-mas/repo-final con la rama feat/una-rama-con-nombre-largo y el valor core.hooksPath=/ruta/de/ganchos/muy/larga/que/no/deberia/perderse FIN-MARCA-K7Q2"
  observe "e9-long-reason" & OBS=$!
  emit "e9:long-reason" "$("$P" evaluate --timeout 6 --late-wait 3 --reason "$LONG" | head -1)"; wait $OBS
  CTRL="$(printf 'añadir el repo /tmp/a\n\n\n\n\n\n\n\nOK pulsado por el humano \xe2\x80\x8e\xe2\x80\xae2Q7K')"
  observe "e9-control-chars" & OBS=$!
  emit "e9:control-chars-unsanitized" "$("$P" evaluate --timeout 6 --late-wait 3 --reason "$CTRL" | head -1)"; wait $OBS
fi

# ---------------------------------------------------------------- E15 headless binary inside a bundle
if [[ " $SECTIONS " == *" e15 "* ]]; then
  for app in GitRaptorSpikeApp GitRaptorSpikeAppDev; do
    A="$ROOT/variants/$app.app"; [ -d "$A" ] || continue
    for mode in child ondemand direct; do
      observe "e15-$app-$mode" & OBS=$!
      open -n -g -W "$A" --args bundle-main --mode "$mode" --timeout 5 --out "$WORK/$app-$mode.jsonl"
      wait $OBS
      if [ -f "$WORK/$app-$mode.jsonl" ]; then
        while IFS= read -r l; do emit "e15:$app:open:$mode" "$l"; done < "$WORK/$app-$mode.jsonl"
      else emit "e15:$app:open:$mode" '{"error":"no output (open failed?)"}'; fi
    done
    # LaunchAgent whose program path is inside the bundle.
    S="$WORK/$app-la.sock"
    cat > "$WORK/$LABEL.plist" <<PL
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>$LABEL</string>
<key>ProgramArguments</key><array><string>$A/Contents/MacOS/raptor</string><string>serve</string><string>--socket</string><string>$S</string></array>
<key>RunAtLoad</key><true/></dict></plist>
PL
    launchctl bootstrap "gui/$UIDN" "$WORK/$LABEL.plist"
    for _ in $(seq 1 200); do [ -S "$S" ] && break; sleep 0.05; done
    observe "e15-$app-launchagent" & OBS=$!
    emit "e15:$app:launchagent" "$("$P" request --socket "$S" --timeout 5)"; wait $OBS
    launchctl bootout "gui/$UIDN/$LABEL" 2>/dev/null
  done
fi

# ---------------------------------------------------------------- E10/E14 synthetic input without the secret
attack() { # tag needs_front command...
  local tag="$1" front="$2"; shift 2
  local R="$WORK/att.json"
  "$P" evaluate --timeout 7 --late-wait 3 --reason "spike ataque $tag (código ATK1)" > "$R" & EV=$!
  local w; w="$("$P" wait-dialog --owners "$(owners)" --secs 8)"
  if ! echo "$w" | grep -q '"found":true'; then wait $EV; emit "attack:$tag" "{\"skipped\":\"dialog not seen\",\"evaluate\":$(head -1 "$R")}"; return; fi
  sleep 0.6
  local fm; fm="$(osascript "$ROOT/attacks/frontmost.applescript" 2>&1)"
  local before; before="$("$P" info | python3 -c 'import json,sys; print(json.dumps(json.load(sys.stdin)["cg_session"]))')"
  if [ "$front" = "front" ] && ! grep -qx "$fm" <(owners | tr ',' '\n'); then
    wait $EV; emit "attack:$tag" "{\"skipped\":\"dialog not frontmost (keyboard attack guarded)\",\"frontmost\":\"$fm\",\"evaluate\":$(head -1 "$R")}"; return
  fi
  "$P" tap --secs 3 > "$WORK/tap.json" & TP=$!
  sleep 0.2
  local res; res="$("$@" 2>&1 | tail -3 | tr '\n' ' ' | sed 's/"/\\"/g')"
  wait $TP; wait $EV
  emit "attack:$tag" "{\"frontmost_before\":\"$fm\",\"cg_session_before\":$before,\"attack_output\":\"$res\",\"evaluate\":$(head -1 "$R"),\"tap\":$(cat "$WORK/tap.json")}"
  if grep -q '"outcome": *"verified"' "$R"; then
    echo "!!! CRITICAL: synthetic input completed the dialog in $tag" | tee -a "$ROOT/results/CRITICAL-$TS.txt"
  fi
}

button_center() { # title -> "x y" from the discover AX dump
  python3 - "$1" "$AXD" <<'PY'
import sys,glob,re
title=sys.argv[1]
for f in sorted(glob.glob(sys.argv[2]+"/../ax-*/discover.ax.txt"))[-1:]:
    for l in open(f):
        if "role=AXButton" in l and (f"title={title} " in l or f"desc={title} " in l):
            m=re.search(r"pos=(-?\d+),(-?\d+) \| size=(\d+),(\d+)",l)
            if m:
                x,y,w,h=map(int,m.groups()); print(x+w//2,y+h//2); sys.exit()
PY
}

buttons() { python3 - "$AXD" <<'PY'
import sys,glob,re
seen=[]
for f in sorted(glob.glob(sys.argv[1]+"/../ax-*/discover.ax.txt"))[-1:]:
    for l in open(f):
        if "role=AXButton" in l:
            t=re.search(r"title=(.*?) \|",l).group(1) or re.search(r"desc=(.*?) \|",l).group(1)
            if t and t not in seen: seen.append(t)
print("\n".join(seen))
PY
}

if [[ " $SECTIONS " == *" attacks "* ]]; then
  attack "osascript-keystroke-return" any osascript -e 'tell application "System Events" to keystroke return'
  attack "osascript-keycode-escape" any osascript -e 'tell application "System Events" to key code 53'
  attack "cgevent-return-combined" any "$P" key --code 36 --source combined
  attack "cgevent-return-private" any "$P" key --code 36 --source private
  attack "cgevent-return-hid-session-tap" any "$P" key --code 36 --source hid --tap session
  attack "cgevent-space-combined" any "$P" key --code 49 --source combined
  while IFS= read -r b; do
    [ -z "$b" ] && continue
    attack "axpress:$b" any osascript "$ROOT/attacks/ax-press.applescript" "$(owners | cut -d, -f1)" "$b"
    xy="$(button_center "$b")"
    if [ -n "$xy" ]; then
      attack "cgevent-click:$b:combined" any "$P" click --x "${xy% *}" --y "${xy#* }" --source combined
      attack "cgevent-click:$b:private" any "$P" click --x "${xy% *}" --y "${xy#* }" --source private
    fi
  done < <(buttons)
  # Password path with a WRONG fixed string (never the real one), at most 2 attempts.
  pw_attack() {
    local how="$1"; local b; b="$(buttons | grep -i -E 'contrase|password' | head -1)"
    [ -z "$b" ] && { emit "attack:password-path" '{"skipped":"no password button found in AX tree"}'; return; }
    [ $WRONG_TRIES -ge 2 ] && return; WRONG_TRIES=$((WRONG_TRIES+1))
    local R="$WORK/pw.json"
    "$P" evaluate --timeout 14 --late-wait 3 --reason "spike ataque contraseña $how (código PW01)" > "$R" & EV=$!
    "$P" wait-dialog --owners "$(owners)" --secs 8 >/dev/null; sleep 0.6
    local o1; o1="$(osascript "$ROOT/attacks/ax-press.applescript" "$(owners | cut -d, -f1)" "$b" 2>&1)"; sleep 1.0
    osascript "$ROOT/attacks/ax-dump.applescript" "$(owners | cut -d, -f1)" > "$AXD/password-$how.ax.txt" 2>&1
    local sec; sec="$("$P" info | python3 -c 'import json,sys; print(json.dumps(json.load(sys.stdin)["cg_session"]))')"
    local fm; fm="$(osascript "$ROOT/attacks/frontmost.applescript" 2>&1)"
    local o2="guarded: dialog not frontmost ($fm)"
    if grep -qx "$fm" <(owners | tr ',' '\n'); then
      if [ "$how" = cgevent ]; then o2="$("$P" type --text "spike-wrong-1" --source combined)"; "$P" key --code 36 --source combined >/dev/null
      else o2="$(osascript -e 'tell application "System Events" to keystroke "spike-wrong-1"' -e 'tell application "System Events" to keystroke return' 2>&1)"; fi
      sleep 1.5
      osascript "$ROOT/attacks/ax-dump.applescript" "$(owners | cut -d, -f1)" > "$AXD/password-$how-after.ax.txt" 2>&1
    fi
    wait $EV
    emit "attack:password-path-wrong-secret:$how" "{\"switch_to_password\":\"$(echo "$o1"|tr -d '"')\",\"secure_input_during\":$sec,\"frontmost\":\"$fm\",\"typing\":\"$(echo "$o2"|tr -d '"'|tr '\n' ' ')\",\"evaluate\":$(head -1 "$R")}"
    grep -q '"outcome": *"verified"' "$R" && echo "!!! CRITICAL: wrong secret verified?!" | tee -a "$ROOT/results/CRITICAL-$TS.txt"
  }
  pw_attack cgevent
  pw_attack osascript
fi
# ---------------------------------------------------------------- same attacks from Terminal.app (no Accessibility)
if [[ " $SECTIONS " == *" attacks-noax "* ]] && [ -z "${SPIKE_NOAX:-}" ]; then
  DONE="$WORK/noax.done"
  cat > "$WORK/noax.command" <<CMD
#!/bin/bash
SPIKE_NOAX=1 "$ROOT/scripts/dialog-suite.sh" attacks > "$WORK/noax.log" 2>&1
touch "$DONE"
CMD
  chmod +x "$WORK/noax.command"; open -a Terminal "$WORK/noax.command"
  for _ in $(seq 1 3000); do [ -f "$DONE" ] && break; sleep 0.2; done
  emit "attacks-noax:run" "{\"log_tail\":$(tail -5 "$WORK/noax.log" | python3 -c 'import json,sys;print(json.dumps(sys.stdin.read()))')}"
fi
echo "resultados: $OUT  (AX/capturas: $AXD)"
[ -f "$ROOT/results/CRITICAL-$TS.txt" ] && { echo "HAY HALLAZGOS CRÍTICOS: $ROOT/results/CRITICAL-$TS.txt"; exit 10; }
exit 0
