# Evidencia · US-GRD-014 (2026-10-21)

- **Veredicto:** CERTIFIED WITH RESERVATIONS · 25/25 criterios en PASS (`docs/dev-briefs/us-grd-014-team-base-branch.certification.json`).
- **contract_sha256:** `6b57f87e8111dfb12973e3ef4a0a629f8dccbffc7cb6fe6fd43f7414e59aa355`.
- **Plan:** `docs/requirements/features/guardrails/dev-specs/US-GRD-014-rama-base-del-equipo.md` (2 re-planes: PC-4 en D3 antes del arranque y la enmienda de D4 por L-01).

## Reservas

1. **Base roja admitida** (firma del coordinador en su turno). En la suite completa fallan tests ajenos a esta historia que ya fallaban antes de cualquier cambio. La primera baseline congelada se rechazó porque dos tests inestables (`guard_td_grd_001_upgrade`) pasaron en `origin/main` con la máquina a carga ~61; se volvió a medir con carga ~11 y se admitió. Tras el rebase, la suite completa da 2480/2483. Las 3 que fallan son `raw_git_undo::an_agent_recovers_its_work_after_a_raw_reset_hard` (id de snapshot de la Time Machine), `guard_td_grd_001_upgrade::repo_intact_an_upgrade_from_1_…` y `us_tmc_005_hook::scenario_1_rebase_entry_shows_hook_prior`. Las tres estaban en la baseline congelada antes del cambio y tocan zonas que esta rama no modifica.
2. **Integridad de pruebas no comprobada.** P3 corrió `cargo nextest run` directo, no el adaptador del contrato, y no registró `junit-paths.xml` (fricción del plugin).
3. **Sin lente después de la ronda 2.** La última escritura (enmienda de D4 y tests de `unshowable`/`expired`) la revisó la `/security-review` delta (PASS), no una lente de código.

## Revisión (veredictos tal cual)

| Lente | Modelo | Veredicto | Hallazgos |
|---|---|---|---|
| security | opus | APPROVED WITH COMMENTS | M: `take_due` sin tipo (arreglado) · L: `guard.confirm` borra una ventana de desinstalación sin auditar (arreglado) |
| architecture | opus | CHANGES REQUIRED → re-revisión APPROVED WITH COMMENTS | H-1 `changed` a clientes antiguos (arreglado) · L-1 docs (arreglado) · L-2 `RelaxedKey` en tres sitios (pendiente) · L nuevo: la capacidad de cada outcome se decide en tres sitios (pendiente) |
| correctness | sonnet | APPROVED WITH COMMENTS | M1 `take_due` (arreglado) · M2 rechazo rotulado `changed` con texto del cliente (arreglado) |
| testing | sonnet | APPROVED WITH COMMENTS | M migración sin test (añadido) · M degradado y fail-safe sin test aislado (pendiente parcial) · L cobertura unitaria |

## Seguridad

- `/security-review` (security-expert, opus): **PASS WITH FINDINGS**; 0 C/H/M. L-01 (confirmar `main` con principal `master` quitaba protección sin ventana): el Arquitecto enmendó D4 y se implementó.
- Revisión delta final: **PASS**; L-01 cerrado. Info: un build release con `debug-assertions` respeta `GITRAPTOR_TEST_GUARD_WINDOW_MS`/`_GRACE_MS`; conviene un check de CI sobre el perfil de distribución.
- Pendientes Info: I-01 doble `check_reserved` en `reserved_quiet` (sin impacto) · I-02 `commit`/`floor_blob` como `String` · I-03 el degradado protege una base sin confirmar (solo más estricto).

## Conformidad

Todas las cláusulas de la historia y D1–D12 tienen tarea dueña. Los escenarios 1–6 llegan al escalón 3 (e2e con `raptor`, `raptor-hook` y Git reales); D9 y la capacidad, al 2. Por debajo del escalón 2: la unión de respaldo con `GuardEntry.bases` (D6) aislada, la precedencia `Unconfirmed`/`Invalid` sobre `Pending` en el MCP (D9) y la exclusión del KPI (D10). Sobra: nada «no pedido»; la migración, el filtro del bus, `team_status.rs`, `guard/confirm.rs` y la salida con error tras `NothingToConfirm` están justificados o son colaterales.

## No verificado

- Windows y Linux: solo macOS. `cargo clippy --target x86_64-pc-windows-msvc` pasa en `policy` y `api`; `core` y `cli` no compilan aquí para MSVC (`libsqlite3-sys` no encuentra `stdlib.h`). Queda para el CI y las máquinas reales.
- Repos «Solo MCP»: sin diagnósticos ni pista de confirmar hasta US-GRP-016 (§ 8 de la spec).
