---
schema: nassa.approval-request.v1
plan: "docs/dev-briefs/us-mcp-009-010-safe-commit.md"
---

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: no
- Generada: `2026-10-27T20:54:15.007Z`
- Sha de aprobación: `45069bb19c960958c910d8918b4621e5e58f23dcb1f91481d21ea75a30c69b65:78066912964d8c4941ebcc4d177933a3b9473dd04152f4047a966981b5d0031e:bd90ed9cf3f7db1a57408d0aab490e1a165c3744ffed99a6c9a100591aeda55f`

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES e3dcc5a8dfa2>>>
```text
docs/dev-briefs/us-mcp-009-010-safe-commit.md
```
<<<FIN-DATOS-NO-CONFIABLES e3dcc5a8dfa2>>>

planSha: `45069bb19c960958c910d8918b4621e5e58f23dcb1f91481d21ea75a30c69b65` (base de la cadena de re-planes)

Contenido:

<<<DATOS-NO-CONFIABLES aaafae430a34>>>
```text
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
```
<<<FIN-DATOS-NO-CONFIABLES aaafae430a34>>>

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 5f3b32247633>>>
```text
docs/dev-briefs/us-mcp-009-010-safe-commit.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 5f3b32247633>>>

contractSha: `78066912964d8c4941ebcc4d177933a3b9473dd04152f4047a966981b5d0031e`

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES a3b8ad54f2c3>>>
```text
[1] id: MCP009-API-WIRE
statement: El contrato de commit en crates/api: check_commit_message (required, too-long antes de recorrer caracteres, invalid-chars con \n y \t permitidos y \r solo en \r\n), las formas de commit que van y vuelven por el cable, MCP_COMMIT_CALL_LIMIT = suma de sus partes nombradas y McpToolError cerrado en kebab-case
verify: cargo test -p gitraptor-api --test us_mcp_009_wire
tests:
  - crates/api/tests/us_mcp_009_wire.rs

[2] id: MCP010-API-PATHS
statement: check_commit_path/check_commit_paths clasifican la tabla de § Contrato de API de DS-010: invalid (absoluta, UNC, unidad, \, controles, bidi, '-' inicial, no canónica), outside (.., .git y sus alias en todas las plataformas), too-many (257) antes de mirar ninguna
verify: cargo test -p gitraptor-api --test us_mcp_010_paths
tests:
  - crates/api/tests/us_mcp_010_paths.rs

[3] id: MCP010-G21
statement: ⛔2.1 ':(glob)**' y ':!x' pasan la forma léxica: son nombres literales
verify: cargo test -p gitraptor-api --test us_mcp_010_paths -- --list 2>/dev/null | grep -q '^pathspec_magic_is_a_literal_name_for_the_lexical_check: test$' && cargo test -p gitraptor-api --test us_mcp_010_paths pathspec_magic_is_a_literal_name_for_the_lexical_check -- --exact
tests:
  - crates/api/tests/us_mcp_010_paths.rs

[4] id: MCP010-G22
statement: ⛔2.2 los alias de .git (mayúsculas, GIT~1, punto o espacio final, sufijo ':', ignorables de HFS+ incluidos U+206A..U+206F) son 'outside' en todas las plataformas
verify: cargo test -p gitraptor-api --test us_mcp_010_paths -- --list 2>/dev/null | grep -q '^dot_git_aliases_are_outside_on_every_platform: test$' && cargo test -p gitraptor-api --test us_mcp_010_paths dot_git_aliases_are_outside_on_every_platform -- --exact
tests:
  - crates/api/tests/us_mcp_010_paths.rs

[5] id: MCP009-CORE
statement: El brazo commit de producción planea y corre como operación protegida gobernada: rutas con seguimiento o todo lo preparado, punta releída bajo el cerrojo con oid y rutas del commit (también rama sin nacer), rechazos antes de cualquier efecto (HEAD separado, rebase en curso, sin git), previo que no se puede tomar deja la rama y nombra la operación, registro con agente y canal mcp
verify: cargo test -p gitraptor-core --test us_mcp_009
tests:
  - crates/core/tests/us_mcp_009.rs

