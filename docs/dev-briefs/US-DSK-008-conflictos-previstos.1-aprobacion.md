---
schema: nassa.approval-request.v1
plan: "docs/requirements/features/desktop/dev-specs/US-DSK-008-conflictos-previstos.md"
---

Lee este fichero y luego `US-DSK-008-conflictos-previstos.md`; `US-DSK-008-conflictos-previstos.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: no
- Generada: `2026-11-09T20:55:32.871Z`
- Sha de aprobación: `d2f81fa331c25e0a53073e4b58295ebc00380cb7622b62b91d2daaf7c0a8921e:6260875b236ae43e1706b47d70d1f3edfcecd8a737d3ac9b1dde6174dda35efd:01bb982fac5fbee9814c46106fa6426f85bb909f81884f608e136826a0f650d5`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES 010584878e7e>>>
```text
intent: Que quien tiene la app abierta en un repo vea qué pares de agentes (o un agente y la base) van a chocar, en qué archivos y líneas, con cuánta antigüedad, y reciba un aviso dentro de la app cuando aparece un ⚡ nuevo.
endState: El supervisor pide prediction.list por su cuenta (detalle por ids, relleno de cursor, un recorrido a la vez) y manda pairDetails; la caché aplica la regla de rev; el panel de conflictos, la banda, el detalle en el centro, la píldora y el toast pintan el modelo puro, en en/es, accesibles; el comando de confirmar la base lo construye el backend; la lista de comandos IPC no cambia.
nonGoals:
  - No añade comandos IPC ni métodos, eventos o capacidades al canal; todo es de solo lectura
  - No pinta la línea de la banda en el grafo (US-DSK-005) ni 'Copiar aviso para los agentes' ni 'Ver en el grafo'
  - No activa notificaciones del SO ni sonido
  - No resuelve G1, G2 (US-GRP-016, US-CKP-009), G4 (TS-CKP-010) ni G11 (forma de la píldora, PO)
  - No corre la suite completa local (la corre el CI) ni valida Windows/Linux
```
<<<FIN-DATOS-NO-CONFIABLES 010584878e7e>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES bef09adb6ca3>>>
```text
[1] id: D8-DETAIL
statement: Un par ⚡ recibe su detalle (archivos y rangos) por prediction.list sin comando IPC; un prediction.changed nuevo pide su detalle; un rev que se mueve lo pide otra vez; una ráfaga de cambios pide una sola llamada de ≤ 50 ids; un prediction.list rechazado deja los pares sin detalle y sigue el stream
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test prediction_supervisor -- --list 2>/dev/null | grep -q '^a_conflict_pair_gets_its_files_and_ranges_without_any_command: test' && cargo test -p gitraptor-desktop --test prediction_supervisor -- --list 2>/dev/null | grep -q '^a_new_conflict_event_asks_for_its_detail: test' && cargo test -p gitraptor-desktop --test prediction_supervisor -- --list 2>/dev/null | grep -q '^a_detail_is_asked_again_when_the_rev_moves: test' && cargo test -p gitraptor-desktop --test prediction_supervisor -- --list 2>/dev/null | grep -q '^a_burst_of_changes_asks_for_details_once_with_at_most_50_ids: test' && cargo test -p gitraptor-desktop --test prediction_supervisor -- --list 2>/dev/null | grep -q '^a_refused_list_leaves_pairs_without_detail_and_keeps_streaming: test' && cargo test -p gitraptor-desktop --test prediction_supervisor a_conflict_pair_gets_its_files_and_ranges_without_any_command -- --exact && cargo test -p gitraptor-desktop --test prediction_supervisor a_new_conflict_event_asks_for_its_detail -- --exact && cargo test -p gitraptor-desktop --test prediction_supervisor a_detail_is_asked_again_when_the_rev_moves -- --exact && cargo test -p gitraptor-desktop --test prediction_supervisor a_burst_of_changes_asks_for_details_once_with_at_most_50_ids -- --exact && cargo test -p gitraptor-desktop --test prediction_supervisor a_refused_list_leaves_pairs_without_detail_and_keeps_streaming -- --exact
tests:
  - apps/desktop/src-tauri/tests/prediction_supervisor.rs
  - apps/desktop/src-tauri/tests/support/fake_daemon.rs

