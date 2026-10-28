# Conformidad · US-MCP-009 y US-MCP-010 (2026-10-27)

Recorrido de un revisor que no escribió el código (`rust-code-reviewer`, sonnet) sobre `b2bd6484..HEAD`. Escalón: 0 declarada · 1 una prueba la nombra · 2 la prueba existe y pasa · 3 verificada por la superficie real (`raptor-mcp` por stdio o el corpus).

## Resumen por especificación

| Cláusula | Tarea dueña | Escalón | Evidencia |
|---|---|---|---|
| DS-009 T001, e2e de la historia | T001 | 3 | `apps/cli/tests/mcp_safe_commit.rs` (18/18) |
| DS-009 T002, contrato del api | T002 | 2 | `crates/api/tests/us_mcp_009_wire.rs` |
| DS-009 T003, brazo de producción (⛔3.1 a ⛔3.4) | T003 | 2 | `crates/core/tests/us_mcp_009.rs`; `ops/mod.rs:production_wiring_has_no_governed_arm_without_guardrails` |
| DS-009 T004, canal y capacidad (⛔4.1) | T004 | 2 | `crates/core/tests/us_mcp_009_channel.rs` |
| DS-009 T005, herramienta `safe_commit` | T005 | 2, e2e 3 | `apps/mcp/src/safe_commit_tests.rs`, `safe_commit_progress_tests.rs`, `apps/mcp/tests/token_budget.rs` |
| DS-009 T006, fallos tipados y S-11 (⛔6.1) | T006 | 2 | `crates/core/tests/us_mcp_009_failures.rs` |
| DS-009 T007, límites por solicitante | T007 | 2 | `crates/core/tests/us_mcp_009_limits.rs`, `crates/core/src/channel/limits_tests.rs`, `crates/core/tests/requester_budget_any_client.rs` |
| DS-009 T008, permiso en el plan (⛔8.1, ⛔8.2) | T008 | 2 | `crates/core/tests/us_mcp_009_permission.rs`, `crates/policy/src/guard/plan_commit_tests.rs` |
| DS-009 T009, corpus | T009 | 3 | `apps/cli/tests/mcp_security_corpus.rs` |
| DS-009 T010 y T011, documentos | T010, T011 | 1 | ADR-MCP-001, ADR-CKP-002, BR v0.5, `api-contract-ipc.md`, XP-47; aceptación = revisión |
| DS-009 § 9.4 (10 comportamientos) | T003–T008 | 2–3 | e2e para 1 a 5; core para 6 a 10 |
| US-MCP-009 escenarios 1, 2, 3, 4, 6, 7, 8 y certificación | T003–T008 | 3 | e2e de `mcp_safe_commit.rs` |
| US-MCP-009 escenario 5 (previo o hook que fallan) | T006 | 2 | solo core: `a_prior_that_cannot_be_taken_…`, `a_hook_that_rejects_is_failed_unchanged` |
| DS-010 T001, e2e | T001 | 3 | `apps/cli/tests/mcp_safe_commit_paths.rs` (11/11) |
| DS-010 T002, forma léxica (⛔2.1, ⛔2.2) | T002 | 2 | `crates/api/tests/us_mcp_010_paths.rs` |
| DS-010 T003, rutas en `raptor-mcp` | T003 | 2, e2e 3 | `apps/mcp/src/safe_commit_paths_tests.rs` |
| DS-010 T004, rutas contra el worktree (⛔4.1 a ⛔4.3) | T004 | 2 | `crates/core/tests/us_mcp_010.rs`; `crates/core/src/executor/ops/commit_tests.rs` (vista desde el git dir de la puerta) |
| DS-010 T005, archivos nuevos (⛔5.1 a ⛔5.3) | T005 | 2 | `crates/core/tests/us_mcp_010_untracked.rs`, `crates/git/src/user_ops_add_tests.rs` |
| DS-010 T006, worktree compartido (⛔6.1, ⛔6.2) | T006 | 2 | `crates/core/tests/us_mcp_010_shared.rs`; pruebas de la presencia en `backend.rs` |
| DS-010 T007, corpus de rutas | T007 | 3 | 37 casos `safe-commit-*` |
| DS-010 T008, documentos | T008 | 1 | BR v0.5, XP-46, estado |
| DS-010 § 9.4 y A1–A25 | T002–T007 | 2–3 | tabla de amenazas del `/security-review`: todas OK tras la ronda de arreglos (A11 y S-03 cerrados); A21 fuera del modelo por diseño |
| US-MCP-010 escenarios 1 a 6 | T002–T006 | 3 | e2e de `mcp_safe_commit_paths.rs` |

Sin dueño en la historia: solo lo de gobierno (estado, G3 = US-MCP-007, G4 = US-MCP-011), diferido a propósito por las DS.

## Sobra

- Con decisión registrada: `pub mod commit_paths` (Supuesto del Brief), los presupuestos `MCP_WRITE_TOOL_TOKENS` y `MCP_POLICY_REFUSAL_TOKENS` (Decisiones del orquestador del 2026-10-27, validadas por Arquitecto), la exención de `user_ops_add_tests.rs` en `executor_boundary.rs` y `static_check.rs` (consecuencia del Supuesto de las pruebas unitarias en archivo propio).
- Sin decisión registrada antes, derivadas de la revisión de seguridad: `RepoReader::open_worktree` (M-02, ⛔4.2/A11) y el gasto de los cupos por solicitante en todo cliente con plaza (M-01, D9), con su prueba `requester_budget_any_client.rs`.
- API pública que podría ser `pub(crate)`: `RepoReader::precompose_unicode`, `RepoReader::assume_unchanged_paths`, `MCP_WRITE_TOOLS`, `tool_token_budget`. Pendiente menor.

## Por debajo del escalón 2

- Documentos (DS-009 T010/T011, DS-010 T008): escalón 1, su aceptación es la revisión.
- Linux y Windows: sin verificar (XP-47, XP-46).
- Escenario 5 de US-MCP-009: escalón 2, sin e2e por stdio.
