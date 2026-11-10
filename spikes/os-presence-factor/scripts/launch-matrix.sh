#!/bin/bash
# SPIKE-GRD-002 E2/E3 (+E5 state): launch forms vs. session attributes and evaluation result.
# No human needed: each evaluation uses a short timeout and the probe cancels it with invalidate().
# Never touches GitRaptor nor its profile. LaunchAgent label: dev.gitraptor.spike.presence (booted out at the end).
#
# usage: scripts/launch-matrix.sh <probe-binary> <out.jsonl> [timeout-s]
set -u
PROBE="$1"; OUT="$2"; TO="${3:-5}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/presence-spike.XXXXXX")"
UIDN="$(id -u)"
LABEL="dev.gitraptor.spike.presence"
trap 'launchctl bootout "gui/$UIDN/$LABEL" 2>/dev/null; launchctl bootout "gui/$UIDN/$LABEL.aqua" 2>/dev/null; rm -rf "$WORK"' EXIT

emit() { # tag json
  python3 -c 'import json,sys; d=json.loads(sys.argv[2]); d["cell"]=sys.argv[1]; print(json.dumps(d))' "$1" "$2" >> "$OUT"
}

req_pair() { # tag socket
  local tag="$1" sock="$2"
  emit "$tag:info" "$("$PROBE" request --socket "$sock" --op info)"
  emit "$tag:evaluate" "$("$PROBE" request --socket "$sock" --timeout "$TO")"
  "$PROBE" request --socket "$sock" --op quit >/dev/null 2>&1 || true
}

# Baseline: direct call from this process tree (child of the caller's shell).
emit "direct:caller-shell" "$("$PROBE" evaluate --timeout "$TO" --late-wait 3 --with-context | tail -2 | python3 -c 'import json,sys; a=[json.loads(l) for l in sys.stdin]; a[0]["context"]=a[1]["context"]; print(json.dumps(a[0]))')"

# E3 on demand, product order: client env_clear()+minimal vars, daemon setsid() itself.
S="$WORK/od.sock"
emit "ondemand:spawn" "$("$PROBE" spawn --socket "$S" --log "$WORK/od.log")"
req_pair "ondemand:env_clear+setsid" "$S"

# E2 LaunchAgent, as crates/core/src/autostart.rs (no LimitLoadToSessionType) and with Aqua.
for variant in plain aqua; do
  L="$LABEL"; extra=""
  if [ "$variant" = aqua ]; then L="$LABEL.aqua"; extra="<key>LimitLoadToSessionType</key><string>Aqua</string>"; fi
  S="$WORK/la-$variant.sock"
  cat > "$WORK/$L.plist" <<PL
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>$L</string>
<key>ProgramArguments</key><array><string>$PROBE</string><string>serve</string><string>--socket</string><string>$S</string></array>
<key>RunAtLoad</key><true/>
<key>StandardErrorPath</key><string>$WORK/la-$variant.log</string>
$extra
</dict></plist>
PL
  t0=$(python3 -c 'import time;print(time.time())')
  if launchctl bootstrap "gui/$UIDN" "$WORK/$L.plist" 2>"$WORK/bs.err"; then
    for _ in $(seq 1 200); do [ -S "$S" ] && break; sleep 0.05; done
    req_pair "launchagent:$variant" "$S"
  else
    emit "launchagent:$variant" "{\"error\":\"bootstrap failed\",\"stderr\":$(python3 -c 'import json,sys;print(json.dumps(open(sys.argv[1]).read()))' "$WORK/bs.err")}"
  fi
  launchctl bootout "gui/$UIDN/$L" 2>/dev/null || true
done

# E3 from Terminal.app (a fresh Aqua app, not a child of the caller): open a .command script.
S="$WORK/term.sock"; DONE="$WORK/term.done"
cat > "$WORK/term.command" <<CMD
#!/bin/bash
"$PROBE" spawn --socket "$S" --log "$WORK/term.log" > "$WORK/term.spawn.json"
"$PROBE" evaluate --timeout $TO --late-wait 3 --with-context > "$WORK/term.direct.jsonl"
touch "$DONE"
exit 0
CMD
chmod +x "$WORK/term.command"
if open -g -a Terminal "$WORK/term.command" 2>/dev/null; then
  for _ in $(seq 1 600); do [ -f "$DONE" ] && break; sleep 0.1; done
  if [ -f "$DONE" ]; then
    python3 -c 'import json,sys; a=[json.loads(l) for l in open(sys.argv[1])]; a[0]["context"]=a[1]["context"]; print(json.dumps(a[0]))' "$WORK/term.direct.jsonl" > "$WORK/t.json"
    emit "terminal-app:direct" "$(cat "$WORK/t.json")"
    emit "terminal-app:ondemand:spawn" "$(cat "$WORK/term.spawn.json")"
    req_pair "terminal-app:ondemand:env_clear+setsid" "$S"
  else
    emit "terminal-app" '{"error":"Terminal.app run did not finish in 60 s"}'
  fi
fi
echo "done: $OUT"
