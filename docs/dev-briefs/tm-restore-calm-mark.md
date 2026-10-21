# Brief: la marca en calma espera a los refs (fix/tm-restore-calm-mark)

## Alcance

NFR-01. `us_tmc_009::a_restore_can_be_undone_back_to_the_exact_prior_state` falla en CI: el undo deshace `git-event-3` (eco tardío del commit protegido) en vez del restore. Este run cierra el hueco de la calma del motor para que toda marca en calma (intención, ancla, restore, undo, captura) cubra también los cambios de refs ya hechos en disco. Cita ADR-TMC-003 § 4 (Marca en calma, `caused_by` por ancla) y ADR-TMC-004 § 2 (calma por estado). Validado por nassa-architect «con ajustes»; los ajustes ya están incorporados.

### Causa raíz (evidencia)

- La calma es **solo por el reflog de `HEAD`** del worktree: `timemachine/engine.rs:RepoMarks::calm` (L159-189). Ningún ref de rama entra en la comparación.
- Hueco H1 (commit a medias): `git` añade al reflog de `HEAD` antes de renombrar el ref. `watch/repo.rs` L816-833 difiere una sola vez un lote "solo reflog" y después lo envía **sin eventos**. Además `RefsView::read` (L84-120) lee las ramas antes que `head_log_len` (L216). `daemon/repos.rs:observed` L497-501 publica esos `head_logs`, así que `settle` da una marca que no cubre `commit`/`branch-update` (eventos 2 y 3). Con eso, el ancla de la op 15 (`channel/conn.rs:3330 after_step`) y la intención del restore (`restore.rs:435-438`) toman la marca 1.
- Hueco H2 (refs fuera de `HEAD`): `branch -D` y el `update-ref` del aplicador no escriben el reflog de `HEAD`, así que la calma es inmediata. Eso explica la op 15 con marca 0. `continuous.rs:await_moved_branches` (L667-718) solo cubre ramas checked-out, y le basta *cualquier* evento posterior con el mismo nombre de rama (falso positivo: los 2 y 3 de la op 15 "satisfacen" la espera del restore).
- Resultado: `undo.rs:external_events_in` (L274-335) deja 2 y 3 sin `caused_by`. `oplog/stack.rs:undo_stack_in` (L110-112) los ordena por `seq` encima del restore (marca 1).

### Decisiones

Cada una es una «Decisión del orquestador (2026-10-04), validada por Arquitecto».

