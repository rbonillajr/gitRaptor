# Brief · US-MCP-016 `check_conflicts` (preparación de `/implement`)

El plan es DS-US-MCP-016 (`docs/requirements/features/mcp/dev-specs/US-MCP-016-dev-spec.md`, spec-ready). Este Brief **no lo reescribe**: cita sus `§` y solo añade la deriva contra el código de hoy (`2d18a3c2`), las convenciones que la DS no cubre y el reparto en tramos. Manda la DS; donde el código la contradice, manda el código y se dice aquí.

## Deriva

Comprobado contra `feat/US-MCP-016-check-conflicts` @ `2d18a3c2`. Ninguna cita rota cambia el diseño: **sin BLOQUEO**.

**§ Contexto rápido**
- `methods/mcp.rs:29` `MCP_STATUS`, `:50` `valid_cursor`, `:54` `GROUP` · se mantiene.
- `conn.rs::mcp_status` `:1765` · movida a `conn.rs:1980`.
- Cubo de lecturas `conn.rs:737-747` · movida: se crea en `conn.rs:231` (`mcp_read_bucket`) y se gasta en `conn.rs:858` (`self.mcp_read_bucket.take()` para todo método con `spec.mcp`). D10 se mantiene: un método con `mcp = true` lo gasta sin código nuevo.
- `MCP_READS_PER_MINUTE`/`MCP_READ_BURST` `mcp_view.rs:40-41` · se mantiene.
- `mcp_scope::locate` `mcp_scope.rs:12`; `locate_with_last` `mcp_status.rs:167` · se mantiene.
- `McpCursors` `mcp_status.rs:481`, `mint` `:491`, `CursorEntry` `:457` · movidas a `:478`, `:488`, `:454`. `CursorEntry::repo_id` en `:468`.
- `wire_len` `mcp_view.rs:219`, `fit_page` `:228` · movidas a `:328`, `:337`. `fit_page(status, budget)` recibe un solo presupuesto, no `(target, hard)`; la firma de `fit_conflicts(c, target, hard)` de la DS es nueva y se mantiene.
- `for_mcp` `mcp_view.rs:476` · movida a `:585` (`pub fn for_mcp(value: &mut Value)`); la vista de status es `impl From<&McpStatus> for McpStatusView` (`:491`), no un `for_mcp` de tipo. `McpConflictsView::for_mcp` de la DS es nuevo: se mantiene.
- `McpToolError` `mcp_view.rs:65`, `ALL` `:112` · movida `ALL` a `:136`. **No se mantiene el número**: `ALL` tiene hoy **33** códigos, no 21; con `PredictionUnavailable` pasa a **34** (T002 decía «de 21 a 22»). No cambia el diseño.
- `MAX_MCP_PART_BYTES` `mcp_view.rs:26` · se mantiene.
- `snapshot.rs::tool()` `:44`; `status.rs::cursor_argument` `:9` · se mantiene (`:10`).
- `server.rs::list_tools` `:161`, `call_tool` `:172` · movidas a `:186`, `:194`. El catálogo ya no es solo `status` + `snapshot`: `tools()` (`server.rs:61`) publica `status` + `WRITE_TOOLS` (`snapshot`, `safe_commit`).

**§ Ajustes a `main`**
- `PairStateView`, `ConflictKindView`, `PairReason`, `PairLevel`, `LimitCode::ALL` (8), `PairSideView::Base {}`, `ConflictFileView.ours/theirs: Vec<LineRange>`, `has_merge_rule` en `crates/api/src/prediction.rs` · se mantiene.
- `predict_read(store, repo_id, worktree)` `crates/core/src/predict/store.rs:109` · se mantiene; filtra por `PairSideView::Worktree { root }` igual a `worktree` y devuelve vacío para un repo sin publicar.
- `PredictionStore::published` · no existe todavía (aditiva, se crea; `store.lock().get(repo_id).is_some()`). `PredictionStore::active()` existe (`store.rs:48`). El almacén llega a la conexión por `ChannelCtx.prediction` (`channel/server.rs:111`), ya usado en `conn.rs:743`.

