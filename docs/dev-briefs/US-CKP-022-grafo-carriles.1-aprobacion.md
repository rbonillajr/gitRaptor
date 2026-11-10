---
schema: nassa.approval-request.v1
plan: "docs/requirements/features/cockpit/dev-specs/US-CKP-022-grafo-carriles.md"
---

Lee este fichero y luego `US-CKP-022-grafo-carriles.md`; `US-CKP-022-grafo-carriles.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: **sí** — el crítico agotó sus rondas en la ronda 2 sin un plan aprobable; ver «Diferidos»
- Generada: `2026-11-10T20:03:42.995Z`
- Sha de aprobación: `c2caf77c300f1d4a2ddae4fad78d6aa175be7dcfe5ed8a201192add8f89c952e:3740b7b4aec810e1ccba21dba27469c09aeb6ed9f8dc208d8846f811815efb88:a4faf33fa320055df951ec6965045c77c692a47ec78875dd059a0c71190a21f3`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES 3eba5d15bb66>>>
```text
intent: El desarrollador ve bajo la flota un carril por rama de agente y, con g, el grafo a pantalla completa por graph.lanes (consulta de solo lectura, fuera del MCP) que desbloquea US-DSK-005
endState: graph.lanes responde por el QueryPool con base confirmada solamente, la TUI pinta panel y vista conectados al widget GraphLanes, y los cinco escenarios de US-CKP-022 pasan
nonGoals:
  - Exponer graph.lanes al MCP
  - Actor por commit
  - Desplegar o paginar 'N más'
  - Cablear LaneModel.conflict
```
<<<FIN-DATOS-NO-CONFIABLES 3eba5d15bb66>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 52a6d1112431>>>
```text
[1] id: CKP022-API-METHOD
statement: graph.lanes está declarado y en GROUPS, sin error_block propio, con mcp false (no se ofrece a raptor-mcp) y con .since(CAPABILITIES_PROTOCOL) (D2, D14)
verify: cargo test -p gitraptor-api --test us_ckp_022_wire -- --list 2>/dev/null | grep -q 'graph_lanes_is_declared_for_the_full_profile_only: test$' && cargo test -p gitraptor-api --test us_ckp_022_wire graph_lanes_is_declared_for_the_full_profile_only
tests:
  - crates/api/tests/us_ckp_022_wire.rs

[2] id: CKP022-API-WIRE
statement: Los tipos de crates/api/src/graph.rs serializan como los dos ejemplos del contrato de API de la DS (kebab-case, deny_unknown_fields)
verify: cargo test -p gitraptor-api --test us_ckp_022_wire -- --list 2>/dev/null | grep -q 'graph_lanes_wire_matches_the_contract_example: test$' && cargo test -p gitraptor-api --test us_ckp_022_wire graph_lanes_wire_matches_the_contract_example
tests:
  - crates/api/tests/us_ckp_022_wire.rs

[3] id: CKP022-API-NOACTOR
statement: Ningún tipo del contrato lleva actor, sesión, autor ni correo, y un campo actor se rechaza (D6, D7)
verify: cargo test -p gitraptor-api --test us_ckp_022_wire -- --list 2>/dev/null | grep -q 'graph_types_carry_no_actor_author_or_email: test$' && cargo test -p gitraptor-api --test us_ckp_022_wire graph_types_carry_no_actor_author_or_email
tests:
  - crates/api/tests/us_ckp_022_wire.rs

[4] id: CKP022-GIT-WALK
statement: RepoReader::lane devuelve HEAD menos la base, del commit más reciente al más antiguo, con merge-base y flag merge (D4)
verify: cargo test -p gitraptor-git --test us_ckp_022_lane -- --list 2>/dev/null | grep -q 'lane_walks_head_minus_base_newest_first: test$' && cargo test -p gitraptor-git --test us_ckp_022_lane lane_walks_head_minus_base_newest_first
tests:
  - crates/git/tests/us_ckp_022_lane.rs

[5] id: CKP022-GIT-TOTAL
statement: El total es exacto hasta max_walk y pasa a AtLeast después (D4, D12)
verify: cargo test -p gitraptor-git --test us_ckp_022_lane -- --list 2>/dev/null | grep -q 'lane_total_is_exact_until_the_walk_limit: test$' && cargo test -p gitraptor-git --test us_ckp_022_lane lane_total_is_exact_until_the_walk_limit
tests:
  - crates/git/tests/us_ckp_022_lane.rs