- **D1. Barrera por estado de refs, no ligado de ecos.** Un worktree está en calma solo si, además de lo de hoy, las ramas locales del repo en disco son las del último lote persistido. Ligar ecos exigiría adivinar qué emite un `git`. La barrera cierra H1 y H2 y mantiene "calma por estado" (ADR-TMC-004 § 2).
- **D2. Paridad de lectura.** `RefsView::read` y la calma leen las ramas con un único helper, `watch::read_local_branches`, sobre `RepoReader::local_branches`. La comparación es directa entre `BTreeMap<String, String>`, sin construir texto.
- **D3. Directorio común confiable.** `RepoMarks` guarda el directorio común que registró el daemon y lee de ahí, nunca a través del `.git` de un worktree (#223 I-03).
- **D4. Falla cerrado.** La lectura es falible (`Result`). Si el repo no abre o el listado falla, la respuesta es "no en calma" (no se usa el `unwrap_or_default` de `continuous::branch_tips`). Solo si el motor nunca publicó ramas para el repo se usa el respaldo por tiempo `quiet()`. Fuera del límite, `RepoBusy`/`GitBusy`, como hoy.
- **D5. Orden de memoria explícito.** Ramas y `seq` del último lote viven en el mismo `Mutex` (`RepoMark.persisted`). El escritor publica `seq` y ramas juntos, en una sección crítica. El lector compara las ramas y lee `seq` en esa misma sección. Si coinciden con las del lote N, la `seq` devuelta es ≥ la del lote N. El `AtomicI64 seq` se sigue actualizando para `mark()`.
- **D6. Barrera por repo.** Se comprueba una vez por sondeo de `settle`, para todo el repo, no por worktree. Los commits de un worktree B retrasan el `settle` de A. Es un límite declarado y medido (C6). No se acota por worktree en este fix.
- **D7. Despertar antes de esperar.** `DaemonEngine::settle` envía `ShutdownHandle::wake(repo_id, WakeCause::Sentinel)` antes de sondear. Hoy el restore y el undo solo despiertan el repo en `raw_events` (`daemon/mod.rs:830-839`), que va *después* de `settle`, y el ejecutor no lo despierta.
- **D8. `await_moved_branches` se mantiene sin cambios en este fix.** Se retira aparte (ver Not Built).

### Caso 2 (W85): una captura sellada con una marca anterior a su contenido (pérdida de datos)

`apps/cli/tests/raw_git_undo.rs::an_agent_recovers_its_work_after_a_raw_reset_hard` falla 8 de 8 bajo carga. El reset es el evento 2, pero la Observation seq 4 tiene marca 1 y contenido *posterior* al reset. El undo elige el último punto con marca < 2, que es la seq 4, y restaura sin el trabajo del agente.

- `continuous.rs:observe` toma la marca en `settle` (L553-556) y **después** lee `at_start = GitState::read` (L557). Un `git reset --hard` que termina entre los dos pasos ya está en `at_start`.
- El guard `still_valid` (L587-591) solo compara `at_start` con el punto de validez (`store/capture.rs:707`), así que no ve nada.
- La lectura del contenido (`capture.rs:capture_locked`, detección L513+) llega aún más tarde: tras la cola del almacén (`prior_waiting` o el lock), y la marca ya está fijada en `CaptureRequest.engine_mark` (L584).
- El mismo patrón está en `manual.rs:446`/`575` y en `hook_prior.rs:332`/`396`. El prior garantizado (`protected/mod.rs:137-141`) lleva la marca de la intención y no tiene ningún guard (`still_valid: None`).

Cada una es una «Decisión del orquestador (2026-10-04), validada por Arquitecto». nassa-architect las validó «con ajustes»; los ajustes ya están incorporados.

- **D9. Se confirma la marca en el punto de validez.** Toda captura sellada con una marca de `settle` lleva en `still_valid` un AND con `engine::confirm_mark` (definida en `timemachine/engine.rs`; entra **solo** por el closure de `still_valid`, sin tocar `store/capture.rs`), que exige `engine.settle(repo, worktrees, ZERO) == Some(mark)`. Con D1-D5 eso significa: calma ahora (reflog, `HEAD`, índice y ramas) y la `seq` no se movió. Si además el `GitState` es igual desde `at_start` hasta la validez, el contenido leído entre medias es exactamente el estado que la marca cubre. Un evento que se persiste entre la marca y la lectura mueve la `seq`; uno que aún no se ha persistido rompe la calma. En los dos casos, `Discarded`.
- **D10. Se descarta, no se re-marca.** Re-marcar con la `seq` nueva exigiría probar que el contenido ya incluye ese evento, y el guard no compara ramas. Descartar falla cerrado. La captura continua reintenta (`Attempt::Again`) y el ancla reintenta 3 veces (`anchor`). Un ancla que falla no puede dejar el eco de su operación como Git crudo: `external_events_in` cae a la siguiente captura o a `i64::MAX` (C17).
- **Trade-off aceptado (D10, inanición):** la `seq` es de todo el repo, así que la actividad de un worktree hermano descarta las capturas de este. `Discarded` → `Attempt::Again` cada `SETTLE` (150 ms) hasta `MAX_RETRIES` = 50; el tope de 6×M de `give_way` no aplica. Al agotarse se registra `tm_capture_failed kind=discarded-exhausted`. Se prefiere perder cobertura a sellar una marca falsa. C16 mide que con actividad sostenida en el hermano sigue saliendo al menos una Observation en un tiempo acotado.
- **D11. Alcance: todas las capturas que llevan marca de `settle`.** Son `observe` (continua y ancla), `manual::capture` y `hook_prior`. La regla es una sola función, sin copias. `hook_prior` (`hook_prior.rs:361`, que hoy convierte `Discarded` en error al primer intento) reintenta mientras quede plazo. Al vencer, `TimeLimit` → denegación de US-GRD-017 (falla cerrado). La denegación sale por plazo, nunca por un único descarte (C18).
- **D12. El prior garantizado también confirma.** `ProtectedOperation` le pasa `still_valid = confirm_mark(intent mark)`. Si sale `Discarded`, el ejecutor vuelve a tomar la marca de intención y repite el prior hasta 3 veces. Después rechaza por la vía existente de "marca de intención sin calma" (`executor/mod.rs:988 refuse(&plan, RejectReason::GitBusy)`), no por `Refusal::GitBusy{lock}`. El paso no corre: nada cambia y el agente reintenta.
- **D13. Compatibilidad con #272 (`prior_point.rs`).** `before_event` y su sombreado por `mid_operation` asumen que una marca vieja solo aparece dentro de la ventana de comando de un prior. D9 elimina las Observation con marca vieja fuera de esa ventana sin tocar el contrato de `PointFacts`. La regla vale en main y tras #272. #272 **sí** toca `store/capture.rs`; por eso D9 no lo edita: `confirm_mark` vive en `engine.rs` y entra solo por `still_valid`. Los demás archivos de este slice (`continuous.rs`, `manual.rs`, `hook_prior.rs`, `protected/mod.rs`, `executor/mod.rs`) no los toca #272.
- **D14. Hook de depuración para W85.** `CaptureDeps.after_mark: Option<Arc<dyn Fn() + Send + Sync>>`, `#[doc(hidden)]`, se ejecuta en `observe` entre `settle` y `GitState::read` y solo en `cfg!(debug_assertions)`, con el mismo patrón que `CaptureDeps.layer` (`continuous.rs:97`, `TmCapture.layer`). Es lo que inyecta el `reset --hard` de forma determinista. En release se ignora.

### Caso 3: un `git` que sigue vivo ya cambió el contenido, pero aún no su reflog (pérdida de datos)

`raw_git_undo` pasa 8 de 10. El objetivo del undo es la Observation seq 4, con marca 1 y contenido posterior al reset; el Reset es el evento 2.

**Hipótesis del experto descartada.** Los `head_logs` no adelantan al evento:
- El `Reset` sale de la misma lectura que el tamaño (`watch/repo.rs:376-400`): `head_log_tail(before, w.head_log_len)` sobre la misma `new`.
- El lote "solo reflog" se difiere solo si `!others` (L823-833), y un reset al mismo commit siempre trae su línea `reset:` → evento → `others`.
- El arranque y el despertar publican los `head_logs` de la vista base de la propia tarea (`watch/mod.rs:988-1005`, `1219`; `daemon/repos.rs:360`, `tiers.rs:357`): no hay nada por delante de la base.
- Una línea a medio escribir es teórica: git añade la línea con un solo `write`. Aun así se cierra (D16).

**Causa probada: la calma ve el reflog, y `reset --hard` lo escribe el último.**
1. `git reset` hace primero `reset_index` (árbol de trabajo + índice, con `index.lock` solo mientras escribe el índice).
2. Después `reset_refs`: una transacción para `ORIG_HEAD` y luego otra para `HEAD`, que añade la línea `reset:` al reflog de `HEAD`. Cada una dispara el hook `reference-transaction committed`.
3. El hook del test duerme 0,5 s en `committed` (`apps/cli/tests/raw_git_undo.rs:132-136`). Duerme primero en la transacción de `ORIG_HEAD`, con el contenido ya revertido y el reflog de `HEAD` aún sin tocar.
4. Durante ese hueco, `RepoMarks::calm` (`engine.rs`) da calma: el reflog en disco es igual al publicado, no hay `index.lock`, `HEAD` y las ramas no cambian (D1). `settle` da 1.
5. `GitState` en `at_start` y en la validez es el mismo (el índice ya se escribió antes), y `confirm_mark(1)` también da calma con `seq` 1 (D9).
6. Resultado: Observation con marca 1 y contenido sin WORK. La suposición del doc de `engine.rs` ("todo `git` que cambia el estado escribe el reflog", y así lo cubre la calma) solo vale para un `git` **terminado**. Un `git` en curso ya puede haber cambiado el contenido sin haber escrito su reflog. Sin el hook, el hueco es el tramo entre `reset_index` y `reset_refs`, que la carga ensancha.

Cada una es una «Decisión del orquestador (2026-10-04), validada por Arquitecto». nassa-architect validó D15-D16 «con ajustes»; los ajustes se recogen en D17-D23.

- **D15. No hay calma mientras viva un `git` en el ámbito.** `RepoMarks` recibe un `Arc<dyn detect::procs::ProcLister>`, que ya existe, ya tiene 2 implementaciones (`SystemProcLister` y la tabla sintética de los tests de `detect`) y ya lee `exe` y `cwd`.
  - `calm(repo, wt)` es `false` si hay un proceso con `exe` cuyo nombre es `git`/`git.exe` y cuyo `cwd` está bajo la raíz canónica del worktree o bajo su directorio Git.
  - Quedan fuera los hijos directos del daemon (`ppid == std::process::id()`): sus `git` de escritura corren bajo el lock del repo, fuera de cualquier `settle` del mismo repo.
  - Falla cerrado: un `git` con `cwd` ilegible cuenta como dentro del ámbito, y `list_bare() == None` es "no en calma". Excepción declarada: en una plataforma donde `ProcLister::cwd` no está soportado, D15 queda inactivo y se registra `tm_calm_probe_unsupported` una vez.
  - Sin lister (dobles y tests), la regla de hoy.
  - Una sola lista de procesos por sondeo de `settle`, compartida entre los worktrees. D9 hereda D15 sin cambios: `confirm_mark` llama a `settle(ZERO)`.
- **D16. Solo se publican líneas completas del reflog.** `head_log_len` (`watch/repo.rs:223`) y el `head_log_len` de `engine.rs` pasan a ser el desplazamiento tras el último `\n`, no el tamaño del archivo. Una línea a medio escribir mantiene la calma en falso solo hasta que se completa: el crecimiento sin evento (checkout de la misma rama, commit detached) sigue llegando a la calma, porque cada línea completa se publica aunque no genere evento (L816-860).
- **Trade-off aceptado (D15):** un `git` largo y de solo lectura en el worktree (`git log` con paginador, `git commit` con el editor abierto) bloquea la calma mientras vive. Las capturas se aplazan y las operaciones TM devuelven `repo-busy` pasado el límite. Se prefiere eso a sellar contenido de un `git` en curso con una marca anterior. `gc --auto` entra en el mismo trade-off.
- **D17. Interbloqueo con `hook_prior` (bloqueante): exclusiones explícitas.** `hook_prior.rs:412`/`452` llama a `settle` mientras el `git` que disparó el hook sigue vivo; con solo D15, se bloquearía hasta `TimeLimit`.
  - `settle` y `calm` reciben `CalmExclusions { pids, own_hook }`. `pids` son los ancestros `git` del cliente del hook, ya calculados en `guardrails/evaluate.rs:339` (`EvalRequest.chain`). Si `chain` es `None` (cadena desconocida), no se excluye nada: falla cerrado → `TimeLimit` → denegación de US-GRD-017.
  - Exclusiones vacías para todo lo demás.
- **D18. Llamadas de hook en vuelo (complemento barato, válido en Windows).** `RepoMarks` lleva un contador de llamadas de hook en vuelo por worktree: `hook_begin(repo, wt)` devuelve un guard RAII, y el daemon lo toma al recibir la llamada del hook y lo suelta al responder. `calm(wt)` es `false` mientras haya alguna, salvo la `own_hook` del propio `settle` (D17).
- **D19. D15 se mantiene.** Cubre el hueco sin hooks: un `git` sin hooks entre `reset_index` y `reset_refs`, que bajo carga no dura microsegundos garantizados.
- **D20. `git -C <wt>` desde otro `cwd`.** Un `git` cuyo `cwd` cae fuera del ámbito cuenta igualmente si `git_redirects(args, env)` (`detect/procs.rs:66`) da `true` o no se puede evaluar (argv o env ilegibles). Falla cerrado: un `-C` o un `GIT_DIR` puede apuntar al ámbito.
- **D21. Coste: primero lo barato.** El orden es reflog, `HEAD`, `index.lock`, ramas (D1), hooks en vuelo (D18), y solo si todo dice calma, procesos.
  - `list_bare()` da el nombre sin `exe`; `exe`, `cwd` y argv se leen solo para los candidatos `git`.
  - Se mide el p95 de una pasada en macOS con unos 800 procesos (C27). Si pasa de 5 ms, se cachea la lista como mucho 1 sondeo por repo.
- **D22. `git` de larga vida excluidos por argv ya.** Quedan fuera `git fsmonitor--daemon` y `git cat-file --batch*` (`--batch`, `--batch-check`, `--batch-command`). `git credential-cache--daemon` se ejecuta como `git-credential-cache--daemon`, con otro nombre de `exe`, así que el filtro por nombre no lo cuenta; un test lo fija. El paginador, el editor y `gc --auto` quedan como trade-off aceptado.
- **D23. Observabilidad e inanición declarada.** Un `git status` en bucle (IDE, prompt de shell) puede impedir la calma por D15 hasta el límite. Es un límite declarado. Cada vez que un `settle` agota su límite por D15, se registra `tm_calm_blocked_by_git` con `pid` y subcomando (`argv[1]` saneado), nunca el argv completo, como mucho una vez por `settle`.
- **Nietos del daemon.** Solo se excluyen los hijos directos (`ppid == daemon`). Los nietos (hooks y subprocesos de un `git` del propio daemon) no; esos `git` corren bajo el lock del repo, que serializa las operaciones TM del mismo repo, y un `settle` concurrente de la captura continua solo se aplaza.
- **⚠️ RIESGO NFR-01 abierto (Windows).** Sin `cwd` de otro proceso, D15 queda inactivo (se registra `tm_calm_probe_unsupported`). D18 cubre los `git` con hooks de GitRaptor, pero no un `reset --hard` sin hooks en pleno vuelo. Está registrado en `docs/requirements/release-plan.md` (Huecos, fila 12), no solo como "Pendiente".

## Topología

Slice 1 (tracer, criterios: C1, C2, C3, C3b):
- `crates/core/src/watch/mod.rs`: `pub fn read_local_branches`. `WatchStart.branches: Option<BTreeMap<..>>`, que se rellena en `watch_repo` (L988) y al despertar (L1219).
- `crates/core/src/watch/repo.rs`: `RefsView::read` usa `read_local_branches` (el error se mapea a `RefsReadError::Listing`).
- `crates/core/src/timemachine/engine.rs`: `RepoMark.persisted` y `common_dir`, `set_persisted`, `refs_calm` dentro de `settle`, `remove()` que limpia todo. Doc del módulo (refs y orden de memoria).
- `crates/core/src/daemon/repos.rs`: `set_persisted` tras `write_batch` Ok (L499-501) y en el arranque (L355). Las ramas del lote viajan en un campo nuevo `ObservedBatch.branches: Option<BTreeMap<..>>`, que sale de `new.branches` en `repo.rs` L801.
- `crates/core/src/daemon/tiers.rs`: `set_persisted` al despertar (L352).
- `crates/core/src/daemon/tm.rs`: D7 en `DaemonEngine::settle`.
- `crates/core/tests/tm_calm_refs.rs` (nuevo): C1, C1b, C2, C3, C3b, C5b, C8.

Slice 2 (criterios: C4, C6, C7, C9):
- `crates/core/tests/tm_calm_refs_stack.rs` (nuevo): C4.
- `crates/core/tests/tm_calm_refs_load.rs` (nuevo, `#[ignore]`): C6, C9.
- `crates/core/tests/tm_calm_dormant.rs` (nuevo): C7.
- `docs/architecture/decisions/ADR-TMC-003-…md`: enmienda. `ADR-TMC-004-…md`: una línea en la fila "Calma por estado".

Slice 3, caso 2 (criterios: C10-C18; depende de D5 del slice 1):
- `crates/core/src/timemachine/engine.rs`: `pub fn confirm_mark`. Es el mismo archivo del slice 1, así que va en serie después de él.
- `crates/core/src/timemachine/continuous.rs`: el AND en `still_valid` de `observe` (L587), `CaptureDeps.after_mark` (D14) y el log `discarded-exhausted` en el agotamiento de `Pending::after`.
- `crates/core/src/timemachine/manual.rs` (L446) y `hook_prior.rs` (L332, más el reintento por plazo en L361).
- `crates/core/src/timemachine/protected/mod.rs` (L137-141) y `executor/mod.rs` (L986-989: reintento ×3 y `refuse(GitBusy)`).
- `crates/core/src/timemachine/capture_mark_tests.rs` (nuevo; `#[cfg(test)] #[path]` en `continuous.rs`): C10-C13, C16, C17.
- `crates/core/src/timemachine/hook_prior_mark_tests.rs` (nuevo; `#[path]` en `hook_prior.rs`): C18.
- `crates/core/src/executor/prior_mark_tests.rs` (nuevo; `#[path]` en `executor/mod.rs`): C14.

Slice 4, caso 3 (criterios: C19-C22):
- `crates/core/src/timemachine/engine.rs`: `RepoMarks::set_procs`, el paso de procesos en `settle`/`calm`, la longitud por líneas completas y el doc del módulo corregido ("terminado").
- `crates/core/src/watch/repo.rs`: `head_log_len` por líneas completas (L223).
- `crates/core/src/daemon/mod.rs` (o donde se construyen las `RepoMarks` del daemon): `set_procs(Arc::new(SystemProcLister))`.
- `crates/core/src/timemachine/calm_git_alive_tests.rs` (nuevo; `#[path]` en `engine.rs`): C19, C20, C21.
- `crates/core/tests/tm_calm_reset_in_hook.rs` (nuevo, `#![cfg(unix)]`): C22.
- `crates/core/src/timemachine/engine.rs` (en serie): `CalmExclusions`, `settle_excluding`, `hook_begin`, el orden barato de D21, los filtros de D22 y el log de D23.
- `crates/core/src/timemachine/engine.rs` `EngineLink`: `settle_excluding` y `hook_begin`; `crates/core/src/daemon/tm.rs`: su implementación.
- `crates/core/src/timemachine/hook_prior.rs` (L412, L452): `settle_excluding` con `chain` + `own_hook`.
- `crates/core/src/daemon/guard.rs`: `hook_begin` al recibir la llamada del hook, el guard vive hasta la respuesta.
- `crates/core/src/timemachine/calm_exclusions_tests.rs` (nuevo; `#[path]` en `engine.rs`): C23-C26, C28, C29.
- `crates/core/src/timemachine/hook_prior_live_git_tests.rs` (nuevo; `#[path]` en `hook_prior.rs`): C24.
- `crates/core/tests/tm_calm_probe_cost.rs` (nuevo, `#[ignore]`): C27.

## Contratos

- `pub fn read_local_branches(reader: &RepoReader) -> Result<BTreeMap<String, String>, RefsReadError>` en `gitraptor_core::watch`. `RefsReadError` pasa a ser `pub` en `watch`.
- `pub fn set_persisted(&self, repo_id: &str, seq: i64, common_dir: &Path, branches: BTreeMap<String, String>)` en `RepoMarks`. Hace en un solo lock `seq.fetch_max(seq)` + `persisted = Some((seq, branches))` y guarda `common_dir`. En el daemon sustituye al `set_seq` posterior a cada lote que trae ramas.
- `RepoMarks::settle(&self, repo_id, worktrees, limit) -> Option<i64>`: misma firma. Devuelve `Some(s)` solo si `all(calm)` y `refs_calm` dan `Some(s)`. `refs_calm`, bajo el lock de `persisted`:
  - sin publicar: `quiet().then(seq)`;
  - con publicado `(s, b)`: abre el directorio común; si la lectura da `Err`, `None`; si las ramas en disco `== b`, `Some(max(s, seq atómico))`; si no, `None`.
- `RepoMarks::remove(repo_id)`: borra también `persisted` y `common_dir` (sale con la entrada entera del mapa; un test lo fija).
- `ObservedBatch.branches: Option<BTreeMap<String, String>>`, `WatchStart.branches: Option<BTreeMap<String, String>>`: campos nuevos, con `None` por defecto.
- `EngineLink`, el wire y los códigos `repo-busy`/`git-busy`: sin cambios.
- `pub fn confirm_mark(engine: Arc<dyn EngineLink>, repo_id: &str, worktrees: &[PathBuf], mark: i64) -> ValidityGuard` en `timemachine::engine` (`ValidityGuard` se importa de `store::capture`, sin editar ese archivo): `true` solo si `engine.settle(repo_id, worktrees, Duration::ZERO) == Some(mark)`. No bloquea: un solo sondeo.
- `ProtectedRequest` añade el campo `engine: Option<Arc<dyn EngineLink>>`, o recibe el guard ya construido; lo decide `rust-expert` siguiendo `protected/mod.rs:57`. Sin engine (dobles), no hay guard, como hoy.
- `pub fn set_procs(&self, procs: Arc<dyn crate::detect::procs::ProcLister>)` en `RepoMarks`: se guarda una vez, y sin llamarla la calma no mira procesos.
- `pub struct CalmExclusions<'a> { pub pids: &'a [u32], pub own_hook: Option<HookToken> }` (`Default` = vacío) y `pub struct HookToken(u64)`, en `timemachine::engine`.
- `fn settle_excluding(&self, repo_id: &str, worktrees: &[PathBuf], limit: Duration, ex: &CalmExclusions<'_>) -> Option<i64>` en `EngineLink` y `RepoMarks`. `settle(..)` = `settle_excluding(.., &CalmExclusions::default())`.
- `fn hook_begin(&self, repo_id: &str, worktree: &Path) -> HookInFlight` en `EngineLink` y `RepoMarks`. `HookInFlight` es RAII: `token()` da el `HookToken` y `Drop` descuenta.
- `RepoMarks::settle`/`calm`: mismas firmas. `calm` añade la condición de D15. La longitud del reflog que se compara y la que se publica son las de líneas completas (D16).

## Criterios (tests; repos temporales con `HOME`/`GIT_CONFIG_GLOBAL` aislados, sin `sleep`)

- C1 `settle_waits_for_a_ref_move_the_engine_has_not_persisted`: se publica `(1, ramas)`, se inyecta `git update-ref refs/heads/x <c>` y se afirma `settle(ZERO) == None`. Después se publica el lote tardío `(3, nuevas)` y se afirma `Some(3)`. En rojo.
- C1b `packing_refs_between_the_batch_and_settle_keeps_the_calm`: se publica, se hace `git pack-refs --all` y se afirma `Some`.
- C2 `a_heads_only_batch_does_not_calm_a_commit_whose_ref_is_unpersisted`: `git commit` en un worktree enlazado. `head_logs` lleva el tamaño nuevo y las ramas publicadas son las viejas: `None`. Tras publicar las nuevas: `Some`. En rojo.
- C3 `settle_without_published_refs_keeps_the_quiet_rule`: sin publicar y con `touch(now)`, `None` a 30 ms. Con `touch(0)`, `Some`.
- C3b `an_unreadable_common_dir_is_never_calm`: se publica con ramas vacías, se borra o corrompe el directorio común y se afirma `settle(30 ms) == None`. En rojo.
- C4 `a_late_echo_bound_by_a_calm_anchor_stays_below_the_restore`: `RepoMarks` real. El lote tardío (seq 2 y 3) se publica en el orden inyectado. Se afirma que el ancla de la op 15 y la marca del restore son ≥ 3, que `caused_by(2,3) = op15` (`external_events_in`) y que `undo_stack_in(..).last_operation() == restore`.
- C5 (e2e) `us_tmc_009::a_restore_can_be_undone_back_to_the_exact_prior_state` sin cambios: 50 de 50 bajo carga en ubuntu.
- C5b `remove_forgets_the_persisted_refs`: tras `remove`, se vuelve a la regla `quiet()`.
- C6 `#[ignore] settle_holds_under_another_worktrees_ref_churn`: el worktree B mueve una rama cada X ms con un daemon real y el `settle` de A debe dar `Some` dentro de `ANCHOR_SETTLE_LIMIT`. Barre X e imprime la X mínima.
- C7 `a_tm_operation_wakes_a_dormant_repo_before_settling`: con el fixture de tiers del daemon, se duerme el repo, se mueve una rama mientras duerme y se lanza un undo. Debe salir `Ok`, no `repo-busy`.
- C8 `a_corrupt_loose_ref_reads_the_same_in_the_view_and_the_calm`: basura en `refs/heads/roto`. `RefsView::read` y `refs_calm` dan el mismo resultado (los dos `Err` → no en calma).
- C9 `#[ignore] settle_is_cheap_when_idle`: p95 de `settle` en reposo menor de 5 ms en release. Se mide también con unas 5000 ramas e imprime los números; no es un gate de debug.

- C10 `an_event_persisted_between_mark_and_read_discards_the_observation`: `EngineLink` falso. Su primer `settle` devuelve `Some(1)` y, en el mismo callback, ejecuta `git reset --hard` en el repo temporal y sube la `seq` a 2 (es el orden inyectado: evento persistido entre la marca y la lectura). Se afirma que `observe` da `Err(Capture(Discarded))` y que no queda ninguna Observation `Complete` con marca 1. En rojo.
- C11 `an_event_not_yet_persisted_between_mark_and_read_discards_the_observation`: igual que C10, pero la `seq` sigue en 1 y el reflog en disco ya no coincide, así que `settle(ZERO) == None`: `Discarded`. En rojo.
- C12 `a_calm_unmoved_mark_still_captures`: sin inyección, `Ok` con marca 1 (guarda de regresión).
- C13 `manual_and_hook_prior_captures_confirm_their_mark`: la inyección de C10 sobre `manual::capture` y `hook_prior` da `Discarded`. En rojo.
- C14 `a_guaranteed_prior_whose_mark_moved_refuses_with_git_busy`: el ejecutor con un engine falso que mueve la `seq` entre la intención y el prior da `RejectReason::GitBusy`, el paso no corre y el repo queda intacto. En rojo.
- C10/C11 inyectan el reset con `CaptureDeps.after_mark` (D14), no desde el `settle` falso.
- C16 `sibling_activity_still_lets_an_observation_through_within_a_bound`: un `RepoMarks` real. Un hilo sube la `seq` del repo (worktree hermano) cada K ms durante una ráfaga acotada y después para. La captura continua de A da al menos una Observation `Complete` antes de `MAX_RETRIES`×`SETTLE`. Con ráfaga sin fin, el log `discarded-exhausted` sale exactamente una vez. Usa barreras o canales, sin `sleep` de espera.
- C17 `a_failed_anchor_leaves_the_echo_with_its_operation`: `anchor` agota sus 3 intentos (con `after_mark` moviendo la `seq` cada vez). `external_events_in` atribuye los eventos (marca de la op, siguiente captura] a la operación y ninguno queda con `caused_by = None`.
- C18 `a_hook_prior_denies_by_deadline_not_by_one_discard`: un único descarte seguido de calma da `Ok` del prior. Descartes sostenidos dan `TimeLimit` → denegación US-GRD-017, solo al vencer el plazo.
- C15 (e2e) `apps/cli/tests/raw_git_undo.rs::an_agent_recovers_its_work_after_a_raw_reset_hard` sin cambios: 50 de 50 bajo carga, en main y tras rebasar sobre #272.
- C19 `a_worktree_with_a_git_still_running_is_not_calm`: `ProcLister` sintético con un `git` cuyo `cwd` está en el worktree; los `head_logs`, el `HEAD` y las ramas publicados coinciden con el disco. `settle(ZERO) == None`. Se quita el proceso de la tabla y da `Some`. Variantes: `cwd` ilegible → `None`; `ppid` = daemon → `Some`; `git` en otro repo → `Some`. En rojo.
- C20 `confirm_mark_rejects_a_capture_while_a_git_is_alive`: con `after_mark` (D14) se mete el `git` vivo en la tabla sintética entre la marca y la lectura, y `observe` da `Discarded`. En rojo.
- C21 `a_half_written_reflog_line_is_not_published`: un reflog cuya última línea no tiene `\n`. `RefsView::read` publica el desplazamiento de la última línea completa y no emite evento. Al completar la línea, `Reset` y la longitud entera. En rojo.
- C22 `a_capture_during_a_reset_paused_in_its_hook_is_not_taken`: repo real, `git reset --hard` con un hook `reference-transaction` que en `committed` escribe un marcador y se bloquea leyendo un FIFO (sin `sleep`). El test espera el marcador; ahí el contenido ya está revertido y `RepoMarks` con `SystemProcLister` da `settle(ZERO) == None` y `observe` da `NotCalm`. Se libera el FIFO, `git` termina y se publica el lote. Después, `Some(m)` con `m` ≥ la `seq` del Reset. En rojo.
- C21 cubre D16 completo: la línea parcial no se publica hasta su `\n`, y una línea completa sin evento (`checkout` de la misma rama) llega a la calma sin `Reset`.
- C23 `excluded_pids_and_own_hook_do_not_block_their_own_settle`: tabla sintética con `git` pid 42 en el ámbito más un hook en vuelo. Con `pids = [42]` y `own_hook` = ese token: `Some`. Sin exclusiones: `None`. Con otro hook en vuelo: `None` (D18).
- C24 `a_hook_prior_with_its_live_git_in_scope_seals_with_a_mark` (bloqueante): `hook_prior` con el `git` cliente vivo en el ámbito y `chain` = sus ancestros sella con marca, sin `TimeLimit`. Con `chain = None`: `TimeLimit` → denegación.
- C25 `a_git_dash_c_from_another_cwd_counts_in_scope`: `cwd` fuera y argv `-C <wt>` → `None`; argv ilegible → `None`; argv sin redirección → `Some`.
- C26 `long_lived_git_daemons_do_not_block_the_calm`, con un caso por proceso: `fsmonitor_daemon_is_excluded`, `cat_file_batch_is_excluded`, `credential_cache_daemon_is_not_named_git` (sus `exe` reales en la tabla sintética).
- C27 `#[ignore] one_calm_pass_with_800_processes_is_cheap`: p95 de una pasada con `SystemProcLister` en macOS, en release. Imprime el valor; si pasa de 5 ms, se activa el caché de D21.
- C28 `the_process_probe_runs_only_when_cheap_checks_say_calm`: un `ProcLister` contador con `index.lock` presente → 0 llamadas a `list_bare`.
- C29 `a_settle_blocked_by_git_logs_pid_and_subcommand_only`: logger de captura. `tm_calm_blocked_by_git` sale una vez con `pid` y `status`, sin el resto del argv.
- C15 (e2e) `raw_git_undo`: 50 de 50 bajo carga (macOS: el test es `cfg(target_os = "macos")`).

## Convenciones observadas

- Marcas: primero `seq` y después lo leído (`daemon/repos.rs:observed` L497-501).
- Lectura del directorio común (`continuous.rs:branch_tips`) y errores de vista (`watch/repo.rs:RefsReadError`).
- Tests de calma con `tempdir` y límites de `ZERO`/30 ms (`engine.rs:tests::a_worktree_is_calm_when_the_engine_read_its_head_reflog`).
- Tests de contrato en su propio archivo (R4). Git aislado como en `tests/us_tmc_009.rs:git`.

## Inventario de reutilización

- Reutiliza: `RepoReader::{open,local_branches}`, `RepoMarks::{set_head_logs,set_heads,touch,settle,remove}`, `ShutdownHandle::wake`, `external_events_in`, `Oplog::undo_stack_in` y el fixture de tiers de los tests del daemon.
- Genera: ninguno.
- Crea nuevo: `read_local_branches` (code-search "local_branches helper refs view": solo el bucle inline de `RefsView::read`), `RepoMarks::set_persisted` (code-search "persisted branches marks": nada).

## Supuestos

- Packed-refs: `local_branches` de gix lee loose y packed, así que `git pack-refs` no cambia el mapa (C1b). El backend reftable sigue siendo un límite declarado (ADR-TMC-004).
- Solo cuentan las ramas locales. Los remotos y los tags no son eco de una operación TM.
- Repo dormido: hoy `settle` corre antes de cualquier despertar (`restore.rs:435`, `undo.rs:732`, `conn.rs:3323`), y D7 lo corrige. El despertar es asíncrono, así que `settle` espera al primer lote del repo despierto dentro del límite. Si no llega, `repo-busy` (C7).
- Coste: una apertura más un listado por sondeo de 10 ms solo mientras espera; la comparación es de `BTreeMap`. Presupuesto en C9.
- Plataformas: misma lógica en Linux, macOS y Windows. CI en ubuntu; macOS y Windows quedan pendientes en máquina real.
- Seguridad: no hay rutas nuevas desde el contenido del repo y los nombres de rama solo se comparan. Va a `security-expert` (NFR-01).

### Enmienda ADR-TMC-003 § 4 (esquema)

"Enmienda (2026-10-21, fix/tm-restore-calm-mark)":
1. La Marca en calma exige además que las ramas locales del directorio común sean las del último lote persistido, comparadas bajo el mismo lock que su `seq`.
2. Por qué: el reflog se escribe antes que el ref, y `branch -D` y el aplicador no escriben el reflog.
3. Falla cerrado si el repo no se lee.
4. Límite declarado: la barrera es por repo. Con otro worktree moviendo ramas cada menos de X_min ms (medido en C6, se anota el valor), el `settle` puede agotar 2 s y dar `repo-busy`.

5. Marca confirmada (caso 2): la marca de una captura vale solo si en su punto de validez el motor sigue en calma con la misma `seq`. Si no, la captura se descarta (las capturas reintentan; el prior rechaza con `git-busy`). Así ninguna captura guarda contenido posterior a un evento con una marca anterior a ese evento.

6. Git en curso (caso 3): la calma exige además que no viva ningún `git` en el ámbito (lista de procesos del usuario; los hijos del daemon quedan fuera; si no se puede leer, falla cerrado). La razón: un `git` cambia el contenido antes de escribir su reflog (`reset --hard`: índice → `ORIG_HEAD` → `HEAD`). Límite declarado: un `git` largo de solo lectura aplaza capturas y operaciones. En una plataforma sin `cwd` de procesos, D15 queda inactivo (Windows pendiente). Se corrige la frase de ADR-TMC-004 § 2 "todo `git` terminado…": la calma por reflog solo cubre `git` terminados.
7. Reflog: se publican solo líneas completas (D16).

ADR-TMC-004 § 2: se añade "y las ramas locales" a la fila de Calma por estado. ADR-TMC-004, fila Consistencia: "la marca se confirma en el punto de validez (calma + `seq` sin moverse); si no, descarte". Se cita la compatibilidad con la regla del punto previo de #272.

## Decisiones abiertas

Ninguna. No hay cambio de contrato público ni de datos.

## Not Built (deferred)

- Excluir de D15 los `git` de solo lectura (`log`, `show`, `diff`, `status`… por argv): cuando el bloqueo por paginador o editor se vea en uso real. Hoy cuenta cualquier `git`.
- D15 en Windows (`cwd` de otro proceso): cuando se valide en máquina real. Hasta entonces queda inactivo y se registra, D18 lo mitiga y el hueco es un ⚠️ RIESGO NFR-01 abierto (release-plan, Huecos, fila 12).

- Barrera por worktree (solo las ramas de su ámbito): cuando C6 muestre `repo-busy` con la cadencia real de los agentes.
- Retirar `await_moved_branches`: cuando C4 y us_tmc_009 lleven 2 semanas en verde. Su falso positivo (cualquier evento ajeno posterior con el mismo nombre de rama) no puede sobrevivir a la retirada, ni copiado ni en un sustituto.
- Calma con el backend reftable, y remotos y tags en la barrera: cuando se soporten o una operación TM los mueva.

## Stubs (para que compilen los tests en rojo)

- `pub fn read_local_branches(reader: &RepoReader) -> Result<BTreeMap<String, String>, RefsReadError> { Ok(BTreeMap::new()) }` en `watch/mod.rs`, con `RefsReadError` como `pub`.
- `pub fn set_persisted(&self, _repo_id: &str, seq: i64, _common_dir: &Path, _branches: BTreeMap<String, String>) { self.set_seq(_repo_id, seq) }` en `RepoMarks`.
- `pub fn confirm_mark(_engine: Arc<dyn EngineLink>, _repo_id: &str, _worktrees: &[PathBuf], _mark: i64) -> ValidityGuard { ValidityGuard(Arc::new(|| true)) }` en `engine.rs`. Con este stub C10, C11, C13 y C14 compilan y fallan.
- Hook de depuración `CaptureDeps.after_mark: Option<Arc<dyn Fn() + Send + Sync>>` (D14). Se añade con `None` en todos los constructores y se llama en `observe` tras `settle`, solo en debug. Es necesario para que compilen C10, C11 y C17.
- `pub fn set_procs(&self, _procs: Arc<dyn crate::detect::procs::ProcLister>) {}` en `RepoMarks` (stub vacío: C19, C20 y C22 compilan y fallan). Si los tests de integración necesitan el módulo `detect::procs` y no es `pub`, se exporta `ProcLister`, `ProcEntry` y `SystemProcLister`. C21 no necesita stub.
- `CalmExclusions`, `HookToken` y `HookInFlight` con sus campos. `settle_excluding` delega en `settle` ignorando `ex`. `hook_begin` devuelve un `HookInFlight` inerte. Así C23, C24 y C26 compilan y fallan.