**T001–T006**
- T002: `methods/mcp.rs` `GROUP` admite la línea `method(MCP_CONFLICTS, false, true).since(CAPABILITIES_PROTOCOL)` · se mantiene. `mcp_view.rs` no tiene hoy submódulos: `mod conflicts;` en `mcp_view.rs` + `crates/api/src/mcp_view/conflicts.rs` es válido (módulo no `mod.rs`).
- T004: `channel/mod.rs` declara `mod mcp_status;` bajo `#[cfg(any(unix, windows))]` (`mod.rs:21-22`) · la línea nueva `mod mcp_conflicts;` lleva el mismo `cfg`. Brazo `MCP_STATUS` `conn.rs:890` · movido a `conn.rs:1023`. `owned_by_me` (`mcp_status.rs:49` unix, `:61` no-unix) es privada · pasa a `pub(super)` en ambas ramas, como dice la DS. Helpers reutilizables ya en `conn.rs`: `remember_scope` (`:2113`), `mint_cursor` (`:2124`), `cursor_entry` (`:2135`, filtra por `repo_id` pero **no** por variante: `mcp_conflicts` comprueba la variante después, paso 3).
- T006: `Engine::status` `engine.rs:54` · movida a `engine.rs:80`; `refusal` `engine.rs:277` · movida a `engine.rs:523`. `status_value` `server.rs:100` · movida a `:125`; `respond` `:110` · movida a `:135`. `messages.rs::refusal` es exhaustivo sobre `McpToolError`.
- T005: `apps/mcp/tests/handshake.rs:104-113` espera hoy `["status","snapshot","safe_commit"]` (la DS decía `["status","snapshot"]`). `apps/mcp/tests/token_budget.rs` **no** lista nombres: recorre `tools/list` y cubre la herramienta nueva sin cambios.

**§ Estrategia de pruebas → tests existentes que se ajustan** (la DS nombra cuatro; el código pide cinco más)
- `mcp_view.rs::mcp_tool_codes_are_kebab_and_closed` (`:858`): su `match` exhaustivo gana `PredictionUnavailable` · se mantiene.
- `server.rs::the_catalog_fits_its_token_budget` (`:422`) · se mantiene (añadir `conflicts::tool()` a la lista).
- `apps/mcp/tests/handshake.rs` · se mantiene con la lista corregida (ver Gaps S1).
- `messages.rs::every_code_has_its_texts_in_both_languages` · se mantiene (sin cambios).
- **No está en la DS:** `methods/mcp.rs::only_status_is_offered_over_mcp` (`:488-494`, `assert_eq!(mcp, [MCP_STATUS])`) → `[MCP_STATUS, MCP_CONFLICTS]`.
- **No está en la DS:** `crates/api/tests/legacy_protocols.rs` `AFTER_FREEZE_FULL` y `AFTER_FREEZE_MCP` (`:93-113`) ganan `"mcp.conflicts"`.
- **No está en la DS:** `server.rs::every_tool_is_read_only_or_a_catalog_operation` (`:263`): cuenta `1 + WRITE_TOOLS.len()` y solo admite `STATUS_TOOL` como lectura → admite `CONFLICTS_TOOL` como segunda herramienta de lectura.

**Decisión sobre los ajustes:** son **trabajo de build, no criterios**. Cinco de los siete viven en módulos `#[cfg(test)] mod tests` de ficheros de producción (`mcp_view.rs`, `mcp.rs`, `server.rs`, `messages.rs`), y declararlos como `tests` de un criterio haría que R4 bloqueara la edición de producción. `handshake.rs` y `legacy_protocols.rs` están en verde hoy y se ponen rojos con el build: se ajustan en el mismo tramo que los rompe. Los mide la suite (P3), no un `verify`.

## Convenciones observadas

