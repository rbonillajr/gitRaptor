# Evidencia de entrega — US-TMC-005

## Qué cambia

Implementa **US-TMC-005**. Un rebase o un borrado de rama local con Git crudo, en un repo con los hooks de Guardrails, tiene ahora un punto `previo_hook` tomado **antes** de que Git toque nada. Esta historia desbloquea **US-GRD-017**.

- **Un solo punto de entrada (D1).** El daemon toma el snapshot dentro de `guard.evaluate`, después de decidir `allow` y antes de escribir la entrada del registro de decisiones.
  - Lo hace solo si la conexión tiene la capacidad nueva `guard.prior-snapshot`. Responde con `Decision.priorSnapshot` = `complete` o `failed` (con causa).
  - No hay un subcomando nuevo. US-GRD-017 solo tiene que convertir `failed` en `deny snapshot-failed` en ese mismo punto (ADR-GRD-003 § 7).
- **Disparadores (D2).** Solo `pre-rebase` y `reference-transaction prepared` con un borrado de `refs/heads/*`. Nunca push, commits, consultas ni movimientos de rama.
- **Un snapshot por comando (D3).** Uno solo por cada comando de Git (el `git` más cercano), y sin recursión.
- **Plazo (D5).** 5 s. Un snapshot tardío se descarta y nunca queda `complete`. El reloj y el plazo son inyectables.
- **Cupo SEC-TMC-12 (D6), en el oplog.**
  - Cifras: 10 por minuto y 120 en 24 h por solicitante y worktree; 300 en 24 h por worktree y 1.000 en 24 h por repo. Van marcadas como ⚠️ ASSUMPTION.
  - Los cupos globales solo cuentan a los agentes (**Q-GRD-37**).
  - El cupo no consume la reserva del previo garantizado (D7).
- **Origen seguro.** El worktree y el repo los resuelve el daemon: el registro, más el cwd del proceso del hook leído por el daemon. Pasan por la misma puerta del `.git` que #226 (`tm_scope_for` y `open_registered_worktree`). Nunca se usan rutas que mande el hook.
- **Migración del oplog.** Se añade al final de la lista con la línea marcadora `-- migration: hook-prior requester`, así que se puede renumerar. Los `CHECK` nuevos exigen solicitante y canal `hook` en las filas `hook-prior`.
- **Timeline (D10).** Con la misma marca del motor, gana el previo.
- **Hook (D11).** Con `failed` avisa por stderr con la causa (i18n en/es) y deja pasar la operación. Con `complete` no escribe nada.
- **Documentos.**
  - Enmiendas de ADR-TMC-004 § 3 y de ADR-GRD-003.
  - La historia se reescribe según el PO: título nuevo, escenario 1 con el borrado de una rama, un escenario de rebase nuevo, y R2 parcialmente abierto para `checkout -f`/`restore`/`reset --hard`, que siguen en observación.
  - BR-TMC-CONS-003 incluye los cupos.
  - Ficha de deuda **TD-TMC-001**, más el Brief y el contrato.

## Decisiones (registradas en el Brief y en la historia)

- **Escenario 1 reescrito.** Ningún hook de Guardrails corre antes de que un `checkout` cambie el working tree, y Git ya se niega a un checkout o un rebase que sobrescriba ediciones sin commitear. *Decisión del orquestador (2026-10-04), validada por Arquitecto/PO.*
- **D1–D16** del Brief: *validadas por el Arquitecto (rust-architect) y el PO*, y aprobadas por el coordinador con tres condiciones, todas cumplidas:
  1. migración renumerable;
  2. la puerta del `.git` de #226, con un test de gitdir hacia otro repo (`TMC005-UNTRUSTED`);
  3. plazo y cupo probados sin `sleep`.