[2] id: D8-FILL
statement: Una instantánea truncada recorre el cursor de prediction.list y termina con filled; un resync durante un recorrido nunca corre dos recorridos a la vez; un recorrido se detiene a los 2 MiB sin filled
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test prediction_supervisor -- --list 2>/dev/null | grep -q '^a_truncated_snapshot_walks_the_cursor_and_ends_filled: test' && cargo test -p gitraptor-desktop --test prediction_supervisor -- --list 2>/dev/null | grep -q '^a_resync_during_a_walk_never_runs_two_walks: test' && cargo test -p gitraptor-desktop --test prediction_supervisor -- --list 2>/dev/null | grep -q '^a_walk_stops_at_2_mib_without_filled: test' && cargo test -p gitraptor-desktop --test prediction_supervisor a_truncated_snapshot_walks_the_cursor_and_ends_filled -- --exact && cargo test -p gitraptor-desktop --test prediction_supervisor a_resync_during_a_walk_never_runs_two_walks -- --exact && cargo test -p gitraptor-desktop --test prediction_supervisor a_walk_stops_at_2_mib_without_filled -- --exact
tests:
  - apps/desktop/src-tauri/tests/prediction_supervisor.rs

[3] id: D8-BASE
statement: Una base movida llega al webview como pares calculating; un par contra la base pending con base-unconfirmed llega con su motivo; el mensaje del repo lleva el comando de confirmar con el raptor del daemon y la ruta del repo
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test prediction_supervisor -- --list 2>/dev/null | grep -q '^a_base_move_reaches_the_webview_as_calculating_pairs: test' && cargo test -p gitraptor-desktop --test prediction_supervisor -- --list 2>/dev/null | grep -q '^a_pending_base_pair_reaches_the_webview_with_its_reason: test' && cargo test -p gitraptor-desktop --test prediction_supervisor -- --list 2>/dev/null | grep -q '^the_repo_message_carries_the_confirm_command_with_the_daemon_raptor_and_the_repo_path: test' && cargo test -p gitraptor-desktop --test prediction_supervisor a_base_move_reaches_the_webview_as_calculating_pairs -- --exact && cargo test -p gitraptor-desktop --test prediction_supervisor a_pending_base_pair_reaches_the_webview_with_its_reason -- --exact && cargo test -p gitraptor-desktop --test prediction_supervisor the_repo_message_carries_the_confirm_command_with_the_daemon_raptor_and_the_repo_path -- --exact
tests:
  - apps/desktop/src-tauri/tests/prediction_supervisor.rs

[4] id: D8-SHELL
statement: shell_command: el comando de confirmar cita el raptor del daemon y la ruta cruda con comillas POSIX; una ruta con texto inseguro da el comando sin ruta; sin raptor conocido no se ofrece; la comilla se escribe como '\''; el raptor del daemon dentro de un worktree observado cae al del bundle; nunca PATH; wire::stop_command no cambia su salida
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test shell_command -- --list 2>/dev/null | grep -q '^the_confirm_command_quotes_the_daemon_raptor_and_the_raw_repo_path: test' && cargo test -p gitraptor-desktop --test shell_command -- --list 2>/dev/null | grep -q '^a_repo_path_with_unsafe_text_gives_the_command_without_path: test' && cargo test -p gitraptor-desktop --test shell_command -- --list 2>/dev/null | grep -q '^without_a_known_raptor_the_confirm_command_is_not_offered: test' && cargo test -p gitraptor-desktop --test shell_command -- --list 2>/dev/null | grep -q '^a_quote_is_written_as_posix_escape: test' && cargo test -p gitraptor-desktop --test shell_command -- --list 2>/dev/null | grep -q '^the_daemon_raptor_of_the_ready_report_is_used: test' && cargo test -p gitraptor-desktop --test shell_command -- --list 2>/dev/null | grep -q '^a_daemon_raptor_inside_a_watched_worktree_falls_back_to_the_bundle: test' && cargo test -p gitraptor-desktop --test shell_command -- --list 2>/dev/null | grep -q '^never_path: test' && cargo test -p gitraptor-desktop --test shell_command the_confirm_command_quotes_the_daemon_raptor_and_the_raw_repo_path -- --exact && cargo test -p gitraptor-desktop --test shell_command a_repo_path_with_unsafe_text_gives_the_command_without_path -- --exact && cargo test -p gitraptor-desktop --test shell_command without_a_known_raptor_the_confirm_command_is_not_offered -- --exact && cargo test -p gitraptor-desktop --test shell_command a_quote_is_written_as_posix_escape -- --exact && cargo test -p gitraptor-desktop --test shell_command the_daemon_raptor_of_the_ready_report_is_used -- --exact && cargo test -p gitraptor-desktop --test shell_command a_daemon_raptor_inside_a_watched_worktree_falls_back_to_the_bundle -- --exact && cargo test -p gitraptor-desktop --test shell_command never_path -- --exact && cargo test -p gitraptor-desktop --test engine_stop_command
tests:
  - apps/desktop/src-tauri/tests/shell_command.rs
  - apps/desktop/src-tauri/tests/engine_stop_command.rs