[6] id: CKP022-GIT-NOBASE
statement: Sin merge-base lane devuelve GitLaneGap::NoMergeBase (D16) y no recorre nada
verify: cargo test -p gitraptor-git --test us_ckp_022_lane -- --list 2>/dev/null | grep -q 'lane_without_merge_base_is_a_gap: test$' && cargo test -p gitraptor-git --test us_ckp_022_lane lane_without_merge_base_is_a_gap
tests:
  - crates/git/tests/us_ckp_022_lane.rs

[7] id: CKP022-GIT-SUMMARY
statement: El asunto es la primera línea cortada a max_summary_bytes en límite de carácter UTF-8, sin autor ni correo (D6)
verify: cargo test -p gitraptor-git --test us_ckp_022_lane -- --list 2>/dev/null | grep -q 'summary_is_the_first_line_cut_on_a_char_boundary: test$' && cargo test -p gitraptor-git --test us_ckp_022_lane summary_is_the_first_line_cut_on_a_char_boundary
tests:
  - crates/git/tests/us_ckp_022_lane.rs

[8] id: CKP022-GIT-SAMETOTAL
statement: El total de lane coincide con ahead_behind_commits (pública, reader.rs), el mismo conjunto que cuenta el ↑ del stream; only_in_walk es compartida pero privada al crate (D4)
verify: cargo test -p gitraptor-git --test us_ckp_022_lane -- --list 2>/dev/null | grep -q 'lane_total_equals_ahead_behind_commits: test$' && cargo test -p gitraptor-git --test us_ckp_022_lane lane_total_equals_ahead_behind_commits
tests:
  - crates/git/tests/us_ckp_022_lane.rs

[9] id: CKP022-GIT-READONLY
statement: lane no escribe índice, refs, config ni objetos (NFR-01)
verify: cargo test -p gitraptor-git --test us_ckp_022_lane -- --list 2>/dev/null | grep -q 'lane_writes_nothing: test$' && cargo test -p gitraptor-git --test us_ckp_022_lane lane_writes_nothing
tests:
  - crates/git/tests/us_ckp_022_lane.rs

[10] id: CKP022-GIT-STOP
statement: Si stop() devuelve true a mitad del recorrido el resultado es TooSlow, nunca una ventana parcial
verify: cargo test -p gitraptor-git --test us_ckp_022_lane -- --list 2>/dev/null | grep -q 'a_stop_mid_walk_is_too_slow_never_a_partial_window: test$' && cargo test -p gitraptor-git --test us_ckp_022_lane a_stop_mid_walk_is_too_slow_never_a_partial_window
tests:
  - crates/git/tests/us_ckp_022_lane.rs

[11] id: CKP022-CORE-TOTAL
statement: El total de cada carril coincide con el ↑ (DivergenceView.ahead) del stream para el mismo repo, y lanes_total es el número de carriles (D4, D12)
verify: cargo test -p gitraptor-core --test graph_query -- --list 2>/dev/null | grep -q 'lane_totals_match_the_ahead_of_the_stream: test$' && cargo test -p gitraptor-core --test graph_query lane_totals_match_the_ahead_of_the_stream
tests:
  - crates/core/tests/graph_query.rs

[12] id: CKP022-CORE-UNCONF
statement: Base sin confirmar: base not-computable base-unconfirmed y lanes vacío (D5, BR-CKP-CALC-004)
verify: cargo test -p gitraptor-core --test graph_query -- --list 2>/dev/null | grep -q 'unconfirmed_base_has_no_lanes: test$' && cargo test -p gitraptor-core --test graph_query unconfirmed_base_has_no_lanes
tests:
  - crates/core/tests/graph_query.rs

[13] id: CKP022-CORE-MISSING
statement: Base sin ref (develop): not-computable base-missing, sin carriles y sin usar otra rama (BR-CKP-EDGE-001)
verify: cargo test -p gitraptor-core --test graph_query -- --list 2>/dev/null | grep -q 'missing_base_is_not_computable_without_lanes: test$' && cargo test -p gitraptor-core --test graph_query missing_base_is_not_computable_without_lanes
tests:
  - crates/core/tests/graph_query.rs

[14] id: CKP022-CORE-TRUNK
statement: El worktree con la rama base sacada no es un carril (D3)
verify: cargo test -p gitraptor-core --test graph_query -- --list 2>/dev/null | grep -q 'the_base_worktree_is_not_a_lane: test$' && cargo test -p gitraptor-core --test graph_query the_base_worktree_is_not_a_lane
tests:
  - crates/core/tests/graph_query.rs

