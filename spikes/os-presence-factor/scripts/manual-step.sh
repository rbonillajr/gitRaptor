#!/bin/bash
# SPIKE-GRD-002: one physical step of the manual script (research/SPIKE-GRD-002-guion-manual.md).
# Each step runs the probe, appends its JSON to results/manual.jsonl and asks one or two
# observation questions. Nothing records credentials: the probe only sees yes/no and an error code.
# usage: scripts/manual-step.sh <step>
set -u
cd "$(dirname "$0")/.."
ROOT="$PWD"; P="$ROOT/variants/linker-adhoc"; [ -x "$P" ] || P="$ROOT/target/release/presence-probe"
OUT="$ROOT/results/manual.jsonl"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/presence-manual.XXXXXX")"
UIDN="$(id -u)"; LABEL="dev.gitraptor.spike.presence.manual"
trap 'launchctl bootout "gui/$UIDN/$LABEL" 2>/dev/null; rm -rf "$WORK"' EXIT
STEP="${1:-}"

ask() { local a; read -r -p "  ▶ $1 " a; echo "$a"; }
save() { # json, then key=answer pairs
  local j="$1"; shift
  python3 -c 'import json,sys
d=json.loads(sys.argv[1]) if sys.argv[1].strip().startswith("{") else {"raw":sys.argv[1]}
d={"step":sys.argv[2],"result":d,"answers":dict(a.split("=",1) for a in sys.argv[3:])}
print(json.dumps(d))' "$j" "$STEP" "$@" >> "$OUT"
  python3 -c 'import json,sys; d=json.loads(sys.argv[1]); r=d if "outcome" in d else d.get("result",d)
print("  → resultado:", r.get("outcome"), "| código:", r.get("la_code"), "| ms:", r.get("result_ms"))' "$j" 2>/dev/null || true
  echo "  ✓ guardado en results/manual.jsonl"
}
ev() { "$P" evaluate --timeout 60 --late-wait 3 --reason "$1" | head -1; }

