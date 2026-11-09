---
schema: nassa.approval-request.v1
plan: "docs/requirements/features/desktop/dev-specs/US-DSK-004-flota-en-vivo.md"
---

Lee este fichero y luego `US-DSK-004-flota-en-vivo.md`; `US-DSK-004-flota-en-vivo.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: no
- Generada: `2026-11-09T13:38:10.433Z`
- Sha de aprobación: `ee7f15f0b099b14b4976f2706fbb8d0e10e92b3632a52e64074964768755a670:32de944b7151ec31d51922d5e1491c5649e1863e5e6ef8ed493ed899d693fb5f:e897d053b54a242d1dec9103237826c249906f0249cf0337efe6ccd0b870d3ad`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES acfa78ccba0b>>>
```text
intent: Que quien abre un repo en la app vea una tarjeta por copia de trabajo, con su agente, su estado y lo que pide atención primero, actualizada en vivo.
endState: El supervisor sigue el repo abierto (snapshot N, subscribe N+1, sessions.list, resync, unsubscribe al cambiar) y manda repo, worktrees, sessions, repoSession, prediction y repoClosed saneados; el webview los guarda por mensajes en la caché, elige el repo por defecto, pinta RepoTree en la barra lateral y FleetView en el centro, y deja las marcas gr:fleet-delta.
nonGoals:
  - No pinta el grafo ni la autoría de commits (US-DSK-005)
  - No añade métodos, eventos ni capacidades al canal ni escribe nada en el motor
  - No muestra Agentes · N en vivo ni ramas en el árbol
  - No hay filtro de terminadas ni acciones por tarjeta
  - No corre el banco desktop-modify
```
<<<FIN-DATOS-NO-CONFIABLES acfa78ccba0b>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 97f8120168e3>>>
```text
[1] id: D4-OPEN
statement: HubCmd::Open abre un repo: events.unsubscribe del anterior, scope.snapshot de repo, scope.subscribe N+1 y sessions.list, y entrega repo, worktrees y sessions con claves iguales entre worktree y sesión; un cambio de worktree.state llega en orden como worktrees
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test fleet_supervisor -- --list 2>/dev/null | grep -q '^opening_a_repo_delivers_its_worktrees_and_sessions_with_matching_keys: test' && cargo test -p gitraptor-desktop --test fleet_supervisor -- --list 2>/dev/null | grep -q '^a_worktree_state_event_reaches_the_webview_in_order: test' && cargo test -p gitraptor-desktop --test fleet_supervisor -- --list 2>/dev/null | grep -q '^switching_repos_unsubscribes_and_drops_the_old_scope: test' && cargo test -p gitraptor-desktop --test fleet_supervisor opening_a_repo_delivers_its_worktrees_and_sessions_with_matching_keys -- --exact && cargo test -p gitraptor-desktop --test fleet_supervisor a_worktree_state_event_reaches_the_webview_in_order -- --exact && cargo test -p gitraptor-desktop --test fleet_supervisor switching_repos_unsubscribes_and_drops_the_old_scope -- --exact
tests:
  - apps/desktop/src-tauri/tests/fleet_supervisor.rs
  - apps/desktop/src-tauri/tests/support/fake_daemon.rs