[15] id: CKP022-CORE-STATES
statement: Worktree no disponible, rama sin nacer y HEAD separado son carriles con su motivo o su nombre de carpeta (D3)
verify: cargo test -p gitraptor-core --test graph_query -- --list 2>/dev/null | grep -q 'unavailable_and_unborn_worktrees_are_lanes_with_a_reason: test$' && cargo test -p gitraptor-core --test graph_query unavailable_and_unborn_worktrees_are_lanes_with_a_reason
tests:
  - crates/core/tests/graph_query.rs

[16] id: CKP022-CORE-CANCEL
statement: query.cancel de una consulta graph.lanes en vuelo responde query-cancelled
verify: cargo test -p gitraptor-core --test graph_query -- --list 2>/dev/null | grep -q 'cancel_answers_query_cancelled: test$' && cargo test -p gitraptor-core --test graph_query cancel_answers_query_cancelled
tests:
  - crates/core/tests/graph_query.rs

[17] id: CKP022-CORE-BUSY
statement: Una segunda graph.lanes en la conexión, o la cola llena, responde query-busy
verify: cargo test -p gitraptor-core --test graph_query -- --list 2>/dev/null | grep -q 'a_second_request_on_the_connection_is_query_busy: test$' && cargo test -p gitraptor-core --test graph_query a_second_request_on_the_connection_is_query_busy
tests:
  - crates/core/tests/graph_query.rs

[18] id: CKP022-CORE-NOTFOUND
statement: Con el motor en Observing, un repo_id desconocido responde NOT_FOUND al momento (D19)
verify: cargo test -p gitraptor-core --test graph_query -- --list 2>/dev/null | grep -q 'unknown_repo_is_not_found: test$' && cargo test -p gitraptor-core --test graph_query unknown_repo_is_not_found
tests:
  - crates/core/tests/graph_query.rs

[19] id: CKP022-CORE-UNAVAILABLE
statement: Un repo conocido que aún no está observado (o antes de Observing) responde resultado con base not-computable repo-unavailable y sin carriles, nunca un error (D19)
verify: cargo test -p gitraptor-core --test graph_query -- --list 2>/dev/null | grep -q 'a_repo_not_yet_observed_is_repo_unavailable: test$' && cargo test -p gitraptor-core --test graph_query a_repo_not_yet_observed_is_repo_unavailable
tests:
  - crates/core/tests/graph_query.rs

[20] id: CKP022-CORE-MCP
statement: Una conexión MCP no ve graph.lanes (METHOD_NOT_FOUND) (mcp: false, P27 de DS-US-DSK-009)
verify: cargo test -p gitraptor-core --test graph_query -- --list 2>/dev/null | grep -q 'mcp_connection_does_not_see_graph_lanes: test$' && cargo test -p gitraptor-core --test graph_query mcp_connection_does_not_see_graph_lanes
tests:
  - crates/core/tests/graph_query.rs

[21] id: CKP022-CORE-NOLOG
statement: La respuesta no pasa por el logger, la auditoría ni el perfil: asuntos con un token plantado no aparecen en perfil ni logs (SEC-12): el token plantado en los asuntos no aparece en el log del watcher, el perfil ni los logs del motor
verify: cargo test -p gitraptor-core --test graph_query -- --list 2>/dev/null | grep -q 'responses_are_never_logged_nor_persisted: test$' && cargo test -p gitraptor-core --test graph_query responses_are_never_logged_nor_persisted
tests:
  - crates/core/tests/graph_query.rs

[22] id: CKP022-CORE-LIMITS
statement: Los topes (32 carriles, 768 KiB, plazo) cortan con cut o too-slow y nunca fallan (D12)
verify: cargo test -p gitraptor-core --test graph_query -- --list 2>/dev/null | grep -q 'limits_cut_and_never_fail: test$' && cargo test -p gitraptor-core --test graph_query limits_cut_and_never_fail
tests:
  - crates/core/tests/graph_query.rs

[23] id: CKP022-CORE-CACHE
statement: Un segundo refresco con el mismo HEAD y la misma base no recorre el carril (contador de recorridos): acierta la caché; cambiar HEAD o la base la invalida (D9)
verify: cargo test -p gitraptor-core --lib -- --list 2>/dev/null | grep -q 'second_refresh_hits_the_lane_cache: test$' && cargo test -p gitraptor-core --lib second_refresh_hits_the_lane_cache
tests:
  - crates/core/src/channel/graph_tests.rs