case "$STEP" in
  1) echo "Paso 1: aparece el diálogo. APRUÉBALO CON TOUCH ID."
     r="$(ev 'aprobar el paso 1 del spike con Touch ID (código T1T1)')"
     save "$r" "nombre_visto=$(ask '¿Qué nombre de app ves en el diálogo (o «ninguno»)?')" "texto_completo=$(ask '¿Se leía el texto completo con el código T1T1? [s/n]')";;
  2) echo "Paso 2: aparece el diálogo. Pulsa «Usar contraseña…» y aprueba con TU CONTRASEÑA (el probe no la ve)."
     r="$(ev 'aprobar el paso 2 del spike con la contraseña (código P2P2)')"
     save "$r" "ok=$(ask '¿Pudiste aprobar con la contraseña? [s/n]')";;
  3) echo "Paso 3: aparece el diálogo. Pulsa CANCELAR."
     save "$(ev 'cancelar el paso 3 del spike (código C3C3)')";;
  4) echo "Paso 4: aparece el diálogo. Pon un dedo NO registrado (o el nudillo) hasta que el diálogo cambie; luego CANCELA."
     r="$(ev 'denegar el paso 4 del spike (código D4D4)')"
     save "$r" "que_paso=$(ask '¿Qué hizo el diálogo tras los intentos fallidos? (texto breve)')";;
  5) echo "Paso 5: saldrán DOS diálogos seguidos. Aprueba cada uno con Touch ID."
     r="$("$P" evaluate --timeout 60 --late-wait 3 --count 2 --reason 'paso 5 del spike: uso único (código U5U5)' | python3 -c 'import json,sys; print(json.dumps({"runs":[json.loads(l) for l in sys.stdin]}))')"
     save "$r" "dos_gestos=$(ask '¿Tuviste que poner el dedo DOS veces? [s/n]')";;
  6) echo "Paso 6: bloquea el Mac ahora (Ctrl+Cmd+Q) y desbloquéalo con TOUCH ID. El probe espera el desbloqueo y abre el diálogo en cuanto vuelvas."
     for _ in $(seq 1 600); do [ "$("$P" info | python3 -c 'import json,sys;print(json.load(sys.stdin)["cg_session"]["screen_is_locked"])')" = 1 ] && break; sleep 0.2; done
     for _ in $(seq 1 1500); do [ "$("$P" info | python3 -c 'import json,sys;print(json.load(sys.stdin)["cg_session"]["screen_is_locked"])')" = 0 ] && break; sleep 0.1; done
     r="$(ev 'paso 6 del spike: justo tras desbloquear (código R6R6)')"
     save "$r" "pidio_gesto=$(ask '¿El diálogo te pidió el dedo otra vez (no aprobó solo)? [s/n]')";;
  7) echo "Paso 7: daemon como LaunchAgent. Aparece el diálogo: APRUÉBALO CON TOUCH ID."
     S="$WORK/la.sock"
     cat > "$WORK/$LABEL.plist" <<PL
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>$LABEL</string>
<key>ProgramArguments</key><array><string>$P</string><string>serve</string><string>--socket</string><string>$S</string></array>
<key>RunAtLoad</key><true/></dict></plist>
PL
     launchctl bootstrap "gui/$UIDN" "$WORK/$LABEL.plist"
     for _ in $(seq 1 200); do [ -S "$S" ] && break; sleep 0.05; done
     r="$("$P" request --socket "$S" --timeout 60 --reason 'paso 7 del spike: LaunchAgent (código L7L7)')"
     save "$r" "nombre_visto=$(ask '¿Qué nombre de app ves en el diálogo?')";;
  8) echo "Paso 8: daemon bajo demanda (entorno vaciado + setsid). Aparece el diálogo: APRUÉBALO CON TOUCH ID."
     S="$WORK/od.sock"; "$P" spawn --socket "$S" --log "$WORK/od.log" >/dev/null
     r="$("$P" request --socket "$S" --timeout 60 --reason 'paso 8 del spike: bajo demanda (código O8O8)')"
     "$P" request --socket "$S" --op quit >/dev/null 2>&1
     save "$r";;
  9) echo "Paso 9: binario sin interfaz dentro de una .app de prueba. Aparece el diálogo: APRUÉBALO CON TOUCH ID."
     open -n -g -W "$ROOT/variants/GitRaptorSpikeApp.app" --args bundle-main --mode child --timeout 60 --out "$WORK/b.jsonl"
     r="$(sed -n 2p "$WORK/b.jsonl")"
     save "${r:-"{}"}" "nombre_visto=$(ask '¿Qué nombre de app ves en el diálogo (y qué icono)?')";;
  10) echo "Paso 10 (robo de foco): en 3 s verás un prompt. Escribe SIN MIRAR la frase  hola mundo  y pulsa Enter."
      echo "    El diálogo saldrá a mitad. NO lo apruebes: si te lo encuentras, cancélalo."
      (sleep 4.5; "$P" evaluate --timeout 20 --late-wait 3 --reason 'paso 10 del spike: robo de foco (código F10F)' | head -1 > "$WORK/f.json") &
      BG=$!; sleep 3
      read -r -s -p "  escribe aquí (no se muestra): " typed; echo
      wait $BG
      save "$(cat "$WORK/f.json")" "chars_recibidos_por_terminal=${#typed}" "frase_intacta=$([ "$typed" = "hola mundo" ] && echo s || echo n)" "que_paso=$(ask '¿Qué pasó con lo que escribías cuando salió el diálogo? (texto breve)')";;
  11) echo "Paso 11: arranca el salvapantallas y, 3 s después, el probe pide el diálogo. No toques nada durante 10 s; luego vuelve (si pide desbloqueo, desbloquea)."
      open -a ScreenSaverEngine; sleep 3
      r="$("$P" evaluate --timeout 15 --late-wait 3 --reason 'paso 11 del spike: salvapantallas (código S11S)' --with-context | python3 -c 'import json,sys; a=[json.loads(l) for l in sys.stdin]; a[0]["context"]=a[1]["context"]; print(json.dumps(a[0]))')"
      save "$r" "vio_dialogo=$(ask '¿Al volver había un diálogo esperando? [s/n]')";;
  opt-ssh) echo "Opcional E4: ejecuta este paso DENTRO de una sesión SSH a este Mac (ssh localhost) con la sesión gráfica abierta."
      S="$WORK/ssh.sock"; "$P" spawn --socket "$S" --log "$WORK/ssh.log" >/dev/null
      r="$("$P" request --socket "$S" --timeout 20 --reason 'opcional SSH del spike (código SSH4)')"
      "$P" request --socket "$S" --op quit >/dev/null 2>&1
      save "$r" "vio_dialogo=$(ask '¿Apareció un diálogo en la pantalla del Mac? [s/n]')";;
  opt-watch) echo "Opcional: aprueba con el Apple Watch (doble clic del botón lateral)."
      save "$(ev 'opcional reloj del spike (código W0W0)')" "ok=$(ask '¿Aprobó con el reloj? [s/n]')";;
  opt-lid) echo "Opcional E5: con la tapa CERRADA y monitor externo. Aprueba con lo que ofrezca el diálogo."
      save "$(ev 'opcional tapa cerrada (código LID5)')" "ofrecio=$(ask '¿Qué ofrecía el diálogo (Touch ID / contraseña / reloj)?')";;
  *) echo "usage: $0 <1..11|opt-ssh|opt-watch|opt-lid>"; exit 2;;
esac