[6] id: MCP009-G32
statement: ⛔3.2 un commit afecta solo al trabajo propio del solicitante (Affected::Agent del solicitante, fijado por el ejecutor)
verify: cargo test -p gitraptor-core --test us_mcp_009 -- --list 2>/dev/null | grep -q '^a_commit_affects_only_the_requesters_own_work: test$' && cargo test -p gitraptor-core --test us_mcp_009 a_commit_affects_only_the_requesters_own_work -- --exact
tests:
  - crates/core/tests/us_mcp_009.rs

[7] id: MCP009-G33
statement: ⛔3.3 un .git reescrito tras prepare no commitea nada en ninguno de los dos repos (git_dir de la puerta del registro, brazo de producción)
verify: cargo test -p gitraptor-core --test us_mcp_009 -- --list 2>/dev/null | grep -q '^a_dot_git_rewritten_after_prepare_commits_nothing_in_either_repo: test$' && cargo test -p gitraptor-core --test us_mcp_009 a_dot_git_rewritten_after_prepare_commits_nothing_in_either_repo -- --exact
tests:
  - crates/core/tests/us_mcp_009.rs

[8] id: MCP009-G34
statement: ⛔3.4 un hook que imprime una línea de éxito no hace que el commit entre: decide la punta, no la salida de git
verify: cargo test -p gitraptor-core --test us_mcp_009 -- --list 2>/dev/null | grep -q '^a_hook_that_prints_a_success_line_does_not_make_a_commit_land: test$' && cargo test -p gitraptor-core --test us_mcp_009 a_hook_that_prints_a_success_line_does_not_make_a_commit_land -- --exact
tests:
  - crates/core/tests/us_mcp_009.rs

[9] id: MCP009-FAIL
statement: Fallos tipados del paso: hook que rechaza = failed-unchanged; hook sobre el tiempo = time-limit con id y rama igual; time-limit tras entrar el commit = éxito con su oid; espera en cola sobre su tope = queue-full sin efectos; constantes del plazo iguales a las del daemon; una conexión caída no detiene un commit en curso
verify: cargo test -p gitraptor-core --test us_mcp_009_failures
tests:
  - crates/core/tests/us_mcp_009_failures.rs

[10] id: MCP009-G61
statement: ⛔6.1 una segunda escritura del mismo agente en el mismo repo se rechaza con write-in-progress sin encolarse
verify: cargo test -p gitraptor-core --test us_mcp_009_failures -- --list 2>/dev/null | grep -q '^a_second_write_of_the_same_agent_is_refused_without_queueing: test$' && cargo test -p gitraptor-core --test us_mcp_009_failures a_second_write_of_the_same_agent_is_refused_without_queueing -- --exact
tests:
  - crates/core/tests/us_mcp_009_failures.rs

[11] id: MCP009-LIMITS
statement: S-03: la 9.ª conexión del mismo agente se rechaza (requester-connections), sus conexiones comparten el cubo de escrituras, la CLI lanzada por el agente y los descendientes del ejecutor cuentan en su clave, el desarrollador y el cliente del hook no toman plaza, y una conexión cerrada libera la suya
verify: cargo test -p gitraptor-core --test us_mcp_009_limits
tests:
  - crates/core/tests/us_mcp_009_limits.rs

[12] id: MCP009-CHANNEL
statement: El canal sirve commit solo con CAP_OPERATION_COMMIT: sin ella, prepare de commit responde NOT_IMPLEMENTED con implemented_by US-MCP-009; con ella, el resultado de run lleva commit (oid, rutas) y los fallos llevan failure
verify: cargo test -p gitraptor-core --test us_mcp_009_channel
tests:
  - crates/core/tests/us_mcp_009_channel.rs

[13] id: MCP009-G41
statement: ⛔4.1 un commit pedido por MCP gasta el cubo de escrituras de la conexión (20/min, ráfaga 5)
verify: cargo test -p gitraptor-core --test us_mcp_009_channel -- --list 2>/dev/null | grep -q '^a_commit_spends_the_connections_write_bucket: test$' && cargo test -p gitraptor-core --test us_mcp_009_channel a_commit_spends_the_connections_write_bucket -- --exact
tests:
  - crates/core/tests/us_mcp_009_channel.rs

[14] id: MCP009-PERM
statement: La decisión que cuenta de un commit aplica el permiso de US-GRD-007: commit en ask se ve como ask, en deny se deniega con permission.commit, y los hooks bajo un safe_commit reutilizan la decisión ligada y dejan una sola entrada
verify: cargo test -p gitraptor-core --test us_mcp_009_permission
tests:
  - crates/core/tests/us_mcp_009_permission.rs