[2] id: D4-EVENTS
statement: session.state llega como repoSession y prediction.changed como prediction con claves de worktree; un repo sin sesiones informa detection y ninguna sesión; un snapshot nuevo del repo vuelve a pedir sesiones
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test fleet_supervisor -- --list 2>/dev/null | grep -q '^a_prediction_change_reaches_the_webview_with_worktree_keys: test' && cargo test -p gitraptor-desktop --test fleet_supervisor -- --list 2>/dev/null | grep -q '^a_repo_without_sessions_reports_detection_and_no_sessions: test' && cargo test -p gitraptor-desktop --test fleet_supervisor -- --list 2>/dev/null | grep -q '^a_new_repo_snapshot_asks_for_the_sessions_again: test' && cargo test -p gitraptor-desktop --test fleet_supervisor -- --list 2>/dev/null | grep -q '^a_repo_session_event_reaches_the_webview_as_repo_session: test' && cargo test -p gitraptor-desktop --test fleet_supervisor a_prediction_change_reaches_the_webview_with_worktree_keys -- --exact && cargo test -p gitraptor-desktop --test fleet_supervisor a_repo_without_sessions_reports_detection_and_no_sessions -- --exact && cargo test -p gitraptor-desktop --test fleet_supervisor a_new_repo_snapshot_asks_for_the_sessions_again -- --exact && cargo test -p gitraptor-desktop --test fleet_supervisor a_repo_session_event_reaches_the_webview_as_repo_session -- --exact
tests:
  - apps/desktop/src-tauri/tests/fleet_supervisor.rs

[3] id: D4-RESYNC
statement: Un hueco en el ámbito del repo toma instantánea y sesiones de nuevo; un repo rechazado o scope-closed manda repoClosed y se olvida
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test fleet_supervisor -- --list 2>/dev/null | grep -q '^a_gap_in_the_repo_scope_takes_a_new_snapshot_and_sessions: test' && cargo test -p gitraptor-desktop --test fleet_supervisor -- --list 2>/dev/null | grep -q '^a_closed_repo_is_reported_and_forgotten: test' && cargo test -p gitraptor-desktop --test fleet_supervisor a_gap_in_the_repo_scope_takes_a_new_snapshot_and_sessions -- --exact && cargo test -p gitraptor-desktop --test fleet_supervisor a_closed_repo_is_reported_and_forgotten -- --exact
tests:
  - apps/desktop/src-tauri/tests/fleet_supervisor.rs

[4] id: D4-SAFE
statement: En vista restringida Open no abre nada; la rama y la carpeta no confiables llegan saneadas (OSC y U+202E); ningún método de escritura aparece en calls()
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test fleet_supervisor -- --list 2>/dev/null | grep -q '^a_restricted_session_never_opens_a_repo: test' && cargo test -p gitraptor-desktop --test fleet_supervisor -- --list 2>/dev/null | grep -q '^untrusted_branch_and_folder_reach_the_webview_escaped: test' && cargo test -p gitraptor-desktop --test fleet_supervisor -- --list 2>/dev/null | grep -q '^a_fleet_run_never_sends_replace_stop_or_writes: test' && cargo test -p gitraptor-desktop --test fleet_supervisor a_restricted_session_never_opens_a_repo -- --exact && cargo test -p gitraptor-desktop --test fleet_supervisor untrusted_branch_and_folder_reach_the_webview_escaped -- --exact && cargo test -p gitraptor-desktop --test fleet_supervisor a_fleet_run_never_sends_replace_stop_or_writes -- --exact
tests:
  - apps/desktop/src-tauri/tests/fleet_supervisor.rs

[5] id: D4-INGEST
statement: ingest::key es FNV-1a 64 en 16 hex minúscula sobre los bytes crudos, igual para el mismo texto; detached_at solo pasa con 40 o 64 hex
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test fleet_ingest -- --list 2>/dev/null | grep -q '^key_is_fnv1a_64_in_16_lowercase_hex_of_the_raw_bytes: test' && cargo test -p gitraptor-desktop --test fleet_ingest -- --list 2>/dev/null | grep -q '^detached_at_passes_only_as_40_or_64_hex: test' && cargo test -p gitraptor-desktop --test fleet_ingest key_is_fnv1a_64_in_16_lowercase_hex_of_the_raw_bytes -- --exact && cargo test -p gitraptor-desktop --test fleet_ingest detached_at_passes_only_as_40_or_64_hex -- --exact
tests:
  - apps/desktop/src-tauri/tests/fleet_ingest.rs