[5] id: D8-SAFE
statement: Una corrida de predicción nunca manda métodos reservados ni de escritura (assert_read_only); las rutas no confiables del detalle (OSC 52, U+202E) llegan escapadas
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test prediction_supervisor -- --list 2>/dev/null | grep -q '^a_prediction_run_never_sends_reserved_or_writing_methods: test' && cargo test -p gitraptor-desktop --test prediction_supervisor -- --list 2>/dev/null | grep -q '^untrusted_conflict_paths_reach_the_webview_escaped: test' && cargo test -p gitraptor-desktop --test prediction_supervisor a_prediction_run_never_sends_reserved_or_writing_methods -- --exact && cargo test -p gitraptor-desktop --test prediction_supervisor untrusted_conflict_paths_reach_the_webview_escaped -- --exact
tests:
  - apps/desktop/src-tauri/tests/prediction_supervisor.rs

[6] id: D8-WIRE
statement: El contrato Rust y TS coincide con la fixture congelada ampliada (PairDto con rev y frescura, limits, confirmCommand y pairDetails): engine_wire.rs compara y el test de la caché lee la misma fixture; ingest::pair copia computedAtUtcMs (negativo a null) y pair_detail sanea rutas
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test engine_wire && cargo test -p gitraptor-desktop --test fleet_ingest -- --list 2>/dev/null | grep -q '^a_negative_computed_at_becomes_null: test' && cargo test -p gitraptor-desktop --test fleet_ingest -- --list 2>/dev/null | grep -q '^pair_detail_paths_pass_through_display_visible: test' && cargo test -p gitraptor-desktop --test fleet_ingest a_negative_computed_at_becomes_null -- --exact && cargo test -p gitraptor-desktop --test fleet_ingest pair_detail_paths_pass_through_display_visible -- --exact && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/state/engine-cache.test.ts -t 'fleet fixture with pair details' --reporter=verbose 2>&1 | grep '✓.*fleet fixture with pair details' > /dev/null"
tests:
  - apps/desktop/src-tauri/tests/engine_wire.rs
  - apps/desktop/src-tauri/tests/fleet_ingest.rs
  - apps/desktop/src/state/engine-cache.test.ts
  - apps/desktop/tests/fixtures/engine-messages.json

