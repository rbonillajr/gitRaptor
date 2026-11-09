---
schema: nassa.approval-request.v1
plan: "docs/dev-briefs/guard-exec-ux.2-plan.md"
---

Lee este fichero y luego `guard-exec-ux.2-plan.md`; `guard-exec-ux.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: **sí** — el crítico agotó sus rondas en la ronda 2 sin un plan aprobable; ver «Diferidos»
- Generada: `2026-11-09T00:24:21.546Z`
- Sha de aprobación: `1c0f0e1b66d996b0296359fc0984424483cab1ae6868f7837f6d870dcf2bc96f:5775462af5d40f1d851e4ab1558194b19cc2fbfe6655520543f9a214f83eb7c4:8e4bfcfd54fefd076660d0cc0f5c9c020726ddbd185015137e2c15724d218fc4`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES 73655ad6dfce>>>
```text
intent: Rene probó raptor guard exec y raptor agent correct en su Mac y los avisos confundían: una respuesta inválida terminaba sin explicar, no quedaba claro qué pasa si no respondes ni cuánto tiempo hay, salía '1 blocked actions' y el error de agent correct no decía por qué ni qué hacer.
endState: El aviso de guard exec explica las respuestas válidas y vuelve a preguntar, dice el tiempo y qué pasa sin respuesta, y toda cancelación (inválida, n, ventana cerrada sin respuesta, Ctrl-C) deja exception-cancelled sin ejecutar nada; los contadores de guard status y guard log usan singular/plural en en y es (los demás catálogos con contador se corrigen con el mismo helper, sin criterio propio); agent correct explica por qué falla y qué hacer.
nonGoals:
  - No cambia la duración de la ventana ni el flujo de tokens del daemon (ADR-GRD-007)
  - No toca guard cancel desde otra terminal, uninstall, confirm ni repair, que comparten ask()
  - No añade soporte Windows nuevo: el aviso en consola real queda como XP para la ronda de Windows
```
<<<FIN-DATOS-NO-CONFIABLES 73655ad6dfce>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 807d58748919>>>
```text
[1] id: UX1
statement: Una respuesta inválida (r) en el aviso de guard exec explica qué respuestas valen (y/yes/s/si/sí, n/no; Enter = no) y vuelve a preguntar; tras 3 inválidas cancela diciéndolo; git no se lanza, el remoto no cambia y queda un exception-cancelled en el registro
verify: cargo test -p gitraptor-cli --test guard_exec_ux -- --list 2>/dev/null | grep -q '^invalid_answer_explains_asks_again_and_cancels_after_three: test' && cargo test -p gitraptor-cli --test guard_exec_ux invalid_answer_explains_asks_again_and_cancels_after_three -- --exact
tests:
  - apps/cli/tests/guard_exec_ux.rs

[2] id: UX2
statement: Responder n o solo Enter cancela con el mismo mensaje claro (nada se ejecutó, la protección sigue igual), git no se lanza y queda un exception-cancelled
verify: cargo test -p gitraptor-cli --test guard_exec_ux -- --list 2>/dev/null | grep -q '^answer_no_cancels_and_logs: test' && cargo test -p gitraptor-cli --test guard_exec_ux answer_no_cancels_and_logs -- --exact
tests:
  - apps/cli/tests/guard_exec_ux.rs

[3] id: UX3
statement: Si la ventana se cierra sin respuesta, exec cancela por sí mismo (no espera indefinidamente), lo dice, git no se lanza y queda un exception-cancelled
verify: cargo test -p gitraptor-cli --test guard_exec_ux -- --list 2>/dev/null | grep -q '^no_answer_at_window_close_cancels_and_logs: test' && cargo test -p gitraptor-cli --test guard_exec_ux no_answer_at_window_close_cancels_and_logs -- --exact
tests:
  - apps/cli/tests/guard_exec_ux.rs

[4] id: UX4
statement: Ctrl-C en el aviso interactivo (sin --yes) cancela, git no se lanza y queda un exception-cancelled
verify: cargo test -p gitraptor-cli --test guard_exec_ux -- --list 2>/dev/null | grep -q '^ctrl_c_at_the_prompt_cancels_and_logs: test' && cargo test -p gitraptor-cli --test guard_exec_ux ctrl_c_at_the_prompt_cancels_and_logs -- --exact
tests:
  - apps/cli/tests/guard_exec_ux.rs