[15] id: MCP009-POLICY
statement: normalize de PlanShape::Commit evalúa commit-msg, la transición de la rama y pre-commit, en ese orden, con sus preset alineados por índice
verify: cargo test -p gitraptor-policy --lib -- --list 2>/dev/null | grep -q '^guard::plan::commit_tests::a_commit_plan_evaluates_commit_msg_branch_and_pre_commit_in_order: test$' && cargo test -p gitraptor-policy --lib guard::plan::commit_tests::
tests:
  - crates/policy/src/guard/plan_commit_tests.rs

[16] id: MCP009-MCP
statement: La herramienta safe_commit de raptor-mcp: catálogo con status, snapshot y safe_commit; parámetros desconocidos y reservados = invalid-params; un mensaje no válido nunca llega al motor; los rechazos del motor se traducen a los códigos de la herramienta; progreso solo con progressToken; cada rechazo cabe en su presupuesto en en y es; una lista larga de rutas se recorta; los textos en es dicen lo que dice la historia
verify: cargo test -p gitraptor-mcp --bin raptor-mcp -- --list 2>/dev/null | grep -q '^safe_commit::tests::the_catalog_has_status_snapshot_and_safe_commit: test$' && cargo test -p gitraptor-mcp --bin raptor-mcp safe_commit::tests::
tests:
  - apps/mcp/src/safe_commit_tests.rs

[17] id: MCP010-MCP-PATHS
statement: raptor-mcp rechaza una ruta mala sin llamar al motor; 257 rutas = too-many-paths con max 256; la ruta rechazada viaja como untrusted; los textos en es de las rutas dicen lo que dice la historia
verify: cargo test -p gitraptor-mcp --bin raptor-mcp -- --list 2>/dev/null | grep -q '^safe_commit_paths::tests::a_bad_path_never_reaches_the_engine: test$' && cargo test -p gitraptor-mcp --bin raptor-mcp safe_commit_paths::tests::
tests:
  - apps/mcp/src/safe_commit_paths_tests.rs

[18] id: MCP009-E2E
statement: Los escenarios de US-MCP-009 por proceso real (daemon, raptor-mcp, raptor-hook, agente simulado, git): commit con previo y hooks, respuesta sin mensaje, --only, autoría denegada como el hook, decisión ligada con una entrada, mensajes no válidos, nada que commitear, cancelación y desconexión que terminan el commit, reintento = operation-in-progress, ask = requiere confirmación, sin peticiones del servidor, cliente que corta antes (macOS)
verify: cargo test -p gitraptor-cli --test mcp_safe_commit
tests:
  - apps/cli/tests/mcp_safe_commit.rs

[19] id: MCP010-E2E
statement: Los escenarios de US-MCP-010 por proceso real: ruta fuera rechaza la llamada entera, rutas no válidas nunca llegan a git, enlace que sale, demasiadas rutas, magia de pathspec = no encontrada, archivo nuevo solo nombrado, ignorado nunca, worktree compartido exige rutas, la TUI del desarrollador no lo vuelve compartido (macOS)
verify: cargo test -p gitraptor-cli --test mcp_safe_commit_paths
tests:
  - apps/cli/tests/mcp_safe_commit_paths.rs

[20] id: MCP010-CORE
statement: El daemon contrasta cada ruta con el worktree: padre enlace, repo anidado = outside; enlace interno se commitea como enlace; enlace .gitmodules, carpeta = invalid; borrado con seguimiento se commitea; enlace cambiado entre prepare y run = state-changed; NFD casa con NFC si git precompone; reglas de .git iguales a tree_path; ignorados de info/exclude y del usuario
verify: cargo test -p gitraptor-core --test us_mcp_010
tests:
  - crates/core/tests/us_mcp_010.rs

[21] id: MCP010-G41
statement: ⛔4.1 una ruta con otras mayúsculas no se encuentra (sin plegado)
verify: cargo test -p gitraptor-core --test us_mcp_010 -- --list 2>/dev/null | grep -q '^a_path_with_other_case_is_not_found: test$' && cargo test -p gitraptor-core --test us_mcp_010 a_path_with_other_case_is_not_found -- --exact
tests:
  - crates/core/tests/us_mcp_010.rs

