# Preparación de la corrida · US-GRD-016 (2026-10-28)

Sin diseño: [DS-US-GRD-016](../requirements/features/guardrails/dev-specs/US-GRD-016-misma-decision-por-mcp.md) es el plan (T001–T011, una sola entrega). Contrato: [us-grd-016-mcp-same-decision.contract.json](us-grd-016-mcp-same-decision.contract.json).

## Deriva de citas

Las citas de la spec siguen vigentes en su sentido; `main` avanzó y cambian las líneas. Lo que afecta al build:

| Cita de la spec | Hoy | Efecto |
|---|---|---|
| `evaluate.rs:408` `RuleSet::all()` con `permissions: false` | `evaluate.rs:426-433` | Ninguno |
| `evaluate.rs:488-506` caso especial de `commit` en `decide_all` | `evaluate.rs:505-527` (`commit_permission`, `staged`) | Ninguno |
| `DecideInput` lo construyen `serve_audited`, `plan_gate` y `link.rs` | además `channel/conn/exception.rs:310` (US-GRD-006, PR #301), que llama a `decide` | Motiva S1 |
| `decide_all` | un solo llamador: `plan_gate.rs:293` | Motiva S1 |
| `plan_gate.rs:253` `GuardrailsEngine::decide` | `plan_gate.rs:254`, `DecideInput` en :282 | Ninguno |
| `guard.rs:443` `ProtectionState` | `crates/api/src/guard.rs:473` | Ninguno |
| `guard.rs:1173` `LoggedOperation::ProtectionState`; `:1240` `GuardLogParams` | `:1220`; `:1287` | Ninguno |
| `engine.rs:430` `denied()`; `:34` `MAX_DENIED_RULES` | `:430`; `:35` | Ninguno |
| `mcp_status.rs:331` traducción del estado | `:332-333` | Ninguno |
| `apps/cli/src/guard.rs:547/570/608/871/953` comparaciones con `ProtectionState` | `:639/662/707/970/1052` (cinco, las mismas) | Ninguno |
| `install.rs:341/344/391/602`, `daemon/guard.rs:157` (guarda ⛔2.1) | iguales; `daemon/guard.rs:161` | Ninguno |
| `server.rs` `list_tools` | `server.rs:162-172`: `status`, `snapshot`, `safe_commit` | Ninguno |
| `methods/mod.rs:226` `mcp_never_gets_a_reserved_command` | `:230` | Ninguno |
| `permissions.rs:214` `apply` | `crates/policy/src/guard/permissions.rs:227` | Ninguno |

Ninguna deriva contradice la spec: **sin BLOQUEO**.

## Supuestos

Decisiones del orquestador (2026-10-28).

- **S1 · `known_ops` es un parámetro de `decide_all`, no un campo de `DecideInput`** (desviación de T002 paso 2 y de § Firmas del stack). Validada por el Arquitecto (`architecture-advisor`, opus): ACEPTO. `decide_all` tiene un único llamador y es el único que lo lee; como campo, `decide()`, `serve_audited`, `link.rs` y `exception.rs` cargarían `&[]` sin leerlo, y un test congelado que construye `DecideInput` dejaría de compilar al hacer T002. Condiciones del Arquitecto: el tipo se escribe con la ruta completa (`&[gitraptor_policy::settings::model::Operation]`, porque `evaluate.rs` ya usa el `Operation` de la API); un test ejercita `decide_all` con `known_ops` no vacío (máximo de BR-CALC-001, nada pasa a `allow` por activar permisos y `Commit` decide igual que antes sin el caso especial); y la desviación se anota en la spec (T011) y en el PR. Con esto, `link.rs` y `exception.rs` no se tocan.
- **S2 · Las pruebas del contrato compilan contra el código de hoy.** Fallan en ejecución, no al compilar: el contrato de la API se comprueba por serde (`"mcp-only"`, `"full"`, `layer`, `mcp-allowlist`) y por el nombre de la capacidad, y el canal y la CLI por el JSON del cable. Las pruebas unitarias de la spec que necesitan símbolos nuevos (`catalog_operations`, `ProtectionState::of`, `WRITE_TOOLS`, `reasons` en `denied()`, `status_text_tests`, la TUI, `mcp_status_tests`) las escribe el experto junto al código y no son criterios.
- **S3 · Archivos de prueba del contrato**, todos nuevos: `crates/core/tests/us_grd_016_parity.rs` (T001, como dice la spec), `crates/api/tests/us_grd_016_contract.rs`, `crates/core/tests/us_grd_016_channel.rs` y `apps/cli/tests/guard_us_grd_016.rs` (T010). No se toca ningún archivo de pruebas congelado de otra historia.
- **S4 · Guardas en verde.** Las filas de paridad que ya pasan hoy (las cuatro operaciones sin plan, el mínimo seguro, ⛔3.1 «merge no es `unverifiable`», ⛔4.1 «un motivo por nivel»), el escenario 1 (sin herramienta de push y force-push denegado por Git) y el 5 (ninguna herramienta relaja ni decide) van en los mismos archivos como pruebas de regresión, no como criterios: hoy ya están en verde.
- **S5 · Arnés de actor independiente de la sesión de Claude Code.** Las e2e usan el agente falso `raptor-fake-agent` con `GITRAPTOR_AGENT_EXECUTABLES` (arnés de `apps/cli/tests/mcp_safe_commit.rs`) y perfil, HOME y repos temporales (NFR-01). El Claude Code real que lanza la suite no cuenta como agente. La suite e2e es `#![cfg(target_os = "macos")]` y `#![cfg(debug_assertions)]`, como `mcp_safe_commit.rs` (usa `script` y `proc_pidinfo`); Linux y Windows: pendiente de la etapa de validación multiplataforma (G2).
- **S7 · Ronda 1 del crítico (corregido en la spec y el contrato).** PC-1: la referencia de los hooks en la paridad es `serve_audited` con `Caller { permissions: true, chain: Some(Chain::new(..)) }` y la cadena real de cada operación, no `decide()` (su cadena vacía es `unverifiable`). PC-2: los hooks nunca ven `worktree-remove`; en borrar worktree la capa MCP es **más estricta** que la de hooks (NFR-02), y la paridad se fija solo sobre `branch-delete`. PC-3: la e2e es solo macOS (T010 y G2 lo dicen ya).
- **S6 · Alcance completo en un PR.** T001–T011; ningún tramo queda fuera. Coordinación: W112 (TS-CKP-001 PR-B) toca `crates/api`; si coincidimos en un archivo, integra el segundo en entrar.

## Convenciones observadas

Solo lo que la spec no cubre:

- Motor real sobre perfil y repo temporales: `crates/core/tests/us_mcp_009_permission.rs` (`rig`, `GuardrailsEngine::evaluate_detailed`, `GuardRegistry::for_profile`).
- Canal con capacidades: `crates/core/tests/ts_ckp_003_log_layers.rs` (precedente de `guard.log-layers`).
- E2e por `raptor-mcp` como Claude Code: `apps/cli/tests/mcp_safe_commit.rs`; allowlist del MCP: `apps/cli/tests/mcp_allowlist.rs`; instalación de hooks y espera de salud sin `sleep`: `apps/cli/tests/guard_us_grd_003.rs`.
- Textos: `apps/cli/build.rs` embebe `i18n/<lang>/*.txt`.