- **Q-GRD-37** (DS-US-GRD-017, PR #242). Se incorpora aquí, así que la tarea T008 de esa Dev Spec ya no hace falta.

## Cómo se verificó (macOS)

- **Contrato** `docs/dev-briefs/us-tmc-005-pre-hook-snapshot.contract.json`, con 9 criterios. La línea base salió sana: todos fallaban antes de implementar. Hoy pasan los 9: S1, S3, S4, TRIGGERS, TIMEOUT, QUOTA, NORECURSION, UNTRUSTED y MIGRATION. El escenario 2 ya lo cubrían los tests de US-TMC-004.
- **Gates del run** (`deliver-run`): P2 en verde (`cargo fmt --all --check` y `cargo clippy --workspace --all-targets -- -D warnings`). P3 en verde (`cargo nextest run --workspace --profile ci`). P4 no aplica.
- **Certificación**: **CERTIFIED WITH RESERVATIONS**, 9/9 criterios PASS, `contract_sha256: cb06fe007532b8a1262ab5238f975804e8b3dec68f3acf163c42ec9b9ecdd350` (`docs/dev-briefs/us-tmc-005-pre-hook-snapshot.certification.json`). Reservas: `baseline-roja-explicada` (rojo de la línea base explicado solo por los tests del contrato) e `integridad-sin-verificar` (P3 no escribió el informe por archivo; ver Prueba de plugins).
- **Test extra fuera del contrato**: `crates/core/tests/us_tmc_005_unattributed.rs` (Q-GRD-37).
- **Linux y Windows**: solo CI y clippy cruzado. No se probó en máquinas reales.

## Revisión

- **rust-code-reviewer (opus)**: APPROVED WITH COMMENTS (C:0 H:0 M:1 L:3). No encontró ningún camino que rompa NFR-01.
- **security-expert (opus, `/security-review`)**: PASS WITH FINDINGS (C:0 H:0 M:3 L:2 I:3). Comprobó que:
  - ningún path ni repo id del cliente se usa como fuente;
  - la captura no ejecuta hooks, filtros ni configuración del repo;
  - un fallo nunca se presenta como protegido;
  - el SQL va parametrizado.
- **Medium y Low → TD-TMC-001** (`draft`). Hay que cerrarla antes o dentro de US-GRD-017, porque allí un `failed` deniega:
  - **M-01**: el plazo de 5 s no acota la espera del writer ni la del oplog.
  - **M-02**: el cupo se comprueba tarde, así que hace falta un precheck.
  - **M-03**: el cubo "sin atribuir" no tiene techo por repo, su clave depende del inode y en Windows no hay suelo de disco.
  - **L-01** y **L-02**, y los Low del revisor.
  - Siguiendo la regla de la cadena, no se hizo ronda de arreglos porque no hubo Critical ni High.

## Pendiente

- TD-TMC-001 (ver arriba).
- Las cifras de cupo son ⚠️ ASSUMPTION y hay que revisarlas con uso real.
- Si otra rama (W59, `feat/US-TMC-019-interruption-robustness`) añade una migración del oplog, la que entre segundo renumera su migración.
- Windows y Linux, en las rondas de validación multiplataforma.

## Prueba de plugins

- **Funcionó solo**:
  - `task-source`, `stack-detect` (rust), `deliver-contract validate` y `baseline` con `suite.command` fijado al adaptador. B1 reconoció los 13 rojos del contrato (`suite.redExplained`).
  - `brief-slices` (un solo experto) y los gates P2/P3.
- **Lo que tuve que rodear**:
  - **rust-architect.** Agotó su límite de 80 turnos explorando, sin escribir nada. Hubo que reanudarlo para que escribiera.
  - **Línea base.** Tuve que medirla 3 veces:
    1. B1s por `cargo fmt --check`, porque los tests rojos del arquitecto no estaban formateados.
    2. B1 por un test inestable bajo carga, `gitraptor-cli::live_changes::ten_active_worktrees_are_followed_without_losing_events`. Pasa aislado; lo repetí con `NEXTEST_TEST_THREADS=2`.
    3. Sana.
  - **P3 sin integridad.** P3 corrió `cargo nextest run` directamente y no el `suite.command` del contrato, así que no escribió `junit-paths.xml` y la integridad quedó **sin comprobar** («faltan informes»).
  - **Conformidad.** `devspec-map conformance` sobre un Brief devuelve 0 tareas, así que la recorrí a mano (ver la sección siguiente).
  - **Commits mezclados.** El rust-expert hizo `git add` de todo el árbol y metió en `dc2bda18` y `742c746c` mis documentos S4, que estaban sin commitear (historia, business-rules y ADRs). El hook bloquea `git rebase -i`, así que no los separé: esos dos commits mezclan código y documentos.
  - `docs/ARTIFACTS.md` no lo tocó ningún hook.

## Conformidad (recorrido a mano)

| Cláusula de la historia | Prueba |
|---|---|
| Esc. 1: borrado de rama con punto previo | `scenario_1_*` (us_tmc_005_hook) |
| Esc. rebase con "snapshot previo" | `scenario_1_rebase_entry_shows_hook_prior` |
| Esc. 2: sin hooks, por observación | tests existentes de US-TMC-004 (`raw_git_undo`, `continuous_observation`) |
| Esc. 3: si el previo falla, no figura como protegido | `scenario_3_*` |
| Esc. 4: una consulta no genera punto | `scenario_4_*` |
| RT: solicitante por ascendencia | `us_tmc_005_unattributed.rs` y `quota_*` (cubo por solicitante) |
| RT: sin recursión | `one_point_per_git_command_*` |

Implementa: US-TMC-005 · Refs: ADR-TMC-004, ADR-TMC-005, ADR-TMC-002, ADR-TMC-006, ADR-GRD-001, ADR-GRD-002, ADR-GRD-003, US-GRD-017, Q-GRD-37, TD-TMC-001, NFR-01, NFR-02