[24] id: CKP022-CLI-REFRESH
statement: El refresco de la vista espera max(1 s, 2 × la última duración) y solo hay una petición en vuelo (D9)
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'refresh_spacing_is_twice_the_last_duration_and_one_in_flight: test$' && cargo test -p gitraptor-cli --lib refresh_spacing_is_twice_the_last_duration_and_one_in_flight
tests:
  - apps/cli/src/tui/graph_tests.rs

[25] id: CKP022-CLI-STALE
statement: Un snapshot sin cambios de rama, divergencia, base o worktrees no marca la vista como desactualizada; un git.event sí (D9)
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'a_snapshot_without_changes_does_not_mark_stale: test$' && cargo test -p gitraptor-cli --lib a_snapshot_without_changes_does_not_mark_stale
tests:
  - apps/cli/src/tui/graph_tests.rs

[26] id: CKP022-CLI-ANCHOR
statement: El desplazamiento se ancla al primer commit visible y sobrevive a commits nuevos arriba (D9)
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'anchor_survives_new_commits_on_top: test$' && cargo test -p gitraptor-cli --lib anchor_survives_new_commits_on_top
tests:
  - apps/cli/src/tui/graph_tests.rs

[27] id: CKP022-CLI-TICKET
statement: Una respuesta de un ticket que no es el actual se descarta
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'a_graph_reply_for_an_old_ticket_is_dropped: test$' && cargo test -p gitraptor-cli --lib a_graph_reply_for_an_old_ticket_is_dropped
tests:
  - apps/cli/src/tui/graph_tests.rs

[28] id: CKP022-CLI-CLOSE
statement: Cerrar la vista con la petición en vuelo emite Cmd::CancelQuery
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'closing_the_graph_while_loading_cancels: test$' && cargo test -p gitraptor-cli --lib closing_the_graph_while_loading_cancels
tests:
  - apps/cli/src/tui/graph_tests.rs

[29] id: CKP022-CLI-G
statement: g sin repo no hace nada; con el motor sin graph.lanes da Notice::GraphUnsupported y ningún Cmd; con el diff abierto no hace nada (D11, D20)
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'g_is_inert_without_a_repo_the_capability_or_with_the_diff_open: test$' && cargo test -p gitraptor-cli --lib g_is_inert_without_a_repo_the_capability_or_with_the_diff_open
tests:
  - apps/cli/src/tui/graph_tests.rs

[30] id: CKP022-CLI-CLIENT-REPLY
statement: El hilo del canal pide graph.lanes y entrega EngineMsg::Graph con el resultado por ticket (T004)
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'a_graph_reply_reaches_update_by_ticket: test$' && cargo test -p gitraptor-cli --lib a_graph_reply_reaches_update_by_ticket
tests:
  - apps/cli/src/client/tests/graph.rs

[31] id: CKP022-CLI-CLIENT-FAIL
statement: query-busy y query-cancelled se entregan como GraphFailure::Busy y ::Cancelled (D15)
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'busy_and_cancelled_map_to_graph_failure: test$' && cargo test -p gitraptor-cli --lib busy_and_cancelled_map_to_graph_failure
tests:
  - apps/cli/src/client/tests/graph.rs

[32] id: CKP022-CLI-CLIENT-LOST
statement: Una reconexión entrega GraphFailure::Lost por cada petición en vuelo
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'a_lost_connection_answers_lost: test$' && cargo test -p gitraptor-cli --lib a_lost_connection_answers_lost
tests:
  - apps/cli/src/client/tests/graph.rs

[33] id: CKP022-CLI-CLIENT-CAP
statement: Al empezar la sesión el hilo anuncia ConnEvent::Graph(link.offers(GRAPH_LANES)): un motor sin el método se anuncia
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'an_engine_without_graph_lanes_is_announced: test$' && cargo test -p gitraptor-cli --lib an_engine_without_graph_lanes_is_announced
tests:
  - apps/cli/src/client/tests/graph.rs

[34] id: CKP022-CLI-PANEL
statement: El panel sigue el orden de la flota y el color de la sesión; sin sesión, carril sin atribuir (D3, D8)
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'panel_follows_fleet_order_and_session_color: test$' && cargo test -p gitraptor-cli --lib panel_follows_fleet_order_and_session_color
tests:
  - apps/cli/src/present/graph_tests.rs