[7] id: D8-CACHE
statement: La caché aplica la regla de rev: un resumen viejo nunca pisa uno nuevo; un detalle con rev mayor trae su resumen; filled limpia pairsTruncated; los detalles de un repo sin entrada se descartan; ui-store.center tiene un solo destino (abrir un par sustituye, backToMain vuelve, selectRepo vuelve a main, no se persiste)
verify: bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/state/engine-cache.test.ts -t 'an older summary never replaces a newer one' --reporter=verbose 2>&1 | grep '✓.*an older summary never replaces a newer one' > /dev/null" && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/state/engine-cache.test.ts -t 'a detail with a newer rev brings its summary' --reporter=verbose 2>&1 | grep '✓.*a detail with a newer rev brings its summary' > /dev/null" && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/state/engine-cache.test.ts -t 'filled clears pairsTruncated' --reporter=verbose 2>&1 | grep '✓.*filled clears pairsTruncated' > /dev/null" && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/state/engine-cache.test.ts -t 'pair details of a repo without a cache entry are dropped' --reporter=verbose 2>&1 | grep '✓.*pair details of a repo without a cache entry are dropped' > /dev/null" && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/state/ui-store.test.ts -t 'opening a pair replaces the center view and back returns to main' --reporter=verbose 2>&1 | grep '✓.*opening a pair replaces the center view and back returns to main' > /dev/null" && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/state/ui-store.test.ts -t 'selecting another repo returns to main' --reporter=verbose 2>&1 | grep '✓.*selecting another repo returns to main' > /dev/null"
tests:
  - apps/desktop/src/state/engine-cache.test.ts
  - apps/desktop/src/state/ui-store.test.ts

[8] id: D8-MODEL
statement: predictionView y pairDetailView aplican las reglas de frescura (recalculando, base movida con la antigüedad del último resultado, nada sin resultado dice sin conflictos, cero pares solo límites); nextAlerts aplica A5, E1 y E2 (sembrados, recalculating→current, resync, ausente de una truncada, removed y vuelta); el módulo público exporta solo lo del contrato D6
verify: pnpm --filter @gitraptor/desktop exec vitest run src/conflicts/prediction-model.test.ts && pnpm --filter @gitraptor/desktop exec vitest run src/conflicts/alerts.test.ts
tests:
  - apps/desktop/src/conflicts/prediction-model.test.ts
  - apps/desktop/src/conflicts/alerts.test.ts

[9] id: D8-BAND
statement: ConflictBand dice nivel, frescura y 'nuevo' en texto, oculta el glifo a la tecnología de apoyo, nombra el par con 'y', la variante badge no tiene botón, una banda sin detalle enseña el par sin archivo; CopyCommand anuncia 'Copiado' 2 s en una región status, anuncia el fallo y llama onCopied solo tras escribir
verify: pnpm --filter @gitraptor/desktop exec vitest run src/ui/ConflictBand.test.tsx && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/ui/CopyCommand.test.tsx -t 'a successful copy says Copied for 2 s and announces it once in a status region' --reporter=verbose 2>&1 | grep '✓.*a successful copy says Copied for 2 s and announces it once in a status region' > /dev/null" && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/ui/CopyCommand.test.tsx -t 'a failed copy announces it' --reporter=verbose 2>&1 | grep '✓.*a failed copy announces it' > /dev/null" && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/ui/CopyCommand.test.tsx -t 'onCopied runs only after a successful write' --reporter=verbose 2>&1 | grep '✓.*onCopied runs only after a successful write' > /dev/null"
tests:
  - apps/desktop/src/ui/ConflictBand.test.tsx
  - apps/desktop/src/ui/CopyCommand.test.tsx

[10] id: D8-PANEL
statement: El panel de conflictos va en el centro encima de la vista principal (no en FleetView), enseña el par ⚡ con su archivo, a lo sumo 3 bandas y 'y N más', la base pendiente con el comando copiable (con ruta, sin ruta, raptor desconocido) y la oculta si un par contra la base sale de pending; la píldora ⚡ de la barra de título suma los conflictos contados, abre el repo y conserva su nombre completo en modo compacto
verify: pnpm --filter @gitraptor/desktop exec vitest run src/conflicts/ConflictPanel.test.tsx && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/engine/EngineCenter.test.tsx -t 'the conflict panel sits above the main view and not inside the fleet' --reporter=verbose 2>&1 | grep '✓.*the conflict panel sits above the main view and not inside the fleet' > /dev/null" && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/shell/TitleBar.test.tsx -t 'the conflict pill sums counted conflicts and opens the repo' --reporter=verbose 2>&1 | grep '✓.*the conflict pill sums counted conflicts and opens the repo' > /dev/null" && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/shell/TitleBar.test.tsx -t 'the pill keeps its full name when compact' --reporter=verbose 2>&1 | grep '✓.*the pill keeps its full name when compact' > /dev/null"
tests:
  - apps/desktop/src/conflicts/ConflictPanel.test.tsx
  - apps/desktop/src/engine/EngineCenter.test.tsx
  - apps/desktop/src/shell/TitleBar.test.tsx