[22] id: MCP010-G42
statement: ⛔4.2 un .git apuntado a otro repo tras prepare no commitea nada
verify: cargo test -p gitraptor-core --test us_mcp_010 -- --list 2>/dev/null | grep -q '^a_dot_git_pointed_at_another_repo_after_prepare_commits_nothing: test$' && cargo test -p gitraptor-core --test us_mcp_010 a_dot_git_pointed_at_another_repo_after_prepare_commits_nothing -- --exact
tests:
  - crates/core/tests/us_mcp_010.rs

[23] id: MCP010-G43
statement: ⛔4.3 una entrada skip-worktree o fuera del sparse checkout es invalid y no commitea su borrado
verify: cargo test -p gitraptor-core --test us_mcp_010 -- --list 2>/dev/null | grep -q '^a_skip_worktree_or_sparse_entry_is_invalid_and_commits_no_deletion: test$' && cargo test -p gitraptor-core --test us_mcp_010 a_skip_worktree_or_sparse_entry_is_invalid_and_commits_no_deletion -- --exact
tests:
  - crates/core/tests/us_mcp_010.rs

[24] id: MCP010-UNTRACKED
statement: Un archivo nuevo entra solo nombrado (también en una carpeta nueva); un commit rechazado, por tiempo o cancelado deja el índice como estaba
verify: cargo test -p gitraptor-core --test us_mcp_010_untracked
tests:
  - crates/core/tests/us_mcp_010_untracked.rs

[25] id: MCP010-G52
statement: ⛔5.2 un commit rechazado conserva lo que estaba preparado antes (rm --cached solo de add_first)
verify: cargo test -p gitraptor-core --test us_mcp_010_untracked -- --list 2>/dev/null | grep -q '^a_rejected_commit_keeps_what_was_staged_before: test$' && cargo test -p gitraptor-core --test us_mcp_010_untracked a_rejected_commit_keeps_what_was_staged_before -- --exact
tests:
  - crates/core/tests/us_mcp_010_untracked.rs

[26] id: MCP010-G53
statement: ⛔5.3 un index.lock huérfano tras un time-limit da failed-changed y el lock no se borra
verify: cargo test -p gitraptor-core --test us_mcp_010_untracked -- --list 2>/dev/null | grep -q '^an_orphan_index_lock_after_a_time_limit_is_failed_changed: test$' && cargo test -p gitraptor-core --test us_mcp_010_untracked an_orphan_index_lock_after_a_time_limit_is_failed_changed -- --exact
tests:
  - crates/core/tests/us_mcp_010_untracked.rs

[27] id: MCP010-GIT
statement: UserOp::AddPaths nunca fuerza (sin -f, -A, -u; pathspec :(literal) tras --) y git rechaza un add más allá de un enlace simbólico
verify: cargo test -p gitraptor-git --lib -- --list 2>/dev/null | grep -q '^user_ops::add_tests::add_paths_never_forces: test$' && cargo test -p gitraptor-git --lib user_ops::add_tests::
tests:
  - crates/git/src/user_ops_add_tests.rs

[28] id: MCP010-SHARED
statement: Con otra sesión viva de agente, todo lo preparado se rechaza con la acción; con rutas, commitea solo esas; la sesión del desarrollador no cuenta; commit nunca lleva el aviso active-session
verify: cargo test -p gitraptor-core --test us_mcp_010_shared
tests:
  - crates/core/tests/us_mcp_010_shared.rs

[29] id: MCP010-G61
statement: ⛔6.1 una presencia ilegible cuenta como worktree compartido
verify: cargo test -p gitraptor-core --test us_mcp_010_shared -- --list 2>/dev/null | grep -q '^an_unreadable_presence_counts_as_shared: test$' && cargo test -p gitraptor-core --test us_mcp_010_shared an_unreadable_presence_counts_as_shared -- --exact
tests:
  - crates/core/tests/us_mcp_010_shared.rs

[30] id: MCP010-G62
statement: ⛔6.2 la sesión sin fin de un agente cuyo proceso murió no bloquea todo lo preparado
verify: cargo test -p gitraptor-core --test us_mcp_010_shared -- --list 2>/dev/null | grep -q '^a_crashed_agents_session_does_not_block_all_staged: test$' && cargo test -p gitraptor-core --test us_mcp_010_shared a_crashed_agents_session_does_not_block_all_staged -- --exact
tests:
  - crates/core/tests/us_mcp_010_shared.rs

