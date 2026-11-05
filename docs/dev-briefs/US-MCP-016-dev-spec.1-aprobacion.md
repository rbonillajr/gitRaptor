---
schema: nassa.approval-request.v1
plan: "docs/requirements/features/mcp/dev-specs/US-MCP-016-dev-spec.md"
---

Lee este fichero y luego `US-MCP-016-dev-spec.md`; `US-MCP-016-dev-spec.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: no
- Generada: `2026-11-05T00:37:57.020Z`
- Sha de aprobación: `d697f14f4507bb96871fbcf9f1826d82ebd3a6a384221dacec0498f32f914d7e:b30ce12149f53625bd59b9cbbf0057f4c3e73bdbce1da9673600402dcab98662:c8e1efb00d3f299e7f179bede5dc8009ad4b9fce12fcc2b722b08c0a91d1ecf4`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES 5f518b2b1cfa>>>
```text
intent: Que el agente de IA pida check_conflicts y reciba la predicción de conflictos que el motor ya publicó para su worktree, sin contenido de archivos, para coordinarse o cambiar de enfoque antes de que un merge o un rebase choque (US-MCP-016).
endState: raptor-mcp publica check_conflicts {scope?, cursor?}; el daemon sirve mcp.conflicts al perfil mcp (desde CAPABILITIES_PROTOCOL) leyendo solo el estado publicado del predictor: por par nivel, archivos y rangos [start,len] por lado, age_s, estado y motivo si no hay resultado y los ocho límites; ⚡ primero, páginas dentro de 800 tokens con cursor, textos del repo como untrusted y rechazos con código estable.
nonGoals:
  - Cambiar la forma de la predicción, prediction.list, prediction.changed o attention.conflicts (DS-TS-CKP-001)
  - Exponer prediction.list o el stream al perfil mcp
  - Calcular algo por petición o forzar un recálculo desde el MCP
  - Texto de los LimitCode para el modelo, cambios de la TUI o de raptor conflicts
```
<<<FIN-DATOS-NO-CONFIABLES 5f518b2b1cfa>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES cfee3a38390b>>>
```text
[1] id: MCP016-FIELD-ALLOWLIST
statement: La vista de un par ⚡ de check_conflicts solo lleva los campos de la allowlist del contrato (a, b, state, reason, level, age_s, computed_at_utc_ms, overlap, overlap_partial, conflict con path, kind, a, b, hunks_truncated, merge_rule): nunca pair_id, rev, merge_base, rewrites_escalated, repo_id ni una ruta absoluta
verify: cargo test -p gitraptor-api --test mcp_conflicts_contract a_conflict_pair_view_is_its_field_allowlist -- --exact
tests:
  - crates/api/tests/mcp_conflicts_contract.rs

[2] id: MCP016-PAGE-FIT
statement: Una página típica (3 ⚡ con 2 archivos y 2 rangos, 2 ⚠ con 3 rutas y los límites) cabe en MCP_PAGE_TOKENS; fit_conflicts conserva los ⚡ primero y marca el corte con truncated y el cursor de relleno; un único par que solo pasa de MCP_FIT_BYTES se recorta en sus archivos y se marca truncated/overlap_partial
verify: cargo test -p gitraptor-api --test mcp_conflicts_contract a_typical_page_fits_its_token_target -- --exact && cargo test -p gitraptor-api --test mcp_conflicts_contract fit_conflicts_keeps_conflicts_first_and_marks_the_cut -- --exact && cargo test -p gitraptor-api --test mcp_conflicts_contract a_single_pair_over_the_bound_is_cut_in_its_files -- --exact
tests:
  - crates/api/tests/mcp_conflicts_contract.rs

[3] id: MCP016-SANITIZE-VIEW
statement: Rutas y nombres hostiles (\x1b]52;…, U+202E, U+061C, U+2028, U+2063, U+E0041, nombre de 300 caracteres y ruta de 4 KiB) salen escapados por for_mcp y cortados a 100 caracteres en nombres y 1.024 bytes en rutas
verify: cargo test -p gitraptor-api --test mcp_conflicts_contract hostile_paths_and_names_are_escaped_and_cut -- --exact
tests:
  - crates/api/tests/mcp_conflicts_contract.rs

[4] id: MCP016-CONFLICT-SEEN
statement: Esc. 1: el agente, con cwd en su worktree, recibe el ⚡ que el motor publicó con el otro worktree, con nivel conflict, la ruta, los rangos [start,len] de cada lado y age_s calculada por el daemon
verify: cargo test -p gitraptor-core --test mcp_conflicts the_agent_sees_its_predicted_conflict_with_ranges_and_age -- --exact
tests:
  - crates/core/tests/mcp_conflicts.rs