[11] id: D8-DETAILVIEW
statement: ConflictDetailView lista pagos.rs con las líneas de cada lado, pinta rutas como texto, no enseña un detalle de otro rev, y Esc cierra primero el popover y luego el detalle devolviendo el foco a la banda; el Esc consumido (Dialog, Popover, Toast marcan defaultPrevented) nunca llega a onEscape y un Esc dentro del centro no cierra además el cajón
verify: pnpm --filter @gitraptor/desktop exec vitest run src/conflicts/ConflictDetailView.test.tsx && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/shell/AppShell.test.tsx -t 'Esc inside the center view does not also close the drawer' --reporter=verbose 2>&1 | grep '✓.*Esc inside the center view does not also close the drawer' > /dev/null" && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/shell/AppShell.test.tsx -t 'an Esc already consumed never reaches onEscape' --reporter=verbose 2>&1 | grep '✓.*an Esc already consumed never reaches onEscape' > /dev/null" && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/ui/Dialog.test.tsx -t 'the Esc that closes it is marked as consumed' --reporter=verbose 2>&1 | grep '✓.*the Esc that closes it is marked as consumed' > /dev/null" && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/ui/Popover.test.tsx -t 'the Esc that closes it is marked as consumed' --reporter=verbose 2>&1 | grep '✓.*the Esc that closes it is marked as consumed' > /dev/null" && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/ui/Toast.test.tsx -t 'the Esc that closes it is marked as consumed' --reporter=verbose 2>&1 | grep '✓.*the Esc that closes it is marked as consumed' > /dev/null"
tests:
  - apps/desktop/src/conflicts/ConflictDetailView.test.tsx
  - apps/desktop/src/shell/AppShell.test.tsx
  - apps/desktop/src/ui/Dialog.test.tsx
  - apps/desktop/src/ui/Popover.test.tsx
  - apps/desktop/src/ui/Toast.test.tsx

[12] id: D8-ALERTS
statement: Un ⚡ nuevo muestra un toast info con el par y el primer archivo (espera el archivo hasta 2 s), se retira si el par deja de ser ⚡, tres o más nuevos en una pasada dan un solo toast, un recuento que sube en otro repo avisa sin nombres de par
verify: pnpm --filter @gitraptor/desktop exec vitest run src/conflicts/useConflictAlerts.test.tsx
tests:
  - apps/desktop/src/conflicts/useConflictAlerts.test.tsx

