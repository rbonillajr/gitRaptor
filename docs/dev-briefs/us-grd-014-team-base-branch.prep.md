# Preparación de la corrida · US-GRD-014 (2026-10-21)

Sin diseño: `docs/requirements/features/guardrails/dev-specs/US-GRD-014-rama-base-del-equipo.md` ya es el plan (`spec-ready`). La preparó el loop principal. Esta nota recoge solo lo que la preparación encontró y lo que fija para el implementador.

## Deriva de citas

Las 21 citas de `must_read` y de «Lo que falta» siguen ciertas en el código de hoy (`team.rs` `guarded_base_branches`/`relaxes`/`combine`, `team_baseline.rs`, `evaluate.rs` `serve_audited`, `install.rs` `bases`/`status`/`publish`/`recover`, `hook.rs` `degraded`, `health.rs` `BaseUnconfirmed`, `GuardAction`, `Diagnostic`, `PendingKind::Uninstall`, `audit_pending` con `GUARD_UNINSTALL`, `pending::Entry`, `WINDOW_ENV`). Sin bloqueos.

## Ajustes a la spec (no cambian ninguna decisión)

- T008 citaba `US-MCP-004-quien-mas-trabaja.md`, que no existe: la ficha es `US-MCP-004-status-completo-y-no-disponible.md` (crítico, PC-1).
- § 6 declara dónde viven los tests que certifican (crítico, PC-3).

## Supuestos (reversibles) que fijan los tests del contrato

- **Rama base aparte de las relajaciones.** D3 nombra `engine.baseBranch` en la lista cerrada; § 3 la muestra separada de «Relajaciones (N)». Los tests siguen § 3: el cambio de base va en `base.from → base.to` y cuenta para la ventana (D4), y `relaxations` lleva solo `permissions.*` y `policies.*`.
- **API pública de `team::confirm`** (cabecera de `crates/policy/tests/us_grd_014_confirm.rs`): `plan(&TeamConfig, Option<&Confirmed>) -> Result<ConfirmPlan, Refusal>`; `ConfirmPlan { base: { from: Option<RefName>, to: RefName }, relaxations: Vec<Relaxation { key, before, after }>, fingerprint: Fingerprint }` y `needs_window()`; `RelaxedKey::as_str()` (`"permissions.allow"`, …); `Refusal::{NothingToConfirm, BaseInvalid, FloorUnreadable, UnshowableRelaxation}`. Sin confirmación, `from = Some(main)`.
- **Nombres en el cable** (kebab-case): `confirm.action = "guard-confirm"`, `PendingKind` `confirm-team`, diagnósticos `base-change-pending`, `base-change-pending-invalid`, `floor-relax-pending`; auditoría `guard.confirm` con `applied`/`cancelled`/`expired`/`changed`; capacidad `guard.team-confirmation`.
- **Textos del CLI** que comprueban los tests: el plan antes de `[y/N]`, «Relaxations (N)», `disableSafeMinimum` en el plan, «Ctrl-C» en la ventana, «cancelled» al cancelar y «only the developer can change the protection» para el agente.
- **La segunda comparación de la huella** (al aplicar tras la ventana) no se ejercita e2e: dependería del tiempo. La cubren `FP-CHANGED` (entre el plan y el sí), `FP-CONCURRENT` (otro cliente confirma primero) y `POL-FINGERPRINT`; el revisor comprueba que el daemon compara también al aplicar.
- **Un solo experto**: `brief-slices` no encuentra topología agrupada y los tramos comparten `target/`.