[35] id: CKP022-CLI-SAFE
statement: Ramas, rutas y asuntos con controles (OSC 52, RLO, U+2028) se saneán antes de pintarse (SEC-12)
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'summaries_with_controls_are_sanitized: test$' && cargo test -p gitraptor-cli --lib summaries_with_controls_are_sanitized
tests:
  - apps/cli/src/present/graph_tests.rs

[36] id: CKP022-CLI-BASEMSG
statement: Los mensajes de base no calculable son exactos: develop no encontrada, sin confirmar con raptor guard confirm, sin rama base (D5)
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'base_not_computable_messages_are_exact: test$' && cargo test -p gitraptor-cli --lib base_not_computable_messages_are_exact
tests:
  - apps/cli/src/present/graph_tests.rs

[37] id: CKP022-CLI-TEXT
statement: Cada GraphText tiene texto en en y es
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'every_graph_text_has_both_languages: test$' && cargo test -p gitraptor-cli --lib every_graph_text_has_both_languages
tests:
  - apps/cli/src/present/graph_text_tests.rs

[38] id: CKP022-CLI-YIELD
statement: A 80×24 con panel de conflictos el grafo cede antes que él y la lista conserva LIST_MIN (D10, G2)
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'the_graph_yields_before_the_conflict_panel_at_80x24: test$' && cargo test -p gitraptor-cli --lib the_graph_yields_before_the_conflict_panel_at_80x24
tests:
  - apps/cli/src/tui/view/graph_tests.rs

[39] id: CKP022-CLI-SNAPS
statement: Snapshots del panel 100×30 y de la vista 80×24 en en, es y ASCII
verify: cargo test -p gitraptor-cli --lib -- --list 2>/dev/null | grep -q 'graph_snapshots_panel_100x30_and_view_80x24: test$' && cargo test -p gitraptor-cli --lib graph_snapshots_panel_100x30_and_view_80x24
tests:
  - apps/cli/src/tui/view/graph_tests.rs

[40] id: CKP022-E2E-1
statement: Escenario 1: un carril por worktree desde su merge-base, claude-1 y claude-2 con su agent.n, sin carril para main
verify: cargo test -p gitraptor-cli --test tui_graph -- --list 2>/dev/null | grep -q 'one_lane_per_worktree_from_its_merge_base: test$' && cargo test -p gitraptor-cli --test tui_graph one_lane_per_worktree_from_its_merge_base
tests:
  - apps/cli/tests/tui_graph.rs

[41] id: CKP022-E2E-2
statement: Escenario 2: feat-pagos muestra 50 commits y '22 más'
verify: cargo test -p gitraptor-cli --test tui_graph -- --list 2>/dev/null | grep -q 'a_lane_shows_50_commits_and_22_more: test$' && cargo test -p gitraptor-cli --test tui_graph a_lane_shows_50_commits_and_22_more
tests:
  - apps/cli/tests/tui_graph.rs

[42] id: CKP022-E2E-3
statement: Escenario 3: cada commit dice 'sin atribuir' y el nodo lleva el color del carril
verify: cargo test -p gitraptor-cli --test tui_graph -- --list 2>/dev/null | grep -q 'every_commit_is_unattributed_and_keeps_the_lane_color: test$' && cargo test -p gitraptor-cli --test tui_graph every_commit_is_unattributed_and_keeps_the_lane_color
tests:
  - apps/cli/tests/tui_graph.rs

[43] id: CKP022-E2E-4
statement: Escenario 4: base develop inexistente, ningún carril y 'Rama base develop no encontrada: no calculable' en vista y panel
verify: cargo test -p gitraptor-cli --test tui_graph -- --list 2>/dev/null | grep -q 'missing_base_shows_no_lanes_and_says_why: test$' && cargo test -p gitraptor-cli --test tui_graph missing_base_shows_no_lanes_and_says_why
tests:
  - apps/cli/tests/tui_graph.rs

[44] id: CKP022-E2E-5
statement: Escenario 5: a 80×24 con 12 filas en la flota (sin alertas: el repo de prueba no predice conflictos), la lista conserva su mínimo, la pista g grafo está en la barra y g abre la vista a pantalla completa (que el grafo cede ante el panel de conflictos lo fija CKP022-CLI-YIELD)
verify: cargo test -p gitraptor-cli --test tui_graph -- --list 2>/dev/null | grep -q 'the_graph_yields_space_first_at_80x24: test$' && cargo test -p gitraptor-cli --test tui_graph the_graph_yields_space_first_at_80x24
tests:
  - apps/cli/tests/tui_graph.rs