Solo lo que la DS no cubre.
- **Tests de integración del daemon con cliente `mcp` en otro proceso:** el arnés más cercano no es `channel_scopes.rs` sino `crates/core/tests/mcp_status_full.rs::mcp_client_entry` (cliente re-ejecutado con `current_exe` y `RAPTOR_TEST_MCP_CLIENT={"root","calls","out"}`, pasos `#wait`/`#signal`/`#cd`, referencias `"$N:/pointer"`) con `wait_for_file` (`:152`) en lugar de `sleep`. `channel_scopes.rs` (`:61` `add_repo`, `:65` `DaemonEnv::from_vars`, `:83` `Daemon::start`) y `channel_protected.rs` (`:189` `current_exe`) son el patrón de arranque que cita la DS.
- **Pares reales del predictor en un daemon:** `crates/core/tests/predict_publish.rs` (`:79` `DaemonConfig` con el predictor activo, `:184` `conflicting()` fija un conflicto real, `:369` `settled` y `:420` espera de `PREDICTION_CHANGED` en una conexión completa). `crates/core/tests/mcp_conflicts.rs` combina este arnés con el de `mcp_status_full.rs`; copia lo que necesite (cada test de integración es su propio crate), sin crear un módulo `common` nuevo.
- **Tests unitarios en archivo propio:** `#[cfg(test)] #[path = "<mod>_tests.rs"] mod tests;` al final del módulo (`channel/mcp_status.rs:513-519`, `apps/mcp/src/engine.rs:606-611`). Nombre resultante: `channel::mcp_conflicts::tests::<fn>` en `gitraptor-core --lib` y `conflicts::tests::<fn>` en `gitraptor-mcp --bin raptor-mcp`.
- **`apps/mcp` es solo binario** (`apps/mcp/Cargo.toml:10-12`, `raptor-mcp`): sus unitarios se filtran con `--bin raptor-mcp`; sus integraciones usan `env!("CARGO_BIN_EXE_raptor-mcp")` (`tests/token_budget.rs:17`).
- **Catálogo de `apps/mcp`:** `server.rs::tools()` (`:61`) arma la lista; `WRITE_TOOLS` (`:55`) solo admite operaciones del catálogo de escritura. `check_conflicts` es lectura: no entra en `WRITE_TOOLS`. Presupuesto del catálogo entero: `mcp_view::catalog_overruns` (`:403`), medido en `server.rs::the_catalog_fits_its_token_budget` y por cable en `tests/token_budget.rs::the_catalog_fits_its_token_budget`.
- **Corpus de seguridad MCP (INF-MCP-001):** `apps/cli/tests/mcp_security_corpus.rs::the_corpus_is_fully_rejected`, casos JSON en `apps/cli/tests/mcp_corpus/cases/`, motor en `crates/testkit/src/mcp_corpus/`. Corre contra los binarios reales: necesita `cargo build -p gitraptor-cli -p gitraptor-mcp --bins` antes (`docs/dev-briefs/inf-mcp-001-mcp-security-corpus.contract.json`, C2). Esta historia no añade casos.
- **Registro de métodos:** un método nuevo se declara en el fichero de su grupo, nunca en `methods/mod.rs` (`crates/api/tests/architecture.rs::the_method_registry_declares_no_method`); todo método del grupo `mcp` lleva `.since(CAPABILITIES_PROTOCOL)` (`architecture.rs::no_method_waits_for_a_newer_protocol`, `legacy_protocols.rs::protocol_9_adds_only_the_capability_handshake`).
- **Stubs para el rojo:** los ficheros de producción nuevos llevan los tipos completos y cuerpos `unimplemented!()`; ningún stub queda alcanzable desde una ruta de producción (sin brazo en `conn.rs`, sin entrada en `tools()`) hasta su tramo de build. Stubs + tests en rojo se commitean antes de `baseline --write`.

### Inventario de reutilización