[6] id: D4-CMD
statement: commands.json se compara EXACTO con app_info, engine_retry, engine_watch y fleet_open (la lista cerrada de ADR-GRP-017 no se relaja); fleet_open está en la capability; acepta {params:{repoId}} y rechaza campos desconocidos, id vacío, de más de 128 bytes o con control (invalid-params); sin supervisor da not-watching; V2 sigue cerrada
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test commands -- --list 2>/dev/null | grep -q '^d3_cmds_engine_commands_are_listed_and_allowed: test' && cargo test -p gitraptor-desktop --test commands -- --list 2>/dev/null | grep -q '^fleet_open_is_in_the_closed_list_and_the_capability: test' && cargo test -p gitraptor-desktop --test commands -- --list 2>/dev/null | grep -q '^fleet_open_accepts_params_and_rejects_bad_ids: test' && cargo test -p gitraptor-desktop --test commands -- --list 2>/dev/null | grep -q '^fleet_open_without_a_supervisor_is_not_watching: test' && cargo test -p gitraptor-desktop --test commands d3_cmds_engine_commands_are_listed_and_allowed -- --exact && cargo test -p gitraptor-desktop --test commands fleet_open_is_in_the_closed_list_and_the_capability -- --exact && cargo test -p gitraptor-desktop --test commands fleet_open_accepts_params_and_rejects_bad_ids -- --exact && cargo test -p gitraptor-desktop --test commands fleet_open_without_a_supervisor_is_not_watching -- --exact && cargo test -p gitraptor-desktop --test tauri_surface
tests:
  - apps/desktop/src-tauri/tests/commands.rs
  - apps/desktop/src-tauri/tests/tauri_surface.rs

[7] id: D4-CMD-TS
statement: commands.ts declara fleet_open con { repoId } y lo invoca con el envoltorio params
verify: pnpm --filter @gitraptor/desktop exec vitest run src/client/commands.test.ts -t 'fleet_open'
tests:
  - apps/desktop/src/client/commands.test.ts

[8] id: D4-WIRE
statement: El contrato Rust y TS de los mensajes del repo coincide con la fixture congelada (repo, worktrees, sessions, repoSession, prediction, repoClosed): engine_wire.rs solo compara y el test de la caché lee la misma fixture
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test engine_wire -- --list 2>/dev/null | grep -q '^every_variant_serializes_exactly_as_the_frozen_fixture: test' && cargo test -p gitraptor-desktop --test engine_wire every_variant_serializes_exactly_as_the_frozen_fixture -- --exact && pnpm --filter @gitraptor/desktop exec vitest run src/state/engine-cache.test.ts -t 'fleet fixture'
tests:
  - apps/desktop/src-tauri/tests/engine_wire.rs
  - apps/desktop/src/state/engine-cache.test.ts
  - apps/desktop/tests/fixtures/engine-messages.json

[9] id: D4-GATE
statement: El gate de seguridad de la DS sigue en verde con el código nuevo: V1 (dependencias), V7 (entorno), V2 (superficie Tauri) y V5 (src/ui no importa client) de ADR-GRP-017
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test boundaries --test environment --test tauri_surface && pnpm --filter @gitraptor/desktop exec vitest run tests/eslint-v5.test.ts
tests:
  - apps/desktop/src-tauri/tests/boundaries.rs
  - apps/desktop/src-tauri/tests/environment.rs
  - apps/desktop/src-tauri/tests/tauri_surface.rs
  - apps/desktop/tests/eslint-v5.test.ts

[10] id: D4-CACHE
statement: applyEngineBatch guarda la flota solo por mensajes y sin mutar: repo vacía sesiones, worktrees reemplaza, sessions y repoSession fusionan con la regla de D3 (una terminada con el mismo stateSince gana), prediction hace upsert y quita removed, repoClosed quita la entrada y deselecciona. El criterion-tester SUSTITUYE la aserción de US-DSK-003 «D3-CACHE: the keys are the Dev Spec's and nothing writes engineKeys.repo» (engine-cache.test.ts) por la regla nueva: solo applyEngineBatch escribe engineKeys.repo y useFleet lo lee sin queryFn de red
verify: pnpm --filter @gitraptor/desktop exec vitest run src/state/engine-cache.test.ts -t 'fleet cache' && pnpm --filter @gitraptor/desktop exec vitest run src/state/engine-cache.test.ts -t 'only applyEngineBatch writes engineKeys.repo' && pnpm --filter @gitraptor/desktop exec vitest run src/state/ui-store.test.ts -t 'selectedRepoId'
tests:
  - apps/desktop/src/state/engine-cache.test.ts
  - apps/desktop/src/state/ui-store.test.ts

