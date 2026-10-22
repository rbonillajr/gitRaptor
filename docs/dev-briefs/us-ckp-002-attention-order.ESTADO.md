# Estado de US-CKP-002 (2026-10-21) — pausada antes de construir

Rama `feat/US-CKP-002-attention-order`, desde `origin/main` tras el PR #282 (US-CKP-003). Sin PR: la ronda se cerró por orden de Rene antes de construir.

## Hecho

- Dev Spec `docs/requirements/features/cockpit/dev-specs/US-CKP-002-orden-atencion-terminadas.md` (spec-ready) con ajustes del plan-critic: texto del caso "sin observar" (visible a las 2 h, "último agente" a las 24 h), ruta real de `no_action_targets_a_row` (`apps/cli/tests/tui_engine_states.rs`, cierra G7) y `tests/tui_fleet_order.rs` en T002 y en el árbol.
- Contrato `docs/dev-briefs/us-ckp-002-attention-order.contract.json`: 13 criterios (10 de pantalla por la API pública en `apps/cli/tests/tui_fleet_order.rs` y 3 PTY de macOS en `apps/cli/tests/live_fleet.rs`). El criterion-tester los dejó rojos por aserción y los probó con su `verify`. Los de color (NINE, LAGGED, FAILEDLIST) están hechos para fallar con el color posicional de hoy.
- Plan-critic (opus) en dos rondas, la segunda APROBABLE (`docs/dev-briefs/us-ckp-002-attention-order.critic.json`). Solicitud de aprobación escrita: `docs/dev-briefs/US-CKP-002-orden-atencion-terminadas.approval.md`.
- El plan se envió al coordinador, que pidió cerrar la ronda antes de la firma. **Nada está firmado ni medido (sin `deliver-run start`, sin baseline).**

## Siguiente paso, para retomar

1. Estar en este worktree y rama; `git fetch origin main` y rebasar si `main` avanzó (suite completa tras el rebase).
2. Si el plan, el contrato o los tests cambiaron, repetir `plan-approval request` (la solicitud del run state no sobrevive a otra sesión). Pasar siempre `--session $CLAUDE_CODE_SESSION_ID` a `plan-critic`, `plan-approval` y `deliver-run` (hay otra sesión con una corrida vieja en el árbol).
3. Pedir la firma: el coordinador escribe en la terminal del worker `apruebo el plan`. Después `deliver-run start`, `deliver-contract baseline --write`. Si B1 sale rojo por tests ajenos inestables (pasó cuatro veces en US-CKP-003, máquina cargada), `deliver-run abort --reason …` y seguir SIN CERTIFICAR, como ordenó el coordinador.
4. Construir con `nassa-dev-rust:rust-expert` (sonnet), un tramo, T001 y T003 a T006; luego revisión opus; T007 docs al final: historia `partially-implemented` (D12: faltan la subida por hueco, conflicto y denegación), fila en `user-stories.md`, `backlog.md`, `release-plan.md`, enmienda de DS-US-CKP-001 § 7 (F1 → D10), `node tools/status/release-status.mjs`.
5. Notas del experto, aprendidas en US-CKP-003: no puede editar los tests del contrato (si uno es incorrecto, devuelve BLOQUEO); `T004` paso 6 debe clasificar `Action::ShowEnded` en `no_action_targets_a_row`; añadir el campo a `AgentRowModel`/`SessionRow`/`Ui` rompe los literales de `view.rs`, `gallery/stories.rs` y `tests/tui_screens.rs`; las pruebas con texto fijan el modo de símbolos y los snapshots van sin color.
6. Cierre: fmt, clippy workspace, `NEXTEST_TEST_THREADS=2 node tools/test/nextest-junit.mjs` completo, push (con lease si es rebase, escrito directo), PR con "Prueba de plugins", `gh pr merge <n> --auto --rebase`.

## Pendiente fuera de esta historia

La subida de filas por hueco de observación (US-GRP-005), por conflicto previsto (US-CKP-006) y por denegación (US-CKP-019), y la marca "compartido" del motor (US-GRP-011).