- **Reutiliza:** `predict_read` y `PredictionStore::active` (`crates/core/src/predict/store.rs`); `mcp_status::{locate_with_last, admit, facts, worktree_name, Located, Refusal, CursorEntry, McpCursors}`; `Connection::{remember_scope, mint_cursor, cursor_entry}` (`conn.rs:2113-2155`); `scope_refused`/`mcp_unavailable` de `conn.rs`; `methods::valid_cursor`; `mcp_view::{for_mcp, compact_schema, check_token_budget, catalog_overruns, MCP_*}` y `opaque_object` (`mcp_view.rs:462`, privado: `mcp_view/conflicts.rs` es hijo y lo ve con `super::opaque_object`); `status::cursor_argument` para validar el cursor en `arguments()`; `server::{within, respond, malformed}`; los tipos de `gitraptor_api::prediction`.
- **Genera:** ninguno aplica.
- **Crea nuevo:** lo que la DS § Contratos declara CREATE, más `PredictionStore::published`. Ningún crate nuevo.

## Estructura de ficheros

Tres tramos disjuntos. S1 va primero (los otros dos importan su contrato); S2 y S3 pueden ir en paralelo después.

### Slice 1 — contrato en `crates/api` (tracer bullet)
criterios: MCP016-FIELD-ALLOWLIST, MCP016-PAGE-FIT, MCP016-SANITIZE-VIEW
- `crates/api/src/methods/mcp.rs` (MODIFY: `MCP_CONFLICTS`, línea en `GROUP`, `#[path] mod conflicts; pub use conflicts::*;`, ajuste de `only_status_is_offered_over_mcp`)
- `crates/api/src/methods/mcp_conflicts.rs` (CREATE)
- `crates/api/src/mcp_view.rs` (MODIFY: `mod conflicts; pub use conflicts::*;`, `PredictionUnavailable` en `McpToolError`, `ALL` 33 → 34, `as_str`, ajuste de `mcp_tool_codes_are_kebab_and_closed`)
- `crates/api/src/mcp_view/conflicts.rs` (CREATE)
- `crates/api/tests/legacy_protocols.rs` (MODIFY: `"mcp.conflicts"` en `AFTER_FREEZE_FULL` y `AFTER_FREEZE_MCP`)
- `crates/api/tests/mcp_conflicts_contract.rs` (CREATE, tester)

### Slice 2 — daemon en `crates/core`
criterios: MCP016-CONFLICT-SEEN, MCP016-NO-CONTENT, MCP016-DEFAULT-SCOPE, MCP016-REPO-SCOPE, MCP016-CALCULATING, MCP016-PENDING, MCP016-LIMITS, MCP016-LEVEL, MCP016-P6-UNKNOWN, MCP016-P6-ALLOWLIST, MCP016-MAIN-WORKTREE, MCP016-DELETED, MCP016-ORDER, MCP016-CURSOR-DAEMON, MCP016-CURSOR-REVERSE, MCP016-AGE, MCP016-SIDE-A, MCP016-RATE, MCP016-UNPUBLISHED
- `crates/core/src/predict/store.rs` (MODIFY, solo `PredictionStore::published`)
- `crates/core/src/channel/mcp_conflicts.rs` (CREATE)
- `crates/core/src/channel/mcp_conflicts_tests.rs` (CREATE, tester)
- `crates/core/src/channel/mod.rs` (MODIFY, una línea `mod`)
- `crates/core/src/channel/mcp_status.rs` (MODIFY: `CursorEntry::Conflicts`, `repo_id()`, `owned_by_me` `pub(super)`)
- `crates/core/src/channel/conn.rs` (MODIFY: brazo `MCP_CONFLICTS` junto a `MCP_STATUS`, `fn mcp_conflicts` junto a `fn mcp_status`; en `fn mcp_status`, rechazo con `NOT_FOUND` de un cursor de la variante `Conflicts` (S4, MCP016-CURSOR-REVERSE))
- `crates/core/tests/mcp_conflicts.rs` (CREATE, tester)