[11] id: D4-USEFLEET
statement: useFleet elige el repo por defecto (primero por nombre y repoId) cuando llega global sin elegido o el elegido desaparece, llama fleet_open en cada cambio de selectedRepoId y lee la caché sin queryFn de red; selectedRepoId no se persiste
verify: pnpm --filter @gitraptor/desktop exec vitest run src/fleet/useFleet.test.tsx && pnpm --filter @gitraptor/desktop exec vitest run src/state/ui-store.test.ts -t 'selectedRepoId'
tests:
  - apps/desktop/src/fleet/useFleet.test.tsx

[12] id: D4-SHELL
statement: AppShell monta RepoTree en la barra lateral y EngineCenter pinta FleetView con el repo abierto (esqueleto con aria-busy sin instantánea del repo); la primera pintura útil con repo elegido espera la instantánea
verify: pnpm --filter @gitraptor/desktop exec vitest run src/shell/AppShell.test.tsx -t 'fleet shell' && pnpm --filter @gitraptor/desktop exec vitest run src/app/App.test.tsx -t 'repo snapshot'
tests:
  - apps/desktop/src/shell/AppShell.test.tsx
  - apps/desktop/src/app/App.test.tsx

[13] id: D4-FRESH
statement: El lote lleva receivedAt y FleetView mide gr:fleet-delta una sola vez por lote tras el primer requestAnimationFrame y marca gr:fleet-delta-painted; el backend manda perf.backendNs en worktrees, repoSession y prediction
verify: pnpm --filter @gitraptor/desktop exec vitest run src/fleet/FleetView.test.tsx -t 'measures each fleet delta once' && pnpm --filter @gitraptor/desktop exec vitest run src/client/engine-stream.test.ts -t 'receivedAt' && pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test fleet_supervisor -- --list 2>/dev/null | grep -q '^fleet_deltas_carry_backend_perf: test' && cargo test -p gitraptor-desktop --test fleet_supervisor fleet_deltas_carry_backend_perf -- --exact
tests:
  - apps/desktop/src/fleet/FleetView.test.tsx
  - apps/desktop/src/client/engine-stream.test.ts
  - apps/desktop/src-tauri/tests/fleet_supervisor.rs
