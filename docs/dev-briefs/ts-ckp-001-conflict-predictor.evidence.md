# Evidencia · TS-CKP-001 PR-A (2026-10-21)

Corrida `/nassa-core:implement --autonomous TS-CKP-001` (rigor alto). Brief: `ts-ckp-001-conflict-predictor.md`. Contrato: `ts-ckp-001-conflict-predictor.contract.json`. Aprobación del plan: «apruebo el plan» en la terminal del worker (sha `e11bcd74…:f749eec4…:c2920ebf…`).

## Veredicto: NO CERTIFICADA (la corrida paró en P3)

- **Criterios:** los 29 en verde (25 de runtime y 4 estáticos), cada uno con su `verify`. El checkpoint de parada da «criterios pasados 29/29 · no certificado». No guardó el árbol: «el árbol no se pudo guardar».
- **P0:** baseline verde con la suite acotada. Los 29 criterios fallaban antes de la implementación.
- **P2:** `cargo fmt --all --check` y `cargo clippy --workspace --all-targets -- -D warnings`, en verde.
- **P3:** paró por `round-budget` con PARA («regresión: 3 tests que no fallaban ahora fallan»).
  - P3 **no corre el `suite.command` fijado en el contrato**: vuelve a detectar las capas (`deliver-run.mjs:343`, `detectLayers`) y corre `cargo nextest run --workspace` entero, sin el adaptador.
  - En las tres pasadas, los rojos salen de `gitraptor-cli`, fuera del contrato y del alcance. Cada pasada da un conjunto distinto, todos pasan solos y varios ya estaban registrados como inestables (`bitacora/orquestador/specs/w85-flaky-batch2.md`).

| Pasada | Rojos (todos en `gitraptor-cli`) | Solos |
|---|---|---|
| Baseline completa (carga 74) | `claude_sessions::a_claude_code_session_is_detected_without_registering_it`, `guard_td_grd_001_upgrade` ×2 (timeout de 240 s), `guard_ts_ckp_003::an_atomic_rebase_that_conflicts_is_evaluated_once`, `guard_us_grd_001::…hostile_environment…`, `tm_chaos::hostile_half_done_merge_rejects_until_aborted` | 6/6 pasan |
| P3, ronda 0 | `claude_sessions::…`, `us_tmc_005_hook::scenario_1_rebase_entry_shows_hook_prior` | 2/2 pasan |
| P3, ronda 1 | `raw_git_undo::an_agent_recovers_its_work_after_a_raw_reset_hard`, `us_tmc_005_hook::scenario_1…` | 1 de 4 pasadas solas falla `raw_git_undo` (inestable también solo) |
| P3, ronda 2 | `guard_td_grd_001_upgrade` ×2, `mcp_snapshot::an_empty_label_is_invalid_text` | 3/3 pasan |

## Enmienda de prueba

`crates/git/tests/ts_ckp_001_merge_bases.rs:14`: `&format!("{file}")` → `file` (`clippy::useless_format`). Aprobada en la terminal: «apruebo la enmienda de crates/git/tests/ts_ckp_001_merge_bases.rs». La aserción no cambia.

## Revisión (P5)

Plan de lentes: correctness, testing, production, performance (sonnet) y architecture (opus). Todas dieron APPROVED WITH COMMENTS, sin Critical ni High. `/security-review` (security-expert, opus) dio APROBADO CON CONDICIONES, también sin Critical ni High.

- **Arreglado en esta corrida:** M-03 de seguridad y M1 de arquitectura, el mismo hallazgo. La excepción G9 de `static_check.rs` saltaba los 15 patrones y la activaba un comentario. Ahora exime solo `write_buf`/`write_stream`, y solo si el fichero contiene `.with_object_memory()`. La re-revisión del security-expert lo da por cerrado.
- **Condición antes de PR-B** (seguridad; el pool ejecuta merges en seco sobre repos del usuario):
  - M-01: tope por tamaño también para árboles y commits.
  - M-02: deadline y tope de merge-bases en el merge en seco.
  - L-02: deadline en `merge_bases`.
- **Pendientes Medium:**
  - Correctness M1: `complete` deja `cached` con la triple rechazada y el par puede quedar `Calculating` para siempre.
  - Correctness M2: un worktree ilegible llega como «sin trabajo sin commitear»; debería ser `partial`.
  - Correctness M3: un `merge_bases` que falla para un par corta el plan del repo.
  - Production M1: un `Recomputed` tardío recrea el estado de un repo retirado o dormido.
  - Production M2 y performance M2: cada recomputo relee todo el repo aunque no se haya movido ninguna punta.
  - Production M3: un pánico borra el estado de todos los repos.
  - Performance M1: clon profundo de `UncommittedPaths` por worktree.
  - Arquitectura M2: el módulo planifica contra una base no confirmada; la base confirmada vive en `daemon/mod.rs`.
  - Testing M: `PLAN_READ_BUDGET` sin prueba.
- **Pendientes Low:** `syn_corpus::command` duplica `Fixture::git_command`; `open_for_dry_merge` duplica el cuerpo de `open`; `session_present` siempre vale falso; el registro no identifica el repo; la parada espera al lote; `conflicts::extract` es cuadrático; las rutas de conflicto van sin sanear (SEC-12, en PR-B); faltan canarios de `filter`/`textconv`/`refs/replace` (Info).

## Verificación fuera de la corrida

- **Clippy cruzado de Windows:** `cargo clippy --target x86_64-pc-windows-msvc -p gitraptor-git --all-targets -- -D warnings` y `cargo check --target x86_64-pc-windows-msvc -p gitraptor-testkit` en verde. `gitraptor-core` no compila de forma cruzada en este Mac (`libsqlite3-sys` necesita las cabeceras C de Windows): lo cubre el job de Windows del CI.
- **Linux y Windows:** no verificados aquí (etapa de validación multiplataforma).