### Slice 3 — herramienta en `apps/mcp`
criterios: MCP016-CURSOR-TOOL, MCP016-PREDICTION-UNAVAILABLE, MCP016-TOOL-BUDGET, MCP016-HOSTILE-PAGE, MCP016-MARKED, MCP016-REFUSALS
- `apps/mcp/src/conflicts.rs` (CREATE)
- `apps/mcp/src/conflicts_tests.rs` (CREATE, tester)
- `apps/mcp/src/engine.rs` (MODIFY: `Engine::conflicts`, `METHOD_NOT_FOUND` → `PredictionUnavailable`)
- `apps/mcp/src/server.rs` (MODIFY: `tools()`, brazo `CONFLICTS_TOOL`, ajuste de `every_tool_is_read_only_or_a_catalog_operation` y `the_catalog_fits_its_token_budget`)
- `apps/mcp/src/messages.rs` (MODIFY: textos en/es de `prediction-unavailable`)
- `apps/mcp/src/main.rs` (MODIFY: `mod conflicts;`)
- `apps/mcp/tests/handshake.rs` (MODIFY: lista de herramientas)

El corpus de seguridad del MCP (INF-MCP-001) no es criterio: ya pasa (B3) y es un archivo existente; lo mide la suite completa, que debe quedar en verde.

## Gaps y supuestos

- **S0 · criterios por tarea.** Las tareas T001–T006 de la DS no llevan línea `Criterios:`; el reparto de criterios lo llevan los tramos de § Estructura de ficheros (línea «criterios:» de cada tramo), que sustituyen a esa línea por tarea. T001/T003/T005 son los tests rojos de los tramos api/core/mcp y T002/T004/T006 sus construcciones.

- **S1 · orden del catálogo.** La DS escribió `["status","snapshot","check_conflicts"]` antes de que existiera `safe_commit`. Supuesto: la herramienta nueva va **al final**, `["status","snapshot","safe_commit","check_conflicts"]`, sin mover las posiciones que ya publica `tools()`. Alternativa: lecturas primero (`status`, `check_conflicts`, después las de escritura); no cambia ningún criterio.
- **S2 · fichero de cada prueba de presupuesto.** La fila «Presupuesto · páginas» dice «api · mcp» sin repartir. Supuesto: `a_typical_page_fits_its_token_target`, `fit_conflicts_keeps_conflicts_first_and_marks_the_cut` y `a_single_pair_over_the_bound_is_cut_in_its_files` en **api** (miden `fit_conflicts`/`conflicts_wire_len`); `a_hostile_page_fits_each_part` en **mcp** (cada parte pasa por `respond`). Igual en «Saneado»: `hostile_paths_and_names_are_escaped_and_cut` en **api**, `every_repo_text_in_conflicts_is_marked` en **mcp**.
- **S3 · presupuesto del catálogo con cuatro herramientas.** El catálogo entero tiene tope 1.500 tokens (RES-MCP-01) y hoy publica tres herramientas. No está medido si cabe una cuarta con su `outputSchema`. Decisión del orquestador (2026-11-04): si no cabe, el expert recorta primero la descripción y el `outputSchema` opaco de `check_conflicts` (sin perder la marca `untrusted` ni el «Never file content»); no toca las descripciones de las herramientas publicadas ni sube el tope del ADR. Si aun así no cabe, es un `BLOQUEO` que decide el coordinador (subir el tope es una enmienda de ADR-MCP-001).
- **S4 · `cursor_entry` no distingue variantes.** Un cursor de `status` de la misma conexión y repo pasa `cursor_entry`; `mcp_conflicts` devuelve `NOT_FOUND` si la variante no es `Conflicts` (D4). Decisión del orquestador (2026-11-04): también al revés, `mcp_status` rechaza un `CursorEntry::Conflicts` con `NOT_FOUND` (`invalid-cursor`) en lugar de servir la página por defecto. Es un cambio de comportamiento de una herramienta publicada, así que lleva criterio propio: MCP016-CURSOR-REVERSE (`a_conflicts_cursor_is_not_a_status_cursor`, `crates/core/tests/mcp_conflicts.rs`).
- **S5 · corpus de seguridad del MCP.** Fuera de `criteria[]` (un criterio en verde antes del build para en B3); la suite completa lo mide y debe quedar verde.
- **S6 · plataformas.** `crates/core/tests/mcp_conflicts.rs` solo corre en macOS y Linux (DS T003); en Windows el canal no existe (G3, XP-01).