```
<<<FIN-DATOS-NO-CONFIABLES 97f8120168e3>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES ded780d84597>>>
```text
(el plan no declara gaps ni supuestos)
```
<<<FIN-DATOS-NO-CONFIABLES ded780d84597>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES 80c5553574c9>>>
```text
docs/requirements/features/desktop/dev-specs/US-DSK-004-flota-en-vivo.md
```
<<<FIN-DATOS-NO-CONFIABLES 80c5553574c9>>>

planSha: `ee7f15f0b099b14b4976f2706fbb8d0e10e92b3632a52e64074964768755a670` (base de la cadena de re-planes)

Léelo en `US-DSK-004-flota-en-vivo.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 8feb470c4f9d>>>
```text
docs/dev-briefs/us-dsk-004-live-fleet.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 8feb470c4f9d>>>

contractSha: `32de944b7151ec31d51922d5e1491c5649e1863e5e6ef8ed493ed899d693fb5f`

## Tests de criterio

testsSha: `e897d053b54a242d1dec9103237826c249906f0249cf0337efe6ccd0b870d3ad`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES a5716f9b6df7>>>
```text
325351007d3cc5b64d345089f897ddba90f5c7660d48c040539c055055e89cef  apps/desktop/src-tauri/tests/boundaries.rs
56b565f2de9ca0ae6613b4cb35fd9447241bf73d402f256852cf744c565f68d0  apps/desktop/src-tauri/tests/commands.rs
8d0506a2cb90d7c31e1ac1df94eb10892be7c0c04025ff0b90280efd57c6829a  apps/desktop/src-tauri/tests/engine_wire.rs
8046e35a65526a9f73ce47139e019dcf7d780d2788e905e71fd3beadd31e1b39  apps/desktop/src-tauri/tests/environment.rs
b4dd3952618ad36a63a80f8fb31848df4f687be57f8aa87b643bfc35950a2841  apps/desktop/src-tauri/tests/fleet_ingest.rs
f221e9683909da087f64de6b591f01f9f566a82fad55cf99c38de2d9fec8572d  apps/desktop/src-tauri/tests/fleet_supervisor.rs
5d597f566a2a22d79d001b610ffeb57e3f2564a14e546fd1cec0795fe2f7ff27  apps/desktop/src-tauri/tests/support/fake_daemon.rs
18e667946a5206a78946d89d405870fd6f8a8556a2d3fbc2e27f65febf06e1dd  apps/desktop/src-tauri/tests/tauri_surface.rs
db2c3112123751416689ee89a544b84422b150816ad772a4467599f2d61eba84  apps/desktop/src/app/App.test.tsx
083308fe822233209343312fe63fa66487f81987c0e87df139ca33bcb006c441  apps/desktop/src/client/commands.test.ts
b3956edd49979e6301c765c9cb52f35101f9867a6cedecc8c77a7f21313dee99  apps/desktop/src/client/engine-stream.test.ts
d1e788aa525d8349ca7a2ce9a57e40fa325a4c4755e81bd6517a5b65c93f1da4  apps/desktop/src/fleet/FleetView.test.tsx
815087f28e0f094418139d95efb37ecae7fc9432c29e16b1998b2b0aecf3ca79  apps/desktop/src/fleet/useFleet.test.tsx
15378c1e1cff707637cae5aafc36b334c7534d78c8f84522fbeaadc4020b8c4e  apps/desktop/src/shell/AppShell.test.tsx
8ff99e74e5fc11e3b8af40befb4622cf899e304df5d5e7ee9de6017731e67ca5  apps/desktop/src/state/engine-cache.test.ts
ba644db53f65c03656dc747ad27f12d71af2ce18255348982daa88ccbabeb650  apps/desktop/src/state/ui-store.test.ts
d9c0f74655f9d95c9623f28aed0bd26fcd436c3e0d94248ef180076d70a2ed92  apps/desktop/tests/eslint-v5.test.ts
2d3f1227251f7c3472221b5aec6bf0d8d1c1232408d25fb274ca61b18f681687  apps/desktop/tests/fixtures/engine-messages.json
```
<<<FIN-DATOS-NO-CONFIABLES a5716f9b6df7>>>

## Crítico

- Rondas (plan): 4
  - ronda 1: BLOQUEO · 6 bloqueante(s) · siguiente: excepcion
  - ronda 2: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
  - ronda 3: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
  - ronda 4: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
- Último veredicto: APROBABLE
- Excepción: no

## Diferidos

Ninguno: el crítico dio el plan por aprobable.

## Firmas

- planSha (contexto, no firmado): `ee7f15f0b099b14b4976f2706fbb8d0e10e92b3632a52e64074964768755a670`
- contractSha: `32de944b7151ec31d51922d5e1491c5649e1863e5e6ef8ed493ed899d693fb5f`
- testsSha: `e897d053b54a242d1dec9103237826c249906f0249cf0337efe6ccd0b870d3ad`
- approvalSha: `ee7f15f0b099b14b4976f2706fbb8d0e10e92b3632a52e64074964768755a670:32de944b7151ec31d51922d5e1491c5649e1863e5e6ef8ed493ed899d693fb5f:e897d053b54a242d1dec9103237826c249906f0249cf0337efe6ccd0b870d3ad`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