[13] id: D8-GATE
statement: Gate de seguridad en verde con el código nuevo (V1, V7, V2 y V5 de ADR-GRP-017); esta historia no añade comandos IPC (ningún nombre con prediction, pair ni conflict; la lista exacta app_info, engine_retry, engine_watch, fleet_open sigue igual; commands.ts sin comandos de predicción); sin notificaciones del SO (ni tauri-plugin-notification, ni new Notification( ni requestPermission en src/); y el catálogo en/es tiene las claves conflicts.* y ui.common.copied/copyFailed
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test boundaries --test environment --test tauri_surface && pnpm --filter @gitraptor/desktop exec vitest run tests/eslint-v5.test.ts && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/i18n/catalog.test.ts -t 'has the predicted conflict keys in en and es' --reporter=verbose 2>&1 | grep '✓.*has the predicted conflict keys in en and es' > /dev/null" && cargo test -p gitraptor-desktop --test commands -- --list 2>/dev/null | grep -q '^the_prediction_detail_adds_no_ipc_command: test' && cargo test -p gitraptor-desktop --test commands -- --list 2>/dev/null | grep -q '^d3_cmds_engine_commands_are_listed_and_allowed: test' && cargo test -p gitraptor-desktop --test commands the_prediction_detail_adds_no_ipc_command -- --exact && cargo test -p gitraptor-desktop --test commands d3_cmds_engine_commands_are_listed_and_allowed -- --exact && bash -c "set -o pipefail; pnpm --filter @gitraptor/desktop exec vitest run src/client/commands.test.ts -t 'no command of the command map is about prediction' --reporter=verbose 2>&1 | grep '✓.*no command of the command map is about prediction' > /dev/null" && cargo test -p gitraptor-desktop --test boundaries -- --list 2>/dev/null | grep -q '^no_os_notifications: test' && cargo test -p gitraptor-desktop --test boundaries no_os_notifications -- --exact
tests:
  - apps/desktop/src-tauri/tests/boundaries.rs
  - apps/desktop/src-tauri/tests/environment.rs
  - apps/desktop/src-tauri/tests/tauri_surface.rs
  - apps/desktop/tests/eslint-v5.test.ts
  - apps/desktop/src/i18n/catalog.test.ts
  - apps/desktop/src-tauri/tests/commands.rs
  - apps/desktop/src/client/commands.test.ts
```
<<<FIN-DATOS-NO-CONFIABLES bef09adb6ca3>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES ded780d84597>>>
```text
(el plan no declara gaps ni supuestos)
```
<<<FIN-DATOS-NO-CONFIABLES ded780d84597>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES 4067d8703580>>>
```text
docs/requirements/features/desktop/dev-specs/US-DSK-008-conflictos-previstos.md
```
<<<FIN-DATOS-NO-CONFIABLES 4067d8703580>>>

planSha: `d2f81fa331c25e0a53073e4b58295ebc00380cb7622b62b91d2daaf7c0a8921e` (base de la cadena de re-planes)

Léelo en `US-DSK-008-conflictos-previstos.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES b431c68978af>>>
```text
docs/dev-briefs/us-dsk-008-predicted-conflicts.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES b431c68978af>>>

contractSha: `6260875b236ae43e1706b47d70d1f3edfcecd8a737d3ac9b1dde6174dda35efd`

## Tests de criterio

testsSha: `01bb982fac5fbee9814c46106fa6426f85bb909f81884f608e136826a0f650d5`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES 07e6453d33f1>>>
```text
094369187aa7f32421a4246c7041d98195b0749c822a6a48392e3ce585e071c8  apps/desktop/src-tauri/tests/boundaries.rs
becba4bd2a62437d982eceaa28284fda5b9e04d9660649f96d0fc66276bcf61b  apps/desktop/src-tauri/tests/commands.rs
b9d38a36e9c21ecdaac3895fd72172b521d82732fd5ffcbea60b08ae2938c263  apps/desktop/src-tauri/tests/engine_stop_command.rs
ff0ddb7b1ab1108c8ddeee859d3e1e822cfc7c5871a5191fbf765e83bbbed0ed  apps/desktop/src-tauri/tests/engine_wire.rs
8046e35a65526a9f73ce47139e019dcf7d780d2788e905e71fd3beadd31e1b39  apps/desktop/src-tauri/tests/environment.rs
13d29201006ae718f90c24a0415c3a8441998f74dac7507aa7c9183da1c3bba3  apps/desktop/src-tauri/tests/fleet_ingest.rs
d166c349dc494ec220f0e4a1871d37e1c921bf9e9d343b6754c03ae8edb44486  apps/desktop/src-tauri/tests/prediction_supervisor.rs
5907b2631a1273b24d378a355e0a00beebc8ac7dbe621a4c40a225fe6193a130  apps/desktop/src-tauri/tests/shell_command.rs
488e0a7f158a2bbe39e001933beec645088837a3dea12160326d06144525353e  apps/desktop/src-tauri/tests/support/fake_daemon.rs
18e667946a5206a78946d89d405870fd6f8a8556a2d3fbc2e27f65febf06e1dd  apps/desktop/src-tauri/tests/tauri_surface.rs
4988dfacc506a18da44820d987c3cf117b707ab1a526ab26b03157430571f072  apps/desktop/src/client/commands.test.ts
047423b29d4522a4e26aa51bd218ee62673ee7cc71d0dcf9e1bf300d3dd41872  apps/desktop/src/conflicts/ConflictDetailView.test.tsx
20fd72920a819a0bac445f0fcaa41c9500df4540fff6f61d38fc46c26a1b564b  apps/desktop/src/conflicts/ConflictPanel.test.tsx
41ec050ef7a3993c94784bd9406635d8e02de37c452c349f3a2c5a9954cd73a8  apps/desktop/src/conflicts/alerts.test.ts
68084d539617a5063ea1b7cd795db4edea1291f82d85628ea3c28809e0766f6d  apps/desktop/src/conflicts/prediction-model.test.ts
394443d5386681d473dfc90211e2f32fdd81abbd5af422336e3556f98dec8862  apps/desktop/src/conflicts/useConflictAlerts.test.tsx
a497369a0cd63dbbf1062f67a988bc78bcde02cb186923f82205984c4a47ffd7  apps/desktop/src/engine/EngineCenter.test.tsx
a6bb048e6d1d3b3e25252fba097e26709172ce07c1d363255970d86d178d9210  apps/desktop/src/i18n/catalog.test.ts
5deeac93c0d46ffa3d2502d03821f343fa2f753395e71f5abdc19718c5afcff8  apps/desktop/src/shell/AppShell.test.tsx
d273a50b9a37dea925afdb08af9ca0e8e46d42cedee5c6f29404e1a553633ec1  apps/desktop/src/shell/TitleBar.test.tsx
2f3744cd0bbadffcafbda0882790577881509fd9b8bb2775fdf62e3042331392  apps/desktop/src/state/engine-cache.test.ts
6596b8d7f2d8cc56bf179a6e941e5f52f7b28696d69288313bbcd24ad4e8aa9d  apps/desktop/src/state/ui-store.test.ts
3664ae885a2827c728fd47c3c2d0f43374dda55b702efc5a632cb5b16127c4c0  apps/desktop/src/ui/ConflictBand.test.tsx
bd9eef2f9f159f44a1c7b32bfc2aa055f774eab3ff604d26eaee44cff2a579da  apps/desktop/src/ui/CopyCommand.test.tsx
d030f1d23edba1effbb68cc09b5afb114ea40aeca602d03d05738207c49480a1  apps/desktop/src/ui/Dialog.test.tsx
b6d832a0ec230ab7736cd21299872b5f0f1c794a03b42b26b14f8c086fe6fa2a  apps/desktop/src/ui/Popover.test.tsx
176ea0e876eb48ed6dd67ae6d5dc60c4e359393a9468b7b76f78844c2eeef829  apps/desktop/src/ui/Toast.test.tsx
d9c0f74655f9d95c9623f28aed0bd26fcd436c3e0d94248ef180076d70a2ed92  apps/desktop/tests/eslint-v5.test.ts
a206825b3a9aca00cb93c9c46be8ac29b1cd6e4ea5538f88eb90a93258a74216  apps/desktop/tests/fixtures/engine-messages.json
```
<<<FIN-DATOS-NO-CONFIABLES 07e6453d33f1>>>

## Crítico

- Rondas (plan): 2
  - ronda 1: AJUSTES · 2 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
- Último veredicto: APROBABLE
- Excepción: no

## Diferidos

Ninguno: el crítico dio el plan por aprobable.

## Firmas

- planSha (contexto, no firmado): `d2f81fa331c25e0a53073e4b58295ebc00380cb7622b62b91d2daaf7c0a8921e`
- contractSha: `6260875b236ae43e1706b47d70d1f3edfcecd8a737d3ac9b1dde6174dda35efd`
- testsSha: `01bb982fac5fbee9814c46106fa6426f85bb909f81884f608e136826a0f650d5`
- approvalSha: `d2f81fa331c25e0a53073e4b58295ebc00380cb7622b62b91d2daaf7c0a8921e:6260875b236ae43e1706b47d70d1f3edfcecd8a737d3ac9b1dde6174dda35efd:01bb982fac5fbee9814c46106fa6426f85bb909f81884f608e136826a0f650d5`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