[5] id: MCP016-NO-CONTENT
statement: Esc. 1: ninguna respuesta de mcp.conflicts contiene contenido ni diff: el marcador único de las líneas en conflicto, @@, <<<<<<< ni líneas +/- de diff
verify: cargo test -p gitraptor-core --test mcp_conflicts no_file_content_nor_diff_in_any_answer -- --exact
tests:
  - crates/core/tests/mcp_conflicts.rs

[6] id: MCP016-DEFAULT-SCOPE
statement: Esc. 2: sin scope, solo salen los pares en los que el worktree del llamante es un lado worktree
verify: cargo test -p gitraptor-core --test mcp_conflicts by_default_only_the_callers_pairs -- --exact
tests:
  - crates/core/tests/mcp_conflicts.rs

[7] id: MCP016-REPO-SCOPE
statement: Esc. 2: con scope repo salen todos los pares del repo del llamante
verify: cargo test -p gitraptor-core --test mcp_conflicts the_repo_scope_gives_every_pair -- --exact
tests:
  - crates/core/tests/mcp_conflicts.rs

[8] id: MCP016-CALCULATING
statement: Un par sin resultado sale con state calculating, sin level, y counts lo cuenta; no se inventa ningún par
verify: cargo test -p gitraptor-core --lib channel::mcp_conflicts::tests::declares_a_calculating_prediction_without_inventing_pairs -- --exact
tests:
  - crates/core/src/channel/mcp_conflicts_tests.rs

[9] id: MCP016-PENDING
statement: Un par pendiente sale con state pending y su reason (p. ej. base-unconfirmed), sin level, y counts lo cuenta; no se inventa ningún par
verify: cargo test -p gitraptor-core --test mcp_conflicts declares_a_pending_prediction_with_its_reason_without_inventing_pairs -- --exact && cargo test -p gitraptor-core --lib channel::mcp_conflicts::tests::declares_a_pending_prediction_with_its_reason_without_inventing_pairs -- --exact
tests:
  - crates/core/tests/mcp_conflicts.rs
  - crates/core/src/channel/mcp_conflicts_tests.rs

[10] id: MCP016-LIMITS
statement: Esc. 4: cada respuesta declara los ocho LimitCode tal como los publica el motor
verify: cargo test -p gitraptor-core --test mcp_conflicts the_limits_are_declared_as_the_engine_published_them -- --exact
tests:
  - crates/core/tests/mcp_conflicts.rs

[11] id: MCP016-LEVEL
statement: level solo aparece en current y recalculating (con el del último resultado) y none va explícito; un par current omite state
verify: cargo test -p gitraptor-core --lib channel::mcp_conflicts::tests::level_is_absent_without_a_result_and_none_is_explicit -- --exact
tests:
  - crates/core/src/channel/mcp_conflicts_tests.rs

[12] id: MCP016-P6-UNKNOWN
statement: P6/D11: con cwd bajo el directorio común de un repo de la allowlist pero bajo ninguna raíz de worktree (repo bare con un enlazado), la respuesta da los pares de todo el repo con scope repo, reason caller-worktree-unknown y sin worktree
verify: cargo test -p gitraptor-core --test mcp_conflicts an_unidentified_worktree_gives_the_repo_scope_with_its_reason -- --exact
tests:
  - crates/core/tests/mcp_conflicts.rs

[13] id: MCP016-P6-ALLOWLIST
statement: P6: un repo fuera de la allowlist responde SCOPE_REFUSED not-allowlisted sin ningún dato del repo
verify: cargo test -p gitraptor-core --test mcp_conflicts outside_the_allowlist_is_the_repo_not_enabled_refusal -- --exact
tests:
  - crates/core/tests/mcp_conflicts.rs

[14] id: MCP016-MAIN-WORKTREE
statement: En el worktree principal salen los pares que lo incluyen como lado worktree; una lista vacía con pares calculating nunca se presenta como sin conflictos
verify: cargo test -p gitraptor-core --lib channel::mcp_conflicts::tests::the_main_worktree_gets_the_pairs_that_include_it -- --exact && cargo test -p gitraptor-core --lib channel::mcp_conflicts::tests::no_pairs_while_calculating_is_not_no_conflicts -- --exact
tests:
  - crates/core/src/channel/mcp_conflicts_tests.rs

