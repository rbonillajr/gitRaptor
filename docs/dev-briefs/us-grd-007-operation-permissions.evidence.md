# Evidencia · US-GRD-007 PR-A, permisos por operación

- **Plan**: [DS-US-GRD-007](../requirements/features/guardrails/dev-specs/US-GRD-007-permisos-por-operacion.md), preparado en el loop principal sin rediseño, más la enmienda **D19** (Q-GRD-46: `ask` solo deniega a los agentes), validada por el Arquitecto y el PO.
- **Contrato**: [us-grd-007-operation-permissions.contract.json](us-grd-007-operation-permissions.contract.json), 20 criterios. `contract_sha256` certificado: `d4819b9950c79338a774e9400d8318f677c4d96f6f02c1a7c4ccc79d34d7b1ad`.
- **Aprobación**: el 2026-10-21, por el coordinador, en un turno propio («apruebo el plan»). El crítico aprobó en 2 rondas.
- **Certificación**: [us-grd-007-operation-permissions.certification.json](us-grd-007-operation-permissions.certification.json).

## Certificación

**20/20 criterios PASS** · veredicto **CERTIFIED WITH RESERVATIONS**.

| Reserva | Motivo |
|---|---|
| `integridad-sin-verificar` | P3 corrió `cargo nextest run --workspace --profile ci` y no `suite.command`, así que no se registró el informe por archivo (`junit-paths.xml`). Le pasó igual a TS-CKP-003 |
| `test-enmendado` (×2) | `crates/policy/tests/us_grd_007_permissions.rs`: (1) los ayudantes `only()` y `all_allow()` declaran `disableSafeMinimum`, coherente con `ctx.minimum = false` (D4). Antes pedían `Ask` y `Allow` a la vez para la misma entrada. (2) `#[cfg(unix)]` en el caso de argv no UTF-8, para que compile en Windows. Las dos las aprobó el coordinador en su turno; ninguna cambia un asserto |

| `revision-tras-arreglo-sin-correr` | La segunda enmienda del test (`#[cfg(unix)]`) se escribió después de la última re-revisión de seguridad; no volvió a pasar ninguna lente sobre ese cambio de 3 líneas |

Aviso `fuera-del-plan`: `context.md` y `business-rules.md` de guardrails. Q-GRD-39 queda ratificada por Rene (2026-10-20) y BR-VAL-002 recibe la enmienda Q-GRD-46, a petición del coordinador.

## Verificación

- **Línea base**:
  - Tres mediciones bajo la carga de otros workers (carga ~17) fallaron cada vez con un test inestable distinto, ajeno al contrato: `guard_td_grd_001_upgrade` (timeouts de 240 s), `mcp_security_corpus`, `raw_git_undo`, `us_tmc_005_hook` y `guard_us_grd_004_events`. Todos pasan aislados.
  - La cuarta, con `NEXTEST_TEST_THREADS=2`, salió sana y es una medición real. El registro manual (`--record`) que firmó el coordinador no se usó. El modo `--record` mide la capa estática con los tests del contrato presentes; ver *Prueba de plugins*.
- **P2 y P3**: clippy del workspace y suite completa en verde tras la ronda de arreglos.
- **Tras el rebase sobre `main`** (9cae89b1): `cargo clippy --workspace --all-targets -- -D warnings` limpio, `cargo fmt --all --check` limpio y suite completa (`node tools/test/nextest-junit.mjs`, 4 hilos) **2349 passed, 0 failed** (macOS).
- **Tras el segundo rebase** (con #267, US-MCP-013): fmt, clippy del workspace y clippy cruzado de Windows limpios; suite completa **2364 passed, 0 failed** (macOS). El contrato con `plan_gate::view_of` es el mismo (`Ask` si y solo si `effect == ask && applied_effect == deny`); los planes del ejecutor siguen sin permisos (`RuleSet::all()`), así que el `Ask` real del ejecutor llega con US-MCP-009 / US-GRD-016 (D19 c).
- **Windows**: `cargo clippy --target x86_64-pc-windows-msvc -p gitraptor-policy -p gitraptor-git -p gitraptor-api --all-targets -- -D warnings` limpio, tras una segunda enmienda aprobada del test de policy (`#[cfg(unix)]` en el caso de argv no UTF-8, que no compilaba en Windows). `gitraptor-core` y `gitraptor-cli` no se compilan en cruzado desde el Mac, porque sqlite necesita el compilador C de MSVC. No hay máquina Windows: los tests e2e son `cfg(unix)` y el CI de Windows es la primera ejecución real.

## Revisión (4 lentes, sin Critical; un High arreglado)

| Lente | Modelo | Veredicto | Hallazgos |
|---|---|---|---|
| security (`/security-review`) | opus | CHANGES REQUIRED → **APPROVED WITH COMMENTS** tras la ronda 1 | **H1** (un actor no verificado pasaba un `ask` como persona): **arreglado** con `actor::resolve_for_ask` (agente, persona, no demostrado). **M2** (nivel de equipo ilegible con la capacidad: se dejaban de aplicar los permisos): **arreglado** con `team_only`, igual que el modo degradado. **M3** (`git fetch . <sha>:refs/heads/x` esquiva `commit`): residuo declarado. **L4** (`pre-commit` deja pasar si falla la llamada al daemon; lo frena `reference-transaction`): residuo |
| correctness | sonnet | APPROVED WITH COMMENTS | L1 (la cadena de `git` se lee aunque todo esté en `allow`, desviación D14): declarado. Observación: renombrar una rama local con `commit` en `deny` puede denegarse de más: residuo |
| testing | sonnet | APPROVED WITH COMMENTS | M1 (rama de error de `team_only` sin test): la cubre en parte `us_grd_007_actor.rs::an_unloadable_team_level_keeps_the_degraded_guarantee`. L2 y L3 (valor viejo `Zero` y atajos de `classified` sin test directo): pendientes |
| architecture | opus | APPROVED WITH COMMENTS | L1 (comentario de documentación desplazado) y L2 (lógica de permisos en `conn.rs`): **arreglados**. L3 (el cliente degradado carga las capas dos veces): pendiente |

Regresiones encontradas por P3 y arregladas en la ronda 1: `guard_us_grd_004_events::a_client_without_the_capability_gets_none_of_it` (`legacy_log` borraba también las entradas sin razones) y `static_check::gix_writes_only_in_store_writer` (el nombre `new_commits` coincidía con el patrón prohibido).

## Conformidad (resumen)

- **Escalón 3** (Git, `raptor` y `raptor-hook` reales): escenarios 1 (filas de PR-A), 2, 3 (agente y persona) y 6; D5, D8, D9, D12, D17 y D19.
- **Escalón 2**: D1, D2, D4, D6, D7, D10, D11 y D18.
- **Por debajo del escalón 2**:
  - `permission-reach` (D9/D18): está publicado, pero ningún test lo nombra.
  - D13: la vía rápida no cambia, y no hay test.
  - D14: la medición de coste (`permissions_cost`) **no se hizo**.
- **Fuera de PR-A**: escenarios 4 y 5, D15 y D16 son de PR-B (US-GRD-014). Las filas `reset --hard`, borrar worktree, crear worktree sin rama nueva y merge completo son de US-GRD-016.
- **Sobra**: nada «no pedido». Hay un cambio colateral en `crates/policy/src/team.rs`: un worktree con el mismo blob que el suelo no cuenta como otro nivel. Está documentado en el § 10 del Dev Spec.
