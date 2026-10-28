# Evidencia · US-MCP-009 y US-MCP-010, `safe_commit` (2026-10-27)

## Veredicto

**SIN CERTIFICAR.** La corrida de `/nassa-core:implement` paró en P3: la suite agotó el tiempo límite de `deliver-run` con la máquina cargada por otros workers. Como parar después de P0 congela el contrato, no se relanzó, porque haría falta otra aprobación. La suite, la revisión, la seguridad y la conformidad se corrieron a mano, fuera del gate, con los resultados de abajo. No hay `certification.json`.

- Contrato: `docs/dev-briefs/us-mcp-009-010-safe-commit.contract.json`, 38 criterios, aprobado por el coordinador (2 veces: la segunda tras corregir los `tests` de CORPUS).
- **Base roja admitida** por el coordinador: `gitraptor-cli::tm_chaos::hostile_half_done_merge_rejects_until_aborted` falla de forma determinista en `b2bd6484` en este Mac (falla también en `main`). Inestables ajenas en la base: `daemon_tiers::edit_then_reset_hard_in_a_dormant_repo_is_recoverable`, `tm_calm_dormant::a_tm_operation_wakes_a_dormant_repo_before_settling`.

## Criterios

Los 38 en verde en macOS al cierre (pruebas de cada archivo de criterio y `grep` de documentos). La suite completa del workspace se corrió tras el rebase: ver la descripción del PR.

## Enmiendas de pruebas de criterio (todas con el turno del coordinador verificado por `amend-test`)

| Prueba | Motivo |
|---|---|
| `crates/core/tests/us_mcp_009.rs`, `us_mcp_009_failures.rs`, `us_mcp_010.rs`, `us_mcp_010_untracked.rs`, `us_mcp_010_shared.rs` | El rig usaba `NoProcs` (ningún `git` arrancaba) y commits del agente sin trailer y sobre `main`, que la política vigente deniega |
| `crates/core/tests/us_mcp_009_channel.rs` | Mensajes del agente sin trailer |
| `apps/cli/tests/mcp_safe_commit.rs` (2 veces) | Orden alfabético de claves, código de regla en vez del nombre de la política, trailer bajo `warn` (BR-AUTH-005); después, solo `rustfmt` |
| `apps/cli/tests/mcp_corpus/cases/safe-commit-path-dot-git-hfs.json` | U+200C es `invalid-path` por el orden de pasos (PC-7); pasa a U+206A |
| `apps/mcp/src/safe_commit_tests.rs` | `policy-denied` se mide contra su propio tope |
| `us_mcp_009_failures.rs`, `us_mcp_010_untracked.rs` (2.ª vez) | Límites de 700 ms y 2 s inestables bajo carga → 5 s |

Ninguna enmienda cambia una aserción de comportamiento.

## Revisión (5 lentes) y seguridad

- `correctness`, `testing`, `production` (sonnet), `architecture` (opus): sin Critical ni High. Aprobadas con comentarios.
- `security` = `/security-review` (`security-expert`, opus): **PASS**. A1–A25 en OK tras la ronda de arreglos (A11 y S-03 cerrados). Hay una sección explícita sobre las pruebas del descendiente del ejecutor y del cliente del hook: ninguno puede usar la ruta de escritura MCP con poderes de otro solicitante. Dos re-revisiones posteriores cubren cada cambio en superficie sensible.
- Ronda de arreglos: M-02 (el plan abre el git dir de la puerta), M-01 (cupos S-03, opción C), L-02, L-03.

## Conformidad

`docs/dev-briefs/us-mcp-009-010-safe-commit.conformance.md`. Debajo del escalón 2 solo quedan los documentos (aceptación = revisión), Linux/Windows (XP-47, XP-46) y el escenario 5 de US-MCP-009 sin e2e por stdio.

## Lo que no se verificó

- Linux y Windows en ejecución (XP-47, XP-46). Clippy cruzado de Windows limpio en `gitraptor-api`, `gitraptor-policy` y `gitraptor-git`; `gitraptor-core`, `gitraptor-cli` y `gitraptor-mcp` no compilan en cruzado desde este Mac (`libsqlite3-sys` necesita el compilador C de MSVC).
- La comprobación de citas no encuentra ADR-MCP-001, ADR-CKP-002, US-MCP-007 ni US-MCP-011 en la memoria indexada, aunque los archivos existen: el índice está desactualizado.