```
<<<FIN-DATOS-NO-CONFIABLES 52a6d1112431>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES ded780d84597>>>
```text
(el plan no declara gaps ni supuestos)
```
<<<FIN-DATOS-NO-CONFIABLES ded780d84597>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES d80d8ce0e55f>>>
```text
docs/requirements/features/cockpit/dev-specs/US-CKP-022-grafo-carriles.md
```
<<<FIN-DATOS-NO-CONFIABLES d80d8ce0e55f>>>

planSha: `c2caf77c300f1d4a2ddae4fad78d6aa175be7dcfe5ed8a201192add8f89c952e` (base de la cadena de re-planes)

Léelo en `US-CKP-022-grafo-carriles.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 5e2c008a11a4>>>
```text
docs/dev-briefs/us-ckp-022-graph-lanes.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 5e2c008a11a4>>>

contractSha: `3740b7b4aec810e1ccba21dba27469c09aeb6ed9f8dc208d8846f811815efb88`

## Tests de criterio

testsSha: `a4faf33fa320055df951ec6965045c77c692a47ec78875dd059a0c71190a21f3`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES c4d3a658acdb>>>
```text
d5afa0f80d4f3628d734bde2f6dbeae26cce1bd22539db836ca275f35157492c  apps/cli/src/client/tests/graph.rs
2b23706cd65e55eb0177eca119541683e888ace15437815072464d2ac2d1d37d  apps/cli/src/present/graph_tests.rs
d791e389572b529a72deb9bf3dac772b49c41c0ebc486502032bcdef43303dc8  apps/cli/src/present/graph_text_tests.rs
342333f1af62c896315eb5e4257b268ab4068705ede0f42af16b46e1b8c0389a  apps/cli/src/tui/graph_tests.rs
67e874061732c3291a58d4f6cd1870852ccf1f539fdf86067e80ef6b8fa06e44  apps/cli/src/tui/view/graph_tests.rs
a6d740165f9e1cf164c578307ed8dea559da1e36ff428e2fe24eb052da13a2e0  apps/cli/tests/tui_graph.rs
dbc91a8247bd5a6c984715002e3df8e43cd5867856ce2782c50f9a94f024ca10  crates/api/tests/us_ckp_022_wire.rs
e23d8ad6ac07afa0806145a668fafeef01271951389b517e947efc63921e49f7  crates/core/src/channel/graph_tests.rs
41fb845707fcee632fc999a2a70078974efc5f9ef616e1f07420192e233adf3c  crates/core/tests/graph_query.rs
0a8542d1e2238dae527b39b1bca81fb8b71182853c88951defbde35ec0efdedc  crates/git/tests/us_ckp_022_lane.rs
```
<<<FIN-DATOS-NO-CONFIABLES c4d3a658acdb>>>

## Crítico

- Rondas (plan): 2
  - ronda 1: AJUSTES · 2 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: AJUSTES · 1 bloqueante(s) · siguiente: excepcion
- Último veredicto: AJUSTES
- Excepción: sí — se aprueba con los hallazgos abiertos como diferidos, o se para el paquete

## Diferidos

### Diferidos propuestos

1 hallazgo(s) bloqueante(s) que el crítico dejó abiertos, copiados tal cual:

<<<DATOS-NO-CONFIABLES bd87839542ec>>>
```text
id: PC-8
kind: test
toHuman: false
evidence: docs/dev-briefs/us-ckp-022-graph-lanes.contract.json:406
ask: E2E-5 dice con el panel de conflictos presente pero la prueba no lo cumple: quitar esa condición de la statement y de la DS § 9.4, la cesión queda en CKP022-CLI-YIELD.
```
<<<FIN-DATOS-NO-CONFIABLES bd87839542ec>>>

## Firmas

- planSha (contexto, no firmado): `c2caf77c300f1d4a2ddae4fad78d6aa175be7dcfe5ed8a201192add8f89c952e`
- contractSha: `3740b7b4aec810e1ccba21dba27469c09aeb6ed9f8dc208d8846f811815efb88`
- testsSha: `a4faf33fa320055df951ec6965045c77c692a47ec78875dd059a0c71190a21f3`
- approvalSha: `c2caf77c300f1d4a2ddae4fad78d6aa175be7dcfe5ed8a201192add8f89c952e:3740b7b4aec810e1ccba21dba27469c09aeb6ed9f8dc208d8846f811815efb88:a4faf33fa320055df951ec6965045c77c692a47ec78875dd059a0c71190a21f3`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
