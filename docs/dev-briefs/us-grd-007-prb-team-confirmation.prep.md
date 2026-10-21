# Preparación de la corrida · US-GRD-007 PR-B (2026-10-21)

Sin diseño: `docs/requirements/features/guardrails/dev-specs/US-GRD-007-permisos-por-operacion.md` (D15, D16, T010 y T011) ya es el plan (`spec-ready`). La preparó el loop principal. Alcance: **solo PR-B**, es decir, los escenarios 4 y 5 de extremo a extremo con `raptor guard confirm` de US-GRD-014 (#277, en `main`), y el cierre de la ficha.

## Deriva de citas

- D15: `guard.confirm`, `PendingKind::ConfirmTeam`, `floor-relax-pending` y la pista `guard-confirm` existen en `main` (`crates/core/src/guardrails/team_status.rs`, `crates/api/src/guard.rs`). El plan de 014 lista `permissions.*` en la lista cerrada (DS-US-GRD-014 D3, ajustada en #277: `engine.baseBranch` va aparte).
- D4: `minimum_set` con `disabled-by-team` existe en `GuardStatus` (`crates/api/src/guard.rs`).
- La suite e2e `apps/cli/tests/guard_us_grd_007.rs` (PR-A) tiene `Machine::new/build` con el suelo del equipo; los tests de PR-B van en el mismo archivo (T010).

## Resultado: el comportamiento ya existe

El criterion-tester escribió los dos tests (`confirming_a_floor_that_disables_the_minimum_lets_force_push_through` y `a_team_relaxation_waits_for_the_developer`) y los dos pasan en verde con el `main` de hoy, sin código de producción. US-GRD-014 (#277) y PR-A (#268) ya daban el comportamiento. La línea base habría parado en B3 («un `verify` ya pasa»).

**Decisión del orquestador (2026-10-21):** PR-B se cierra por el flujo directo (tests y documentos), sin arrancar `deliver-run`: no hay código que construir ni contrato que certificar. El contrato y la ronda del crítico quedan como registro de la parada. Los dos tests quedan como regresión de los escenarios 4 y 5, revisados por la lente de pruebas y por `/security-review`.

## Supuestos (reversibles)

- Si los dos escenarios ya pasan con `main` (014 y PR-A juntos pueden bastar), no hace falta código de producción: B3 lo dirá en la línea base y PR-B queda en tests y documentación. Ese hallazgo va a la parada.
- El rechazo al agente se comprueba en la auditoría de comandos reservados de 014 (`guard.confirm`, `rejected`), sin redefinir nada de su contrato (D15).
- El cierre de T011 (ficha a `implemented`, backlog, `release-status.md` regenerado) no tiene criterio en el contrato: lo revisan la conformidad y el coordinador antes del merge (crítico PC-1).
- Las enmiendas de § 9 de la spec ya están en `main` (ADR-GRD-003 ya tiene `ask-unavailable`). PR-B no toca ADRs (crítico PC-2).
- T011 también actualiza G1 y § 10 de la Dev Spec, que aún dicen que `raptor guard confirm` no está en `main` (crítico PC-3).
- Un solo experto, si hace falta: el cambio es de un archivo de tests y, como mucho, un ajuste puntual.