[31] id: CORPUS
statement: El corpus de INF-MCP-001 con los casos safe-commit-* de DS-009 T009 y safe-commit-path-* de DS-010 T007 es válido, único y se rechaza al 100 %
verify: cargo test -p gitraptor-cli --test mcp_security_corpus
tests:
  - apps/cli/tests/mcp_corpus/cases/safe-commit-message-bidi.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-message-empty.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-message-nul.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-message-too-long.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-param-acknowledge.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-param-amend.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-param-expect-worktree.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-param-no-verify.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-param-repo.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-absolute.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-bidi.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-dot-git-hfs.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-dot-git-short.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-dot-git-trailing.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-dot-git.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-drive.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-nul.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-option.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-too-many.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-traversal.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-unc.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-windows-reserved.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-windows-stream.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-windows-trailing-dot.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-path-windows-trailing-space.json
  - apps/cli/tests/mcp_corpus/cases/safe-commit-unattributed.json

[32] id: DOC-ADR-MCP
statement: ADR-MCP-001 lleva las Enmiendas (2026-10-21, US-MCP-009) y (2026-10-21, US-MCP-010)
verify: grep -q 'Enmienda (2026-10-21, US-MCP-009)' docs/architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md && grep -q 'Enmienda (2026-10-21, US-MCP-010)' docs/architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md
tests:
  (ninguno)

[33] id: DOC-ADR-CKP
statement: ADR-CKP-002 lleva la Enmienda (2026-10-21, US-MCP-009) de § 3
verify: grep -q 'Enmienda (2026-10-21, US-MCP-009)' docs/architecture/decisions/ADR-CKP-002-catalogo-operaciones-ejecutor.md
tests:
  (ninguno)

[34] id: DOC-BR
statement: business-rules.md recoge S-11 (una segunda escritura del mismo agente se rechaza sin encolarse) y el desarrollador que no cuenta como sesión presente
verify: grep -q 'sin encolarse' docs/requirements/features/mcp/business-rules.md && grep -q 'el desarrollador no cuenta' docs/requirements/features/mcp/business-rules.md
tests:
  (ninguno)

[35] id: DOC-IPC
statement: api-contract-ipc.md documenta operation.commit, requester-connections y shared-worktree-needs-paths
verify: grep -q 'operation.commit' docs/architecture/design/api-contract-ipc.md && grep -q 'requester-connections' docs/architecture/design/api-contract-ipc.md && grep -q 'shared-worktree-needs-paths' docs/architecture/design/api-contract-ipc.md
tests:
  (ninguno)

[36] id: DOC-XPLAT
statement: xplat-pendientes.md tiene las filas XP-45 y XP-46
verify: grep -q 'XP-45' docs/architecture/xplat-pendientes.md && grep -q 'XP-46' docs/architecture/xplat-pendientes.md
tests:
  (ninguno)

[37] id: DOC-RELEASE
statement: release-plan.md dice que US-MCP-009 se libera junto con US-MCP-010 y US-MCP-011
verify: grep -q 'US-MCP-009 se libera junto con US-MCP-010 y US-MCP-011' docs/requirements/release-plan.md
tests:
  (ninguno)

[38] id: DOC-STATUS
statement: US-MCP-009 y US-MCP-010 quedan partially-implemented con la línea 'Implementado en: PR'
verify: grep -q '^status: partially-implemented' docs/requirements/features/mcp/user-stories/US-MCP-009-safe-commit.md && grep -q 'Implementado en: PR' docs/requirements/features/mcp/user-stories/US-MCP-009-safe-commit.md && grep -q '^status: partially-implemented' docs/requirements/features/mcp/user-stories/US-MCP-010-safe-commit-rutas.md && grep -q 'Implementado en: PR' docs/requirements/features/mcp/user-stories/US-MCP-010-safe-commit-rutas.md
tests:
  (ninguno)
