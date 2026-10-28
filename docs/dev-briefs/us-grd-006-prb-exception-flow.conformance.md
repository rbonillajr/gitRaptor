# Conformidad · US-GRD-006 PR-B (T005–T011) · 2026-10-28

Base 0e4f450b, trabajo sin commitear. Hechos dados: los 26 tests del contrato y `same_decision_for_every_actor` pasan. Escalón 3 = el test lanza `raptor guard exec` / `raptor-hook` / el daemon hijo reales (`apps/cli/tests/*`). Escalón 2 = integración en hilo (`crates/core/tests`) o contrato (`crates/api/tests`). La consulta de capas da `no-medible` en las 11 tareas (sin pruebas mapeadas), así que los escalones salen de leer los tests.

## 1. Cláusulas

| Cláusula | Tarea dueña | Escalón | Evidencia |
|---|---|---|---|
| Historia · esc. 1 misma decisión para cualquier actor | T005 | 3 | `guard_us_grd_006.rs:same_decision_for_every_actor` |
| Historia · esc. 2 anuncio, ventana, ejecución, una entrada `exception` | T006/T007/T009 | 3 | `guard_us_grd_006.rs:exception_is_announced_waits_and_runs_once` |
| Historia · esc. 3 cancelada, queda en registro | T006/T009 | 3 | `a_cancelled_exception_does_not_run` |
| Historia · esc. 4 una sola operación | T007 | 3 | `the_exception_covers_one_operation` |
| Historia · esc. 5 agente rechazado y registrado | T007 | 3 | `an_agent_cannot_use_the_exception` |
| Historia · esc. 6 fuera de alcance rechazado sin anunciar | T009 | 3 | `an_unsupported_operation_is_refused_before_the_announcement` |
| §6 · Ctrl-C cancela (D8) | T009 | 3 | `ctrl_c_in_the_window_cancels`, `core/src/interrupt.rs:interrupt_flag` |
| §6 · Q-GRD-50 política `everyone` | T007 | 3 | `the_exception_skips_an_everyone_policy` |
| §6 · D5 `diffSize`/`commitFormat` en rebase | T005 | **0** | Sin test: excluido por S-B7 (DS-US-GRD-009 sin merge). `exceptable` ya obliga a clasificarlas |
| §6 · MCP: `not-available-to-mcp`, catálogo sin la excepción | T007 | 3 | `the_mcp_never_reaches_the_exception` |
| §6 · US-GRD-017 sin punto de recuperación | T007/T010 | 3 | `exception_runs_without_a_recovery_point` |
| §6 · D5 regla no anunciada | T007 | 3 | `a_rule_not_announced_is_not_skipped` |
| §6 · D5 `not-denied` | T007 | 3 | `an_allowed_operation_needs_no_exception` |
| §6 · D12 plantilla 3, `outdated-install` | T008/T009 | 3 | `an_old_template_refuses_before_the_announcement`, `constants.rs:TEMPLATE_VERSION` |
| §6 · degradado sin daemon | T009 | 3 | `no_exception_without_the_engine` |
| §6 · Val. 7 token copiado | T007/T008 | 3 | `guard_exception_token.rs:a_copied_token_is_rejected` |
| §6 · Val. 7 TTL vencido | T006 | 3 | `an_expired_token_is_rejected` |
| §6 · Val. 7 otra transición | T007 | 3 | `another_transition_is_not_covered` |
| §6 · Val. 8 normalización y rechazos | T007 | 3 | `argv_is_normalized_or_refused_before_the_token`, `exception_facts.rs` |
| §6 · Val. 9 ejecutables neutralizados | T009 | 3 | `configured_executables_are_neutralized`, `crates/git/src/cli.rs:textconv_drivers` |
| §6 · Val. 10 el token no se filtra | T008/T009 | 3 | `the_token_never_leaks` |
| §6 · D19 capacidad en ambas direcciones | T007 | 2 | `core/tests/us_grd_006_channel.rs:exception_capability_both_ways` |
| §6 · `apply` antes de `hook_prior`, una entrada | T007 | 2 | `apply_orders_before_the_prior_and_logs_once` |
| D11 d / S-B12 sin auditoría `used` no hay excepción | T007 | 2 | `no_exception_without_the_used_audit` |
| Revisión PR-A: token mal formado no tumba evaluate | T007 | 2 | `a_malformed_exception_token_does_not_break_evaluate` |
| S1 tipos kebab-case | T007 | 2 | `api/tests/us_grd_006_prb_contract.rs:exception_pending_and_log_kinds_round_trip` |
| S3/S4 métodos registrados y `GUARD_EXCEPTION_REFUSED` (D20) | T007 | 2 | `exception_methods_are_implemented_and_their_error_registered`, `contract.txt` |
| D3/D18 `Debug` sin argv | T007 | 2 | `exception_params_debug_redacts_argv` |
| D1 `--yes` no acorta la ventana | T009 | 1 | `--yes` aparece en las dos suites e2e, pero ningún test aserta que la ventana no se acorta |
| D17 el rechazo del agente no sondea el disco | T007 | **0** | `conn/exception.rs:guard_exception`; ningún test aserta la ausencia de lectura (el registro sí lo cubre `an_agent_cannot_use_the_exception`) |
| D16 `action_id` en la clave de agregación (S-B2) | T007 | 1 | `log.rs:LogEntry.action_id`; solo lo ejerce indirectamente «una sola entrada» de `apply_orders_before_the_prior_and_logs_once` |
| D14 / D15 `ask` y `confirmation-required` | — | n/a | La spec dice «ninguna en el código»: sin comportamiento observable |
| T010 estado y registro en la CLI | T010 | 3 | `guard_us_grd_006.rs` (texto del pendiente y del registro en esc. 2 y 3) |
| T011 documentos (historia, US-GRD-017, spec §10, backlog, release-status) | T011 | 0 | Pendiente del orquestador tras el número de PR. Las enmiendas de § 9 (ADR-GRD-007/001/006) sí están hechas |
| Windows (handle y hora de creación, Val. 12) | T006 | 0 | Pendiente declarado: etapa multiplataforma (S-B11) |

Ninguna cláusula de la historia queda sin tarea dueña.

## 2. Sobra

| Qué sobra | `file:symbol` | Cláusula o tarea | Veredicto |
|---|---|---|---|
| Módulo extra de canal | `crates/core/src/channel/conn/exception.rs` | T007 (la spec §2 lo pone en `conn.rs`; ADR-GRP-016 pide archivo propio) | justificado |
| Lector de hechos del repo | `guardrails/exception_facts.rs` | S-B6 / T007 | justificado |
| Bandera de interrupción | `core/src/interrupt.rs:interrupt_flag` | S-B4 / T009 | justificado |
| Envoltorio de Git CLI con `textconv_drivers` y sus tests | `crates/git/src/cli.rs`, `crates/git/tests/cli_invoke.rs` | Val. 9 (S-B6) | justificado |
| Reescritura de la aprobación | `docs/dev-briefs/US-GRD-006-excepcion-consciente.approval.md` | Ninguna: la regenera `/implement` | colateral |
| Ajustes de tests existentes | `guard_legacy_install_recovery.rs`, `guard_td_grd_001_upgrade.rs`, `guard_us_grd_018.rs`, `td_grd_001_client.rs`, `us_grd_006_contract.rs` | Subida de `TEMPLATE_VERSION` y campo nuevo | colateral |

Ninguno `no pedido`.