[5] id: UX5
statement: El aviso dice cuántos segundos hay para responder y que no responder cancela, en en y es; con --yes no pregunta y dice cuándo corre
verify: cargo test -p gitraptor-cli --test guard_exec_ux -- --list 2>/dev/null | grep -q '^prompt_states_time_and_default_in_en_and_es: test' && cargo test -p gitraptor-cli --test guard_exec_ux prompt_states_time_and_default_in_en_and_es -- --exact
tests:
  - apps/cli/tests/guard_exec_ux.rs

[6] id: UX6
statement: Con un solo elemento, los mensajes con contador de guard status y guard log usan el singular en en y es (1 blocked action / 1 acción bloqueada); con varios, el plural
verify: cargo test -p gitraptor-cli --test guard_plurals -- --list 2>/dev/null | grep -q '^one_item_uses_the_singular_in_en_and_es: test' && cargo test -p gitraptor-cli --test guard_plurals one_item_uses_the_singular_in_en_and_es -- --exact
tests:
  - apps/cli/tests/guard_plurals.rs

[7] id: UX7
statement: raptor agent correct sobre un worktree sin sesión detectada dice por qué no pudo (solo corrige sesiones detectadas por GitRaptor) y qué hacer (raptor sessions para verlas; register para atribuir), en en y es
verify: cargo test -p gitraptor-cli --test agent_correct_message -- --list 2>/dev/null | grep -q '^correct_without_detected_session_explains_why_and_what_to_do: test' && cargo test -p gitraptor-cli --test agent_correct_message correct_without_detected_session_explains_why_and_what_to_do -- --exact
tests:
  - apps/cli/tests/agent_correct_message.rs