```
<<<FIN-DATOS-NO-CONFIABLES a3b8ad54f2c3>>>

## Tests de criterio

testsSha: `bd90ed9cf3f7db1a57408d0aab490e1a165c3744ffed99a6c9a100591aeda55f`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES 1c48cf4fe1d4>>>
```text
724721ef1afe0b6504bed7033d4a3f021152ece76926cf69d3c27538f93f035d  apps/cli/tests/mcp_corpus/cases/safe-commit-message-bidi.json
3a9600d7f2bbd5fd0cc28cba0d10985697210909432982b603c03b4257721a00  apps/cli/tests/mcp_corpus/cases/safe-commit-message-empty.json
815cc84fa18406e6ed6e856090f42420f7bf985398b2d2f9bcc53bdde9d70f81  apps/cli/tests/mcp_corpus/cases/safe-commit-message-nul.json
ad6da733092f48caf71736c31241e19d4e1d1a51a75c790ff3dde68f2e820564  apps/cli/tests/mcp_corpus/cases/safe-commit-message-too-long.json
677e67ab6bd6182420e7a1f23affd27108d6066596fad188c5fbce32c02b5883  apps/cli/tests/mcp_corpus/cases/safe-commit-param-acknowledge.json
3366e90ece4391b1600e12a12ebe4cae61096eb1023b0207094e541b8835c061  apps/cli/tests/mcp_corpus/cases/safe-commit-param-amend.json
9894f3bebccc6cb72ed7e6873649d7d6802a6de3c943ef27016ec4f7b5b348d4  apps/cli/tests/mcp_corpus/cases/safe-commit-param-expect-worktree.json
3942cc02ea86e20774503e740139e6bc15411b2b93e79ce56f5d650c35d671ee  apps/cli/tests/mcp_corpus/cases/safe-commit-param-no-verify.json
0039c3b1b799cba7ab3a9206fab25a79159532410f868da7193c58e7a5700cab  apps/cli/tests/mcp_corpus/cases/safe-commit-param-repo.json
426d9d47bb30d5281a878cb103c8398cc6f3ef87f61a7e3fdd6eeeeadfcb9ecb  apps/cli/tests/mcp_corpus/cases/safe-commit-path-absolute.json
1c29ea46ed23388456a11b5f23c9f3c4c5c5a12556007f46030fe51ffedea461  apps/cli/tests/mcp_corpus/cases/safe-commit-path-bidi.json
c8a82d9f65255b6afed98977c683009f4b8b6425cef4ade57b78a8c732e6827b  apps/cli/tests/mcp_corpus/cases/safe-commit-path-dot-git-hfs.json
896e0f6b1adf6dddbe67b7e3af9f147efb88f1ebef0e6df0a38051233529c124  apps/cli/tests/mcp_corpus/cases/safe-commit-path-dot-git-short.json
efd9771c47805c653ccc7deb7cd8d337b99695f0ad31e64b011dfc6ecc8ad024  apps/cli/tests/mcp_corpus/cases/safe-commit-path-dot-git-trailing.json
ac5c256f62de4fa00cadc1e2acdd833ac58085797876ad43e6ab61ec7c32b44d  apps/cli/tests/mcp_corpus/cases/safe-commit-path-dot-git.json
0eb8ea582e45f1f13b595c9362860cdfbda7de5a747cab5396f3f6fc5b68eab5  apps/cli/tests/mcp_corpus/cases/safe-commit-path-drive.json
cdf9aed65d1a117874b575f9780f58e75a9bc2fedafbb71a053f3b55c2ffa877  apps/cli/tests/mcp_corpus/cases/safe-commit-path-nul.json
cc42bc7966cbad5eaf7933321ba5865c14e5e4e93ef67b4d4a596fce950489a7  apps/cli/tests/mcp_corpus/cases/safe-commit-path-option.json
43438b5ca97f673275c1537249a2b52b0e80938aa56048553055cea3b8929d61  apps/cli/tests/mcp_corpus/cases/safe-commit-path-too-many.json
a4f9a31c971661d942058d8a7fb8624a1dbcb9e762e86e34f7d722d3a51a6617  apps/cli/tests/mcp_corpus/cases/safe-commit-path-traversal.json
77eff7bf3575a06a1c75c414c17c37ef21abfd27e0366ee5c05857b01c341e74  apps/cli/tests/mcp_corpus/cases/safe-commit-path-unc.json
af5b888f46b1ed5eb55bfd2429dc5d3bbe68badc76fe8aeb49447503f448f2fe  apps/cli/tests/mcp_corpus/cases/safe-commit-path-windows-reserved.json
315e6c502645e51311d7d222222a1c8435c1a4bc8e4db6e51879f4ebb0977900  apps/cli/tests/mcp_corpus/cases/safe-commit-path-windows-stream.json
c819b19723973e9ccffe81388ce21031837a35c4d47eb38e27868e72a41dce2d  apps/cli/tests/mcp_corpus/cases/safe-commit-path-windows-trailing-dot.json
4fe502dc03ee496a33208694db6b62a4a31d7a478212e6a745eb564485cd70cb  apps/cli/tests/mcp_corpus/cases/safe-commit-path-windows-trailing-space.json
2fd6a4ece774f5801e7ee4be7127abfafa7008a07db6194413aaf692d5ad69a4  apps/cli/tests/mcp_corpus/cases/safe-commit-unattributed.json
875d531a8642359f72280209b094976fd9b00f67ff712e2fc1fd8a797046059c  apps/cli/tests/mcp_safe_commit.rs
2745fd36deb550d76ab9dd5ce46b12d698b9715711d00eae0fc10310b9df3f5d  apps/cli/tests/mcp_safe_commit_paths.rs
df26f110c08a337b2c0372439defb9a0f563ccda0ff520519775a912944c19a5  apps/mcp/src/safe_commit_paths_tests.rs
281a7c705202bbf3a1faee52ba738d1b2d0e19b0da782d7edf88f180cc617200  apps/mcp/src/safe_commit_tests.rs
5bc6ff9589e1976ec29835e82a6544ffea6de9323eab971e4b1d78b5bc6d5f2e  crates/api/tests/us_mcp_009_wire.rs
25bda30d84346a982a77ba103e0a1f0b354349d96faa3c50ae5ae9d03e364a10  crates/api/tests/us_mcp_010_paths.rs
01fbe279cc2a5150492f3f7f567e04790ed6168cf857ed492576adb5ff954e86  crates/core/tests/us_mcp_009.rs
b2deb623a96d89a48311d7cf2a305b75e22d0caa6f13c03378ea6776c8599576  crates/core/tests/us_mcp_009_channel.rs
afa166303a473a74a4c61195b461a311e819ea68522d385ff28d084a0946c73f  crates/core/tests/us_mcp_009_failures.rs
75a9fa8aa467ff20d31eee35a33e994340e0f3e6eaadbff999c1280f98d5ace8  crates/core/tests/us_mcp_009_limits.rs
51f784f4ce5611e83b8f2b5bdac93e1fc2cb9cad4f8902040a3d063dfa52023c  crates/core/tests/us_mcp_009_permission.rs
41b85e2333494e61d1ebb6e3a92360536c159563665b6f4b3605825bea7c3724  crates/core/tests/us_mcp_010.rs
a3f15977a13e8081e63128fc9eb376456b1587e0cffa7e0f458a940be8abdbd2  crates/core/tests/us_mcp_010_shared.rs
dbd4df82357c0504f1839abfd7242b241bf6e0f2f04256ec6a6cc8f9a0f11860  crates/core/tests/us_mcp_010_untracked.rs
c774a501d05fc2be3fc31117feb1ca407ded8761b099ae80d9c73d2711b064ab  crates/git/src/user_ops_add_tests.rs
ee829f1d2fc41114c44705a89a21f8144dab7f6adcc2b43fb7a19a3dc69d2a36  crates/policy/src/guard/plan_commit_tests.rs
```
<<<FIN-DATOS-NO-CONFIABLES 1c48cf4fe1d4>>>

## Crítico

- Rondas (plan): 3
  - ronda 1: AJUSTES · 3 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: AJUSTES · 1 bloqueante(s) · siguiente: excepcion
  - ronda 3: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
- Último veredicto: APROBABLE
- Excepción: no

## Diferidos

Ninguno: el crítico dio el plan por aprobable.

## Firmas

- planSha (contexto, no firmado): `45069bb19c960958c910d8918b4621e5e58f23dcb1f91481d21ea75a30c69b65`
- contractSha: `78066912964d8c4941ebcc4d177933a3b9473dd04152f4047a966981b5d0031e`
- testsSha: `bd90ed9cf3f7db1a57408d0aab490e1a165c3744ffed99a6c9a100591aeda55f`
- approvalSha: `45069bb19c960958c910d8918b4621e5e58f23dcb1f91481d21ea75a30c69b65:78066912964d8c4941ebcc4d177933a3b9473dd04152f4047a966981b5d0031e:bd90ed9cf3f7db1a57408d0aab490e1a165c3744ffed99a6c9a100591aeda55f`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