[15] id: MCP016-DELETED
statement: D11: un worktree borrado durante la sesión (Located::Missing) responde worktree-missing
verify: cargo test -p gitraptor-core --lib channel::mcp_conflicts::tests::a_deleted_worktree_is_worktree_missing -- --exact
tests:
  - crates/core/src/channel/mcp_conflicts_tests.rs

[16] id: MCP016-ORDER
statement: D5: la página ordena ⚡ current, ⚡ recalculating, ⚠ current, ⚠ recalculating, calculating, pending, not-computable, none; recorrer con el cursor no duplica ni pierde pares
verify: cargo test -p gitraptor-core --lib channel::mcp_conflicts::tests::conflicts_come_first_then_overlaps -- --exact && cargo test -p gitraptor-core --test mcp_conflicts a_conflicts_cursor_pages_without_duplicates -- --exact
tests:
  - crates/core/src/channel/mcp_conflicts_tests.rs
  - crates/core/tests/mcp_conflicts.rs

[17] id: MCP016-CURSOR-DAEMON
statement: D4: un cursor de status no es un cursor de conflicts, y un cursor de conflicts de otra conexión no existe (NOT_FOUND)
verify: cargo test -p gitraptor-core --lib channel::mcp_conflicts::tests::a_status_cursor_is_not_a_conflicts_cursor -- --exact && cargo test -p gitraptor-core --test mcp_conflicts a_conflicts_cursor_from_another_connection_does_not_exist -- --exact
tests:
  - crates/core/src/channel/mcp_conflicts_tests.rs
  - crates/core/tests/mcp_conflicts.rs

[18] id: MCP016-CURSOR-REVERSE
statement: Un cursor de check_conflicts no es un cursor de status: mcp.status lo rechaza con NOT_FOUND (invalid-cursor) en la misma conexión y el mismo repo, en lugar de servir la página por defecto (D4 en sentido inverso)
verify: cargo test -p gitraptor-core --test mcp_conflicts a_conflicts_cursor_is_not_a_status_cursor -- --exact
tests:
  - crates/core/tests/mcp_conflicts.rs

[19] id: MCP016-CURSOR-TOOL
statement: NFR-02: check_conflicts rechaza con invalid-params {field} y sin llamar al motor cualquier argumento que no sea scope worktree|repo o un cursor de 16 hex
verify: cargo test -p gitraptor-mcp --bin raptor-mcp conflicts::tests::malformed_arguments_are_refused_before_the_engine -- --exact
tests:
  - apps/mcp/src/conflicts_tests.rs

[20] id: MCP016-AGE
statement: D7: age_s la calcula el daemon como now_ms - computed_at_utc_ms y nunca es negativa (0)
verify: cargo test -p gitraptor-core --lib channel::mcp_conflicts::tests::age_is_computed_by_the_daemon_and_never_negative -- --exact
tests:
  - crates/core/src/channel/mcp_conflicts_tests.rs

[21] id: MCP016-SIDE-A
statement: D14: en ámbito worktree el llamante va en el lado a; si el motor lo tiene en b, la vista intercambia lados y rangos
verify: cargo test -p gitraptor-core --lib channel::mcp_conflicts::tests::the_caller_is_side_a_with_its_ranges -- --exact
tests:
  - crates/core/src/channel/mcp_conflicts_tests.rs

[22] id: MCP016-RATE
statement: D10: cada mcp.conflicts gasta el cubo de lecturas del perfil mcp y, agotado, responde RATE_LIMITED
verify: cargo test -p gitraptor-core --test mcp_conflicts conflicts_spend_the_read_bucket -- --exact
tests:
  - crates/core/tests/mcp_conflicts.rs

[23] id: MCP016-UNPUBLISHED
statement: D13: un predictor sin estado para el repo da pairs vacío con unpublished true, sin rechazo
verify: cargo test -p gitraptor-core --lib channel::mcp_conflicts::tests::an_unpublished_prediction_is_declared -- --exact && cargo test -p gitraptor-core --test mcp_conflicts an_unpublished_prediction_is_declared_on_the_wire -- --exact
tests:
  - crates/core/src/channel/mcp_conflicts_tests.rs
  - crates/core/tests/mcp_conflicts.rs

[24] id: MCP016-PREDICTION-UNAVAILABLE
statement: D13: un daemon que no ofrece mcp.conflicts (METHOD_NOT_FOUND) da el código de herramienta prediction-unavailable
verify: cargo test -p gitraptor-mcp --bin raptor-mcp conflicts::tests::a_daemon_without_mcp_conflicts_is_prediction_unavailable -- --exact
tests:
  - apps/mcp/src/conflicts_tests.rs