```
<<<FIN-DATOS-NO-CONFIABLES 807d58748919>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES 8ab94dca70af>>>
```text
- Decisión del orquestador (2026-11-08), validada por Arquitecto (advisor): **A** respuesta inválida explica las válidas y repregunta, máx. 3 inválidas, luego cancela diciéndolo; **B** ventana cerrada sin respuesta cancela (fail-closed) y lo dice; **C** `--yes` solo omite la pregunta. Sin conflicto con ADR-GRD-007 ni ADR-GRD-006.
- Enter solo = **No** (ConfirmPrompt) y se trata como `Answer::No` ("nada se ejecutó"), no como "sin respuesta". Solo en el aviso de `exec`: `ask()` de install, uninstall, confirm y repair no cambia (no-objetivo).
- El plazo para responder es la ventana (10 s por defecto, ⚠️ ASSUMPTION de ADR-GRD-007). Los reintentos no lo alargan: al llegar `applies_at` se cancela aunque el hilo siga leyendo. Se añade una enmienda breve a ADR-GRD-007 § 3 que lo dice (la pide el Arquitecto).
- `cancel()` en `exec.rs` hoy ignora el error. Pasa a devolver `Result`; si falla, `exec` avisa por stderr que no pudo confirmar la cancelación (no dice que quedó auditada) y sale con error. Nunca lanza git. Se prueba con una prueba unitaria del módulo, porque el fallo no es reproducible en e2e sin carrera.
- Plurales: helper `tn(key, count, args)` y `Tr::tn` en `apps/cli/src/i18n.rs` con claves `<key>.one` / `<key>.many` (precedente `timeline.entries.one/.many`). El test `catalogs_match` ya exige las mismas claves y placeholders en en/es. Se corrigen los contadores con sustantivo cuyo valor puede ser 1. Guía: `guard.status.blocked`, `guard.log.summary` (acciones y días), `guard.log.notices`, `guard.log.rate-limited`, `guard.confirm.question-relaxations`, `guard.log.exceptions`, `discovery.found`, `discovery.root-removed`, `status.unmerged`, `status.kept-temps*`, `undo.done-since`, `undo.confirm-plan-since`, `undo.overlap.kept`. El experto enumera con `grep '{count}'` y no cambia los que no pueden valer 1.
- `agent correct`: el rechazo `no-detected-attribution` / `registered-session` no dice que el comando solo corrige sesiones detectadas por GitRaptor. Texto nuevo (en/es): qué pasó, por qué, qué hacer (`raptor sessions` para verlas; `raptor agent register` para atribuir sin sesión detectada; `raptor agent withdraw` si fue registrada).
- Windows: el aviso usa `stdin.is_terminal()` y texto sin ANSI; no se puede verificar desde el Mac. Se añade una fila **XP** (siguiente id libre al implementar y al rebasar) en `docs/architecture/xplat-pendientes.md`: aviso interactivo de `guard exec` en consola real (inválida, `n`, ventana sin respuesta, Ctrl-C).
- UX2 y UX4 fijan comportamiento que ya funciona (regresión); lo nuevo que observa UX2 es el texto del mensaje y que Enter solo cuenta como `n`. UX1, UX3 y UX5 son los rojos.
- El cambio de `cancel()` con resultado no tiene criterio firmado (no es reproducible en e2e); se verifica en la revisión de código y con una prueba unitaria que añade el experto en `exec.rs`.
- Los plurales van después de la pregunta de exec (el mismo experto, en secuencia): ambas tareas tocan `apps/cli/src/guard.rs`; los plurales solo cambian las llamadas `t(...)` con contador.
- La enmienda a ADR-GRD-007 no debe regenerar `docs/ARTIFACTS.md`: si un hook lo modifica, se revierte antes del commit y se anota en el PR.
- Fuera de alcance: duración de la ventana, `guard cancel` desde otra terminal, daemon.
```
<<<FIN-DATOS-NO-CONFIABLES 8ab94dca70af>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES f2ee48c2270a>>>
```text
docs/dev-briefs/guard-exec-ux.2-plan.md
```
<<<FIN-DATOS-NO-CONFIABLES f2ee48c2270a>>>

planSha: `1c0f0e1b66d996b0296359fc0984424483cab1ae6868f7837f6d870dcf2bc96f` (base de la cadena de re-planes)

Léelo en `guard-exec-ux.2-plan.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 1fca50f241d6>>>
```text
docs/dev-briefs/guard-exec-ux.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 1fca50f241d6>>>

contractSha: `5775462af5d40f1d851e4ab1558194b19cc2fbfe6655520543f9a214f83eb7c4`

## Tests de criterio

testsSha: `8e4bfcfd54fefd076660d0cc0f5c9c020726ddbd185015137e2c15724d218fc4`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES c0d9c6d95cbe>>>
```text
bb7ae3de12d4ec39861ac97cb9567fa7ea9b206d86d5dc67bf3986eef4998c3f  apps/cli/tests/agent_correct_message.rs
697254d74a663531364a8e35e0b1e98f4cdfa95409cd266c96df896f92584851  apps/cli/tests/guard_exec_ux.rs
3adb04cd26e26ec185b7954344f2b93432251d8918c2cc39f75cbe7c928c173c  apps/cli/tests/guard_plurals.rs
```
<<<FIN-DATOS-NO-CONFIABLES c0d9c6d95cbe>>>

## Crítico

- Rondas (plan): 2
  - ronda 1: AJUSTES · 1 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: AJUSTES · 1 bloqueante(s) · siguiente: excepcion
- Último veredicto: AJUSTES
- Excepción: sí — se aprueba con los hallazgos abiertos como diferidos, o se para el paquete

## Diferidos

### Diferidos propuestos

1 hallazgo(s) bloqueante(s) que el crítico dejó abiertos, copiados tal cual:

<<<DATOS-NO-CONFIABLES 8b08359120b4>>>
```text
id: PC-3
kind: test
toHuman: false
evidence: apps/cli/tests/guard_exec_ux.rs:185
ask: El test de UX2 debe ejercitar tambien Enter solo en una maquina aparte, con el mismo mensaje y el exception-cancelled.
```
<<<FIN-DATOS-NO-CONFIABLES 8b08359120b4>>>

## Firmas

- planSha (contexto, no firmado): `1c0f0e1b66d996b0296359fc0984424483cab1ae6868f7837f6d870dcf2bc96f`
- contractSha: `5775462af5d40f1d851e4ab1558194b19cc2fbfe6655520543f9a214f83eb7c4`
- testsSha: `8e4bfcfd54fefd076660d0cc0f5c9c020726ddbd185015137e2c15724d218fc4`
- approvalSha: `1c0f0e1b66d996b0296359fc0984424483cab1ae6868f7837f6d870dcf2bc96f:5775462af5d40f1d851e4ab1558194b19cc2fbfe6655520543f9a214f83eb7c4:8e4bfcfd54fefd076660d0cc0f5c9c020726ddbd185015137e2c15724d218fc4`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
