# Preparación de la corrida · TS-CKP-001 PR-B (2026-10-28)

Sin diseño: [DS-TS-CKP-001](../requirements/features/cockpit/dev-specs/TS-CKP-001-predictor-conflictos.md) es el plan. Alcance: **solo PR-B** (D15) = T006 (proceso trabajador), T007 (pool `utility` y pausa por ahorro), T009 (publicación: `prediction.pairs`, `prediction.list`, `prediction.changed`, `RepoView.prediction`, `attention.conflicts`), T010 (suite `repo_intact` del predictor y fronteras), T012 (escenario del banco) y T013 (cierre). T001 a T005 y T011 están en `main` (#275). G2 lo cerró #283 y G5 lo cerró #298. Contrato: [ts-ckp-001-prb-predictor.contract.json](ts-ckp-001-prb-predictor.contract.json). Preparó el loop principal (opus).

## Deriva de citas

Todas las citas de la spec siguen vigentes; solo cambian algunas líneas. Lo que afecta al build:

| Cita de la spec | Hoy | Efecto |
|---|---|---|
| `scope.rs:110` `attention.conflicts` sin publicar | `AttentionView::unpublished()` en `scope.rs:114`, usado en `conn.rs:2459` | El cálculo híbrido sustituye esa llamada |
| `event.rs:93` `scope_of` | `event.rs:89` | Ninguno |
| `rpc.rs:26` `InvalidParams` | `ErrorCode::InvalidParams` en `rpc.rs:110` | Ninguno |
| `state.rs:56` `RepoPairs::apply`, `state.rs:23` `queued_state` | vigentes | La regla de la base movida va ahí |
| `predict/mod.rs:47-75` enums del motor | se desplazan unas 20 líneas por `PredictConfig` | Ninguno |
| `methods/prediction.rs` declara también el evento | Los tipos de evento se registran en `event.rs` (lista `KINDS`, `engine_repo()`) | La constante `PREDICTION_CHANGED` vive en `methods/prediction.rs` y el tipo se registra en `KINDS` como los demás |
| `repo.attention` | El tipo está declarado (`event.rs:132`, `:184`), pero nadie lo publica | T009 crea el primer publicador |
| `crates/core/src/daemon/modules/predict.rs` | `RepoState.queue` por repo, y nada ejecuta la cola | La cola pasa al pool (S3) |

Ninguna deriva contradice la spec: **sin BLOQUEO**.

## Stubs de la preparación

Para que las pruebas del contrato compilen en rojo, la preparación deja firmas sin cuerpo (`unimplemented!("T00x")`), fuera de cualquier camino de producción:

- `crates/api/src/prediction.rs`: los tipos del cable de § Contrato de API, completos, más `pair_id`, `is_pair_id`, `validate_list`, `fit_summary` y `page`. También `methods/prediction.rs` con las constantes (el `GROUP` lo registra T009) y `RepoView.prediction` (`None` en todos los literales).
- `crates/core/src/predict/{merger.rs,pool.rs,worker/mod.rs,worker/serve.rs}`: `DryMerger` e `InProcessMerger` (ya reales, porque son una llamada a `merge_in_memory`), `PredictPool`, `Job`, `Done`, `WorkerMerger`, `WorkerConfig`, `WorkerLimits`, `serve`, `read_frame`, `write_frame` y `validate_outcome`.
- `DaemonConfig.predict: PredictConfig` (`PredictMerger::Off` por defecto), añadido a los 53 arneses que construyen la configuración a mano.

## Supuestos

Decisiones del orquestador (2026-10-28) dentro de lo que la spec ya fija. Ninguna cambia la forma del cable ni un ADR.

- **S1 · Cómo arranca el predictor.** `DaemonConfig.predict.merger`. `Off` es el valor por defecto de los arneses: no corre ningún merge en seco, no se publica nada y `attention.conflicts` sigue en `unavailable(not-published)`, como hoy. `InProcess` sirve para las pruebas de publicación (D8). `Worker(WorkerConfig)` es el de producción: `for_current_user()` resuelve `current_exe()` una sola vez. Si no puede resolverlo, el predictor queda en `Off` y se registra en el log, y el daemon sigue arrancando. `daemon::run_process`, el del banco, hace lo mismo.
- **S2 · Costuras de prueba del trabajador.** `WorkerLimits` es inyectable (plazo, plazo del escalado, cota de memoria, muestreo e inactividad). Las pruebas usan `Duration::ZERO` y cotas mínimas para que el corte sea determinista, y una prueba fija que `WorkerLimits::default()` vale lo de § Valores numéricos. `worker_pid`, `launches`, `close_if_idle` y `close` dejan observar el relanzamiento sin `sleep`. Las pruebas de `crates/core` llegan a `raptor` con `gitraptor_testkit::sibling_bin`, tomando como referencia un binario del directorio del perfil (el abuelo de `current_exe()`).
- **S3 · La cola pasa al pool.** `PredictPool` tiene una sola `PairQueue`, porque `PairKey` ya lleva el `repo_id`, y se elimina `RepoState.queue`. El hilo `raptor-predict` hace `submit` y `forget_repo` (repo dormido o retirado) y recibe `Done` por su canal de mensajes. Después aplica `RepoPairs::complete` y publica. El hilo del pool es `work_class::spawn(Pool::Predictor, WorkClass::Utility, …)`.
- **S4 · Ahorro de energía (D5).** El hilo del pool espera trabajo y cambios de modo con `PowerMode::wait_change`, sin sondeo fijo. En ahorro no saca nada y cierra el trabajador. El hilo `raptor-predict` recibe el cambio, pasa los pares a `Pending(PowerSaving)` con su último resultado y, al volver a corriente, replanifica: solo se encolan las triples que cambiaron. El cambio se aplica en ≤ 60 s.
- **S5 · El escalado lo hace `WorkerMerger`** (D11): los dos trabajos, cada uno con su plazo (`deadline`, `escalated_deadline`). `InProcessMerger` no escala, igual que `merge_in_memory`.
- **S6 · Memoria del hijo sin `unsafe`.** En macOS se lee con `libproc::pidinfo::<TaskInfo>`, que ya es dependencia (`resources/meter.rs`). En Linux, con `/proc/<pid>/statm`. `RLIMIT_AS` lo fija el propio trabajador al arrancar, con `nix::sys::resource::setrlimit`, porque un `pre_exec` exigiría `unsafe` y el workspace lo prohíbe. La muerte es `Child::kill` (SIGKILL) seguido de `wait`. En Windows no hay cota de memoria: queda **pendiente de la etapa multiplataforma** (*Job Object*). No se añaden dependencias.
- **S7 · Protocolo.** Cada trama lleva un prefijo de 4 bytes con su longitud (u32 big-endian) y un cuerpo JSON con tipos privados de `predict::worker`, con un tope de `MAX_WORKER_FRAME` en los dos sentidos. La primera trama del hijo es su versión (`CARGO_PKG_VERSION` más `GIT_DESCRIBE` si existe). Si no coincide, se mata y se relanza una vez; si vuelve a fallar, `Unavailable` (estado `error`). El daemon valida la respuesta con `validate_outcome` antes de usarla.
- **S8 · Lectura compartida.** Los pares publicados viven en un `PredictionStore` compartido (`Arc`, en el contexto del canal, como `mcp_repos`). Lo escribe solo el hilo `raptor-predict`. La firma queda `pub(crate) fn predict_read(store: &PredictionStore, repo_id: &str, worktree: Option<&Path>) -> Vec<PairDetail>`, porque la función necesita el estado. `mcp.conflicts` (US-MCP-016) la reutiliza igual.
- **S9 · `rev`.** El `rev` de cada par de un lote es el `scope_seq` de ese `prediction.changed`. Se asigna bajo el mismo cerrojo del `EventBus` que numera el evento, con una variante de `publish_with` que pasa el `scope_seq` al cierre. Así el almacén y el evento no pueden divergir.
- **S10 · `attention.conflicts`.** Sale del almacén con la regla híbrida, y `repo.attention` se publica solo cuando cambia. Ambas cosas solo con `merger` distinto de `Off`.
- **S11 · Clientes.** La CLI, la TUI y `raptor-mcp` se adaptan para compilar (`prediction: None`) y no negocian `prediction.pairs` todavía: lo harán US-CKP-006/007/008 y US-MCP-016. El cable de una conexión sin la capacidad no cambia, y lo prueba CKPB-API-SHAPE.
- **S12 · Banco (T012).** El binario del banco se reejecuta como daemon (`--daemon-root`), así que también atiende `predict-worker`. El escenario `prediction` sigue a ADR-GRP-011 § 4 (≥ 200 muestras y descarte de las 10 primeras). La medición con la máquina en reposo **no es gate de este PR** (instrucción del coordinador): el script reproducible es `cargo bench -p gitraptor-cli --bench engine -- --only prediction`. El contrato solo comprueba que el escenario existe y compila. La cifra se informa al coordinador.
- **S13 · Hostiles de T010.** El blob de 100 MB se corta por cabecera con los límites reales. Los dos casos de muerte (10.000 archivos en conflicto por memoria y blob denso por plazo) usan `WorkerLimits` reducidos para que sean rápidos y deterministas. En los tres, el daemon sigue respondiendo a `daemon.status`.
- **S14 · CI.** `repo_intact_min` de `.github/workflows/repo-intact.yml` sube en el número de pruebas `repo_intact::` nuevas. No cambia ninguna variable `RUST_*` ni `CARGO_*`.
- **S15 · Independencia del actor.** Las pruebas del predictor no dependen de sesiones de agente (`session_present` sigue en `false`). Si un arnés de daemon necesita el detector, usa `AgentMatcher::only(["raptor-fake-agent"])`, y la suite pasa igual dentro y fuera de una sesión de Claude Code.
- **S16 · Cierre (T013).** TS-CKP-001 y su Dev Spec pasan a `implemented`, con la fila de `backlog.md`, el índice `technical-stories.md` y `release-status.md` regenerado.

## Convenciones observadas

Solo lo que la spec no cubre:

- Las pruebas de unidad en rojo van en un archivo propio (`pool/tests.rs`, `worker/tests.rs`, `prediction/tests.rs`), declarado con `#[cfg(test)] mod tests;`, como `power/monitor/tests.rs`.
- El daemon en hilo para las pruebas de `crates/core` se monta como en `crates/core/tests/guard_evaluate.rs:50-90` y `power_daemon.rs`, con `predict: PredictConfig { merger: … }`.
- Una capacidad nueva se aplica a la conexión con `has(CAP_…)` (`conn.rs:645`) y se filtra en el `Outbox` con un `set_without_*` (`bus.rs:95-167`, patrón `set_without_authorship`). El campo de la instantánea sigue el patrón de `kept_temps` (`conn.rs:2344-2354`).
- Los métodos nuevos usan `method(NAME, …).since(CAPABILITIES_PROTOCOL)` (`methods/events.rs:34-46`).
- El subcomando oculto sigue el patrón de `#[command(hide = true)] Ui` (`apps/cli/src/commands/mod.rs:47-49`).
- Las comprobaciones estáticas recorren los archivos con `rust_files` (`crates/git/tests/static_check.rs:17`).
- Los hilos de clase `utility` se lanzan con `work_class::spawn` o `spawn_unpooled` (`modules/guard_health.rs:44`).

## Tramos

Un solo experto (opus), en orden: la forma del cable (api), el pool, el trabajador y su subcomando, la publicación en el canal, la suite `repo_intact` y las fronteras, el banco y el cierre. Los tramos comparten `crates/core/src/daemon/modules/predict.rs` y `conn.rs`, así que no se paralelizan.
