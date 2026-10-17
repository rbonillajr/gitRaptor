# ESTADO — estabilizar tests inestables (pausa 2026-10-16)

Rama de trabajo: `fix/flaky-tests-under-load` (6 commits sobre origin/main b9a2818a, sin push ni PR). Este WIP: `wip/flaky-2026-10-16`.

## Hecho
- fsevents (2 tests): carrera del test, 11/30 → 0/30. Commit d0fbe418.
- channel flood: test paceado + señal del límite; **fix de producto** `crates/api` (Client perdía la parte de una línea en un read timeout; `read_frame_resumable`). Tests deterministas incluidos.
- live_changes: quitada la aserción ENGINE_BUDGET_MS (gate lo hace el bench `engine`); antes 3/15 fallos con carga.
- timeline_process: `held_commit` (hook retiene git hasta que el motor registró el commit).
- autostart: `fake_tool` se ejecuta una vez antes (hipótesis, no reproducida).
- TUI `answering_yes…`: ya en PR #252 (rama `fix/tui-unobserved-screen-wait`, auto-merge). Su commit 7b68a983 NO está en esta rama.

## Pendiente (siguiente paso exacto)
1. `git fetch && git rebase origin/main` (si #252 ya mergeó no hay conflicto).
2. Verificar: `scratchpad/suite.sh` hacía `cargo test --workspace --no-fail-fast -- --skip repo_intact --skip a_stop_without_a_controlling_terminal` con 30 bucles de CPU, 3 veces; la corrida 1 salió en verde (exit 0), la 2 y 3 no terminaron (interrumpidas). Repetir 2 corridas más SIN cambiar de rama durante la prueba. Cuidado: no hacer `git checkout` mientras corre.
3. `cargo fmt --all --check` y `cargo clippy --all-targets -- -D warnings` (clippy ya estaba verde antes del último rebase).
4. Esperar el informe del revisor Opus (agente lanzado sobre el diff de `crates/api` y `channel.rs`; si se perdió, relanzar `nassa-dev-rust:rust-code-reviewer` con modelo opus) y aplicar hallazgos.
5. Push `git push -u origin fix/flaky-tests-under-load`, PR con el cuerpo ya redactado en el scratchpad (`pr2.md`; reemplazar RESULTADO_SUITE) y `gh pr merge <n> --auto --rebase`. Resumen del cuerpo: tabla test/causa/arreglo/antes→después; carrera de producto del cliente; honestidad: channel, timeline_process y autostart NO reproducidos en local.
6. Reportar al coordinador con `orca orchestration send --type worker_done`.

## Anotar en el PR / aparte
- `us_tmc_009` es inestable de verdad (2 fallos distintos: `restoring_over_an_agents_work…` «operation failed» y `a_restore_can_be_undone…` git-event-3). Probable carrera del observador vs. restore; `undo` por operation_id da -32004. Es un PR aparte (nuevo trabajo).
- `channel_process::a_stop_without_a_controlling_terminal_is_refused` falla solo en mi arnés en segundo plano (`identity-unverified`); no investigado.
