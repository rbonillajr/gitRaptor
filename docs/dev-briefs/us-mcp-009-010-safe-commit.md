# Preparación de la corrida · US-MCP-009 y US-MCP-010, `safe_commit` (2026-10-27)

Sin diseño: el plan son las dos Dev Specs validadas (#271), las dos `spec-ready`:

- `docs/requirements/features/mcp/dev-specs/US-MCP-009-dev-spec.md` (T001–T011): la herramienta, el brazo `commit`, el canal, los límites por solicitante y el permiso en el plan.
- `docs/requirements/features/mcp/dev-specs/US-MCP-010-dev-spec.md` (T001–T008): el modelo de amenazas de rutas A1–A25, los archivos nuevos nombrados y el worktree compartido.

Las dos van en una sola corrida y un solo PR, en el orden que fijan las DS: 009 y después 010 (DS-010 modifica archivos que crea DS-009). Esta nota la escribió el loop principal y solo recoge la preparación: la deriva de las citas, las decisiones reversibles y la topología en tramos.

## Deriva de las citas

Comprobadas contra `b2bd6484` (base de la rama): `ARMS` (`ops/mod.rs:22`), `armed_without_guardrails`, `test_arms.rs:worktree_git_dir`, `catalog.rs:check_literal_relative` y las constantes de `catalog.rs:34-40`, `UserOp::{CommitStaged, CommitPaths}`, `FALLBACK_NO_EDITOR`, `rejecting_editor`, `PlanShape::Commit` en `plan.rs:232`, `authorship::evaluate`, `DEFAULT_PRIOR_DEADLINE` (`protected/mod.rs:49`), `committed.rs`, `tree_path.rs:{HFS_IGNORABLE, is_dotgit, fold}`, `RepoReader::{is_ignored, status}`, `Store::sessions_for_worktree`, `ProcSource`, `DaemonBackend::facts`, `decide_all`, `commit_facts`, `McpToolError::ALL: [Self; 21]` y `unicode-normalization` en `crates/git`. Todas siguen diciendo lo que dicen las DS.

Única deriva: el cubo de escrituras de `snapshot` está en `conn.rs:3322` (`snapshot_admission`, `conn.rs:3310`), no en `conn.rs:3207`. No cambia ninguna decisión.

## Supuestos (decisiones de la preparación, reversibles)

- **Criterios por archivo de pruebas y por guarda.** Cada archivo de pruebas de las DS es un criterio que corre el archivo entero; cada guarda ⛔ que vive en un archivo de pruebas nuevo es además su propio criterio con `--exact`. Los `verify` de pruebas con nombre comprueban antes con `--list` que la prueba existe (un filtro sin coincidencias sale 0).
- **Guarda ⛔3.1 fuera del contrato.** `production_wiring_has_no_governed_arm_without_guardrails` ya existe en `ops/mod.rs` y pasa hoy: como criterio, la línea base B3 pararía la corrida. El experto cambia su primera aserción (T003 paso 2.1) y corre en la suite.
- **Guardas ⛔8.1 y ⛔8.2 fuera del contrato como criterios propios.** `the_commit_plan_logs_the_commit_msg_stage_first` y `a_human_author_denial_carries_its_rule_once` pasan hoy (el plan evalúa solo `commit-msg`): son guardas de regresión de T008 y B3 pararía la corrida. Siguen en `us_mcp_009_permission.rs`, que es el criterio MCP009-PERM (en rojo por sus otras pruebas), y corren en la suite.
- **`an_executor_descendant_counts_for_the_plan_requester` y la mitad del cliente del hook de `the_developer_and_the_hook_client_take_no_agent_slot`** no tienen hoy camino público para provocarse desde una prueba (`ExecutorMarks` se construye dentro de `Server::serve`; el cliente del hook se clasifica por su ejecutable instalado). Las escribe el experto del tramo 5 en `crates/core/tests/us_mcp_009_limits_executor.rs` (fuera de los criterios) o como prueba unitaria del canal, y lo dice en el PR si no hay forma.
- **`progress_is_sent_only_with_a_progress_token` la escribe el experto del tramo 4** en `apps/mcp/src/safe_commit_progress_tests.rs` (fuera de los criterios): la firma de `call` de DS-009 no da costura para observar `notifications/progress`, y el experto la crea (una función pura que construye el aviso a partir del token y del tiempo transcurrido). Los nombres `safe_commit::tool()`, `safe_commit::view()` y `engine::commit_refusal` (`pub(crate)`) los fijan las pruebas, a imagen de `apps/mcp/src/snapshot.rs`.
- **La presencia real del perfil (sesión viva, proceso muerto, TTL) la prueba el experto del tramo 3** con una prueba unitaria de la presencia de `DaemonBackend` (fuera de los criterios): solo se construye dentro del daemon. Las pruebas de `us_mcp_010_shared.rs` inyectan la presencia con `with_presence`; la e2e `a_shared_worktree_needs_named_paths` cubre la real. `a_queue_wait_over_its_bound_is_queue_full_without_effects` espera de verdad `MCP_QUEUE_WAIT` (60 s): la DS no da costura para acortarlo.
- **Reintento dentro de la misma sesión de `raptor-mcp`.** El `Engine` tiene una conexión por sesión bajo `try_lock`; un segundo `safe_commit` mientras el primero corre no llega al daemon. Con un commit en curso, `raptor-mcp` responde él mismo `operation-in-progress` con `kind: "write"` y la acción de S-11 (el mismo código que da el daemon a otra conexión del agente). Es lo que pide el escenario 6 de US-MCP-009; S-11 del daemon cubre las demás conexiones.
- **Pruebas unitarias en su propio archivo.** Las de `apps/mcp`, `crates/policy` y `crates/git` van en archivos `*_tests.rs` que el código de producción incluye con `#[cfg(test)] #[path = …] mod …;`. Esas cuatro líneas las añade el loop principal en el andamiaje (abajo), no el experto, para que la línea base compruebe que los archivos compilan. Las de `user_ops` de DS-010 (`add_paths_never_forces`, `add_beyond_a_symbolic_link_is_refused_by_git`) viven en `crates/git/src/user_ops_add_tests.rs` (módulo `user_ops::add_tests`) y no en `user_ops::tests`, para que R4 no congele `user_ops.rs`. Las de `raptor-mcp` de DS-010 T003 viven en `apps/mcp/src/safe_commit_paths_tests.rs`.
- **Pruebas del canal de T004 en un archivo de integración.** La guarda ⛔4.1 `a_commit_spends_the_connections_write_bucket` y `prepare_of_commit_without_the_capability_is_not_implemented` van en `crates/core/tests/us_mcp_009_channel.rs` (por el socket real), no en `--lib channel`: así no hace falta un `#[path]` en `conn.rs`.
- **Las guardas ⛔8.1 y ⛔8.2 van en `crates/core/tests/us_mcp_009_permission.rs`**: miden la entrada del registro (`plan_gate.rs`) y `decide_all`, los dos de `crates/core`. `crates/policy/src/guard/plan_commit_tests.rs` mide la forma de `normalize`.
- **Andamiaje para compilar.** Las pruebas en rojo deben compilar, o la suite base (B1) sale en rojo para todo el workspace. Tras la aprobación y antes de `baseline --write`, el loop principal añade los símbolos nuevos que nombran las pruebas, con las firmas literales de § Contratos compartidos de las dos DS y cuerpos `todo!()` / variantes sin uso, y las cuatro líneas `#[cfg(test)] #[path = …] mod …;` de `safe_commit_tests.rs`, `safe_commit_paths_tests.rs`, `plan_commit_tests.rs` y `user_ops_add_tests.rs` (con los módulos `safe_commit` y `safe_commit_paths` vacíos en `apps/mcp`). No es implementación: ningún criterio pasa con ellos.
- **Corpus.** Los casos `safe-commit-*` de nivel `server` y los de forma léxica de rutas (`safe-commit-path-*` que no necesitan `setup.files`) los escribe el tester. Los casos de nivel `engine` de DS-010 (`safe-commit-path-link-out`, `-link-parent`, `-nested-repo`, `-dot-git-swap`, `-ignored`, `-shared-all-staged`, `-case`, `-skip-worktree`, `-nfd`, `-glob`, `-exclude`) los escribe el experto con T007 (cambian el esquema del corpus en `crates/testkit`), y los cubre el mismo criterio del corpus.
- **`apps/cli/tests/guard_ts_ckp_003.rs` entra en el tramo 3**: T003 paso 7 permite ajustar un caso suyo si dependía de una forma que el brazo de producción rechaza; el ajuste se dice en el PR.
- **`crates/api/src/commit_paths.rs` se declara con `pub mod commit_paths;` en `lib.rs`.** DS-010 crea el módulo y no dice dónde se declara; DS-009 T002 prohíbe tocar `lib.rs` solo para el contrato de 009 (`crates/api/tests/architecture.rs` vigila las constantes de protocolo, no las líneas `mod`). Una línea por módulo, como pide ADR-GRP-016.
- **`.g\u{200C}it/x` es `invalid-path`, no `path-outside-worktree`.** DS-010 T002 fija el orden «el primero que falla decide» y 2.2 (`is_forbidden_char`, que incluye U+200C) va antes de 2.4 (alias de `.git`); la fila de la tabla de § Contrato de API que lo pone como «fuera» contradice ese orden. Manda el orden de los pasos: los dos rechazan la ruta y ninguna llega a Git. Los ignorables de HFS+ que `is_forbidden_char` no cubre (U+206A–U+206F) sí dan `outside` (guarda ⛔2.2), y `is_dotgit_alias` se prueba directamente con todos.
- **Plataformas.** Este Mac verifica macOS. Linux y Windows quedan para la etapa de validación multiplataforma (XP-45, XP-46); se corre el clippy cruzado de Windows de los crates que compilan en cruzado.
- **Estado de las historias.** US-MCP-009 y US-MCP-010 pasan a `partially-implemented`: siguen abiertos G3 (US-MCP-007) y G4 (US-MCP-011) de DS-009 y G1 (Windows) de DS-010.

## File & Project Topology

Siete tramos disjuntos, en tres olas que siguen los `Depende` de las DS. Ola 1: tramos 1 y 2 (no dependen de nada). Ola 2: tramos 3 y 4 (dependen del 1). Ola 3: tramos 5 y 6 (dependen del 3; el 6 también del 4). El tramo 7 cierra. Los criterios de extremo a extremo (`MCP009-E2E`, `MCP010-E2E`, `CORPUS`) se ponen en verde en la ola 3, con todo el código hecho.

### Tramo 1 — contrato del api (DS-009 T002, DS-010 T002) · criterios: MCP009-API-WIRE, MCP010-API-PATHS, MCP010-G21, MCP010-G22

- `crates/api/src/catalog.rs`
- `crates/api/src/lib.rs` (una línea `pub mod commit_paths;`)
- `crates/api/src/commit_paths.rs`
- `crates/api/src/methods/operation.rs`
- `crates/api/src/timemachine.rs`
- `crates/api/src/mcp_view.rs`

### Tramo 2 — forma del plan de commit en policy (DS-009 T008 paso 1) · criterios: MCP009-POLICY

- `crates/policy/src/guard/plan.rs`

### Tramo 3 — ejecutor, brazo, paso protegido, permiso y Git (DS-009 T003, T006, T008 pasos 2–3; DS-010 T004, T005, T006) · criterios: MCP009-CORE, MCP009-G32, MCP009-G33, MCP009-G34, MCP009-FAIL, MCP009-G61, MCP009-PERM, MCP010-CORE, MCP010-G41, MCP010-G42, MCP010-G43, MCP010-UNTRACKED, MCP010-G52, MCP010-G53, MCP010-SHARED, MCP010-G61, MCP010-G62, MCP010-GIT

- `crates/core/src/executor/ops/commit.rs`
- `crates/core/src/executor/ops/commit_paths.rs`
- `crates/core/src/executor/ops/mod.rs`
- `crates/core/src/executor/ops/test_arms.rs`
- `crates/core/src/executor/mod.rs`
- `crates/core/src/timemachine/protected/mod.rs`
- `crates/core/src/timemachine/protected/scope.rs`
- `crates/core/src/timemachine/protected/backend.rs`
- `crates/core/src/daemon/serve.rs`
- `crates/core/src/guardrails/evaluate.rs`
- `crates/git/src/user_ops.rs`
- `crates/git/src/paths.rs`
- `apps/cli/tests/guard_ts_ckp_003.rs`

### Tramo 4 — herramienta en raptor-mcp (DS-009 T005, DS-010 T003) · criterios: MCP009-MCP, MCP010-MCP-PATHS

- `apps/mcp/src/safe_commit.rs`
- `apps/mcp/src/safe_commit_paths.rs`
- `apps/mcp/src/main.rs`
- `apps/mcp/src/server.rs`
- `apps/mcp/src/engine.rs`
- `apps/mcp/src/messages.rs`
- `apps/mcp/tests/token_budget.rs`
- `apps/mcp/src/safe_commit_progress_tests.rs`

### Tramo 5 — canal y límites por solicitante (DS-009 T004, T007) · criterios: MCP009-CHANNEL, MCP009-G41, MCP009-LIMITS

- `crates/core/src/channel/conn.rs`
- `crates/core/src/channel/limits.rs`
- `crates/core/src/channel/mod.rs`
- `crates/core/src/channel/server.rs`
- `crates/core/tests/us_mcp_009_limits_executor.rs`

### Tramo 6 — corpus de nivel engine y extremo a extremo (DS-009 T009, DS-010 T007) · criterios: CORPUS, MCP009-E2E, MCP010-E2E

- `crates/testkit/src/mcp_corpus/case.rs`
- `crates/testkit/src/mcp_corpus/schema.rs`
- `apps/cli/tests/mcp_corpus/machine.rs`
- `apps/cli/tests/mcp_corpus/cases/safe-commit-path-link-out.json`
- `apps/cli/tests/mcp_corpus/cases/safe-commit-path-link-parent.json`
- `apps/cli/tests/mcp_corpus/cases/safe-commit-path-nested-repo.json`
- `apps/cli/tests/mcp_corpus/cases/safe-commit-path-dot-git-swap.json`
- `apps/cli/tests/mcp_corpus/cases/safe-commit-path-ignored.json`
- `apps/cli/tests/mcp_corpus/cases/safe-commit-path-shared-all-staged.json`
- `apps/cli/tests/mcp_corpus/cases/safe-commit-path-case.json`
- `apps/cli/tests/mcp_corpus/cases/safe-commit-path-skip-worktree.json`
- `apps/cli/tests/mcp_corpus/cases/safe-commit-path-nfd.json`
- `apps/cli/tests/mcp_corpus/cases/safe-commit-path-glob.json`
- `apps/cli/tests/mcp_corpus/cases/safe-commit-path-exclude.json`

### Tramo 7 — documentación (DS-009 T010, T011; DS-010 T008) · criterios: DOC-ADR-MCP, DOC-ADR-CKP, DOC-BR, DOC-IPC, DOC-XPLAT, DOC-RELEASE, DOC-STATUS

- `docs/architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md`
- `docs/architecture/decisions/ADR-CKP-002-catalogo-operaciones-ejecutor.md`
- `docs/requirements/features/mcp/business-rules.md`
- `docs/requirements/release-plan.md`
- `docs/architecture/extender-sin-archivos-compartidos.md`
- `docs/architecture/design/api-contract-ipc.md`
- `docs/architecture/xplat-pendientes.md`
- `docs/requirements/features/mcp/dev-specs/US-MCP-009-dev-spec.md`
- `docs/requirements/features/mcp/dev-specs/US-MCP-010-dev-spec.md`
- `docs/requirements/features/mcp/user-stories/US-MCP-009-safe-commit.md`
- `docs/requirements/features/mcp/user-stories/US-MCP-010-safe-commit-rutas.md`
- `docs/requirements/backlog.md`
- `docs/requirements/release-status.md` (generado)

Las pruebas de los criterios (las escribe el criterion-tester, en rojo) no están en ningún tramo: `contract.json` las lista.

## Convenciones observadas

Las de § Notas del autor → Convenciones observadas de cada DS, sin copiarlas. Además:

- Pruebas de integración de `crates/core` sobre el rig de `crates/core/tests/us_mcp_008_executor.rs` (ejecutor real, oplog en perfil temporal); e2e por proceso real sobre `apps/cli/tests/mcp_snapshot.rs:Machine`.
- Pruebas unitarias de un binario en un archivo propio incluido con `#[path]`: `apps/mcp/src/messages.rs` → `messages_confirmation_tests.rs` (US-MCP-013).

## Inventario de reutilización

El de § Notas del autor → Inventario de reutilización de cada DS, que se sigue tal cual. La preparación no añade necesidades nuevas.

## Mejoras detectadas

- Ninguna fuera de las DS.