[25] id: MCP016-TOOL-BUDGET
statement: RES-MCP-01: la descripción más el inputSchema de check_conflicts caben en 150 tokens, el outputSchema en 400 y el catálogo en 1.500; una descripción duplicada falla
verify: cargo test -p gitraptor-mcp --bin raptor-mcp conflicts::tests::the_check_conflicts_tool_fits_its_token_budget -- --exact
tests:
  - apps/mcp/src/conflicts_tests.rs

[26] id: MCP016-HOSTILE-PAGE
statement: Una página hostil (50 pares, nombres de 1.024 caracteres que crecen al escaparse, 200 rutas de 1.024 B por par) cabe en cada parte de MAX_MCP_PART_BYTES al pasar por la tubería de la herramienta
verify: cargo test -p gitraptor-mcp --bin raptor-mcp conflicts::tests::a_hostile_page_fits_each_part -- --exact
tests:
  - apps/mcp/src/conflicts_tests.rs

[27] id: MCP016-MARKED
statement: SEC-12/MCP06: todo string de la respuesta de check_conflicts está bajo untrusted, es un literal del contrato o es el cursor hex
verify: cargo test -p gitraptor-mcp --bin raptor-mcp conflicts::tests::every_repo_text_in_conflicts_is_marked -- --exact
tests:
  - apps/mcp/src/conflicts_tests.rs

[28] id: MCP016-REFUSALS
statement: RES-MCP-03: cada rechazo de check_conflicts cabe en 80 tokens en en y es y no lleva datos del repo
verify: cargo test -p gitraptor-mcp --bin raptor-mcp conflicts::tests::every_conflicts_refusal_fits_and_carries_no_repo_data -- --exact
tests:
  - apps/mcp/src/conflicts_tests.rs
```
<<<FIN-DATOS-NO-CONFIABLES cfee3a38390b>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES ded780d84597>>>
```text
(el plan no declara gaps ni supuestos)
```
<<<FIN-DATOS-NO-CONFIABLES ded780d84597>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES 940e77c8fd58>>>
```text
docs/requirements/features/mcp/dev-specs/US-MCP-016-dev-spec.md
```
<<<FIN-DATOS-NO-CONFIABLES 940e77c8fd58>>>

planSha: `d697f14f4507bb96871fbcf9f1826d82ebd3a6a384221dacec0498f32f914d7e` (base de la cadena de re-planes)

Léelo en `US-MCP-016-dev-spec.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 7332d32bfae4>>>
```text
docs/dev-briefs/us-mcp-016-check-conflicts.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 7332d32bfae4>>>

contractSha: `b30ce12149f53625bd59b9cbbf0057f4c3e73bdbce1da9673600402dcab98662`

## Tests de criterio

testsSha: `c8e1efb00d3f299e7f179bede5dc8009ad4b9fce12fcc2b722b08c0a91d1ecf4`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES 8630aec91549>>>
```text
a7f4fb08c99d2b7e3f36388b0e9d77bdbc5ee4a72cbb1a188bdf9d409c7a410f  apps/mcp/src/conflicts_tests.rs
f9380d8b5b769508fa4c933b3944b7bff175467e1d8a0d8922617222fd9a7eba  crates/api/tests/mcp_conflicts_contract.rs
f7278251e2357e56e0d7391acb8347a7b9d6ec0decc7f3ea77d723907f9738fc  crates/core/src/channel/mcp_conflicts_tests.rs
2fc7501f8e5603ed4512d633953992aaa52646e97568da9340188f4c41e5aab6  crates/core/tests/mcp_conflicts.rs
```
<<<FIN-DATOS-NO-CONFIABLES 8630aec91549>>>

## Crítico

- Rondas (plan): 1
  - ronda 1: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
- Último veredicto: APROBABLE
- Excepción: no

## Diferidos

Ninguno: el crítico dio el plan por aprobable.

## Firmas

- planSha (contexto, no firmado): `d697f14f4507bb96871fbcf9f1826d82ebd3a6a384221dacec0498f32f914d7e`
- contractSha: `b30ce12149f53625bd59b9cbbf0057f4c3e73bdbce1da9673600402dcab98662`
- testsSha: `c8e1efb00d3f299e7f179bede5dc8009ad4b9fce12fcc2b722b08c0a91d1ecf4`
- approvalSha: `d697f14f4507bb96871fbcf9f1826d82ebd3a6a384221dacec0498f32f914d7e:b30ce12149f53625bd59b9cbbf0057f4c3e73bdbce1da9673600402dcab98662:c8e1efb00d3f299e7f179bede5dc8009ad4b9fce12fcc2b722b08c0a91d1ecf4`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
