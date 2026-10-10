---
id: BRIEF-US-TMC-005
title: "Brief de implementación — US-TMC-005: snapshot `previo_hook` pedido desde los hooks de Guardrails"
type: dev-brief
status: draft
created: 2026-10-09
author: rust-architect
story: US-TMC-005
related:
  adrs: [ADR-TMC-002, ADR-TMC-004, ADR-TMC-005, ADR-TMC-006, ADR-GRD-001, ADR-GRD-002, ADR-GRD-003, ADR-GRP-016]
  stories: [US-TMC-005, US-GRD-017, US-MCP-008, US-TMC-004]
contract: docs/dev-briefs/us-tmc-005-pre-hook-snapshot.contract.json
---

# Brief de implementación — US-TMC-005

## Contexto y objetivo

Con los hooks de Guardrails instalados, una operación destructiva de Git crudo que la capa de hooks intercepta **antes de sus efectos** queda precedida de un snapshot de nivel `previo_hook`. El daemon lo toma dentro de la misma llamada `guard.evaluate` y responde `complete` o `failed` dentro de un tiempo máximo. Sin hooks, nada cambia: la cobertura sigue siendo por observación (US-TMC-004). El mismo punto de entrada sirve a US-GRD-017, que solo convierte `failed` en `deny snapshot-failed`.

## Inventario del código (`main` en `e9dfa23b`)

### Convenciones observadas

- El nivel ya existe: `crates/core/src/timemachine/oplog/model.rs:SnapshotLevel::HookPrior` (`"hook-prior"`) y `Channel::Hook`; el timeline lo traduce en `crates/core/src/timemachine/timeline.rs:protection` a `ProtectionLevel::HookPrior` ("snapshot previo" en `apps/cli/src/commands/timeline.rs`).
- El escritor del almacén ya trata `HookPrior` como un previo: prioridad en `crates/core/src/timemachine/store/capture.rs:run_capture` (`prior = GuaranteedPrior | HookPrior`), sin tope de 50 MB y con 8 hilos (`write_blobs`, comentario de la línea 1061).
- El precedente a reutilizar es la captura manual: `crates/core/src/timemachine/manual.rs:capture_in_store` (cuota contada en el oplog bajo un cerrojo de registro, fila `pending` antes de leer el worktree, `discarded` si falla, nada borrado), `manual.rs:quota` (ventanas puras), `manual.rs:ManualFloor` (suelo + reserva del previo garantizado), `manual.rs:resolve_worktree` y `manual.rs:expecting_root`.
- La fila manual se abre con `crates/core/src/timemachine/oplog/mod.rs:Oplog::begin_manual_snapshot` y la cuota se lee con `oplog/query.rs:Oplog::manual_quota_input`. El `CHECK` de la tabla `snapshots` (`oplog/schema.rs`, migración 3) solo permite `requester`/`channel`/`worktree_key` en filas `manual`.
- El hash de la cadena incluye todas las columnas de `snapshots` desde el formato 3 (`oplog/chain.rs:FORMAT = 3`): reconstruir la tabla con las mismas columnas conserva los hashes (precedente: migración 3).
- Solo las filas `complete` con ref son puntos: `oplog/query.rs:Oplog::offerable_snapshots`; el timeline elige el punto anterior a un evento con `timeline.rs:Points::before_event` (último por `(engine_mark, seq)`).
- `guard.evaluate` se atiende en el hilo de la conexión, nunca en el bucle: `crates/core/src/channel/conn.rs` (brazo `methods::GUARD_EVALUATE`), con `serve_audited` → `log_decision` → `hook_claims` → respuesta.
- El `git` más cercano al hook identifica la operación: `crates/core/src/guardrails/second_line.rs:nearest_git` → `GitProcess = (pid, start_us)`; precedente de memoria acotada por `git`: `second_line.rs:Decided` (FIFO de 256).
- Solicitante por ascendencia (ADR-TMC-005 § 1): `crates/core/src/channel/requester.rs:resolve` → `Resolution { who, executor_operation, .. }`; un `git` del ejecutor sale con `executor_operation = Some`.
- Dependencias de captura disponibles en la conexión: `crates/core/src/channel/server.rs:ServerCtx::tm_engine: Option<CaptureDeps>`; el registro de repos protegidos: `ServerCtx::guard` (`guardrails/registry.rs:GuardRegistry::get`).
- Cambio de forma = capacidad: `crates/api/src/methods/guard.rs` (`CAP_GUARD_POLICIES`, …); el cliente del hook pide solo toda capacidad que conoce (`crates/api/src/client/mod.rs:accept_capabilities`). `Decision` es `deny_unknown_fields` (`crates/api/src/guard.rs:Decision`): un campo nuevo sin capacidad rompería a un cliente viejo.
- El cliente RPC espera como mucho `CALL_TIMEOUT = 10 s` (`crates/api/src/client/mod.rs`); si vence, `hook.rs:ask_daemon` devuelve `Asked::Failed` y el hook **deniega** con `internal-error`.
- Overrides de prueba solo en debug: patrón `crates/core/src/guardrails/pending.rs:window` + `WINDOW_ENV`; el daemon arrancado bajo demanda solo hereda los de `crates/core/src/client.rs:debug_overrides`.
- i18n: un grupo de claves por archivo (`apps/cli/i18n/{en,es}/<feature>.txt`, `build.rs` los registra solos); el hook escribe en stderr desde `apps/cli/src/guard.rs:hook`.
- Pruebas e2e de Guardrails con daemon real, dispatcher real y agente simulado: `apps/cli/tests/guard_us_grd_008.rs:Machine` (`#![cfg(unix)]`, `GITRAPTOR_PROFILE_DIR`, `raptor-fake-agent`, sin esperas fijas). Pruebas de TM sobre un almacén temporal: `crates/core/tests/tm_common/mod.rs:Env` y `crates/core/tests/us_mcp_008.rs`.
- `timemachine.snapshot` está declarado y no implementado (`crates/api/src/methods/timemachine.rs`, `implemented_by: "US-TMC-005"`); su nombre está congelado en `crates/api/tests/legacy_protocols.rs`.

### Mejoras detectadas

- `capture.rs:capture_locked` ignora `abort` en las capturas previas (`yield_now = !prior && (…)`): un previo no puede cortarse por plazo. Este Brief lo corrige para todos los niveles (§ D5); para el previo garantizado y la observación no cambia nada, porque pasan `&|| false`.
- `Points::before_event` desempata por `seq` entre puntos con la misma marca: una captura por observación registrada justo después de un previo, con la misma marca, oculta el previo en el timeline. Se corrige (§ D10).
- `CALL_TIMEOUT` es privado: no hay forma de comprobar en un test que un plazo del daemon cabe en él. Se hace `pub` (§ D5).
- `timemachine.snapshot` lleva un `implemented_by` que ninguna historia va a cumplir (ver Gaps, G4).

## Decisiones de arquitectura

**Decisión del orquestador (2026-10-04), validada por Arquitecto** (2026-10-09, este Brief). El plan está aprobado por el orquestador con tres condiciones (D13 a D15) y por el PO con ajustes (D16).

### D13. Migración renumerable — Decisión del orquestador (2026-10-04), validada por Arquitecto

- La migración del oplog de D6 **no asume el número 4**. Se añade **al final** de `OPLOG_MIGRATIONS`, como una entrada más de una lista de solo añadir. Ninguna lógica ni ningún test usa su índice.
- Su SQL **empieza** con la línea marcador `-- migration: hook-prior requester`. Los tests la localizan por ese marcador.
- Otra rama (`feat/US-TMC-019-interruption-robustness`) puede añadir también una migración. **Quien mergee segundo renumera**: rebasa y pone la suya detrás de la otra, sin tocar el SQL. Los tests siguen válidos en cualquier posición.
- Red de seguridad, en `crates/core/src/timemachine/oplog/hook_prior_migration_tests.rs`:
  - La secuencia completa se aplica sobre una base nueva.
  - La migración se aplica sobre una base que dejaron las migraciones anteriores, y todas las filas quedan igual columna a columna.
  - Se cumplen los `CHECK` de D6 y la tabla sigue siendo de solo añadir.

### D14. Puerta `.git` de #226 y nada del hook — Decisión del orquestador (2026-10-04), validada por Arquitecto

- **Ningún dato del hook localiza nada.** `EvaluateParams.common_dir` y `repo_id` solo se **comparan** con `GuardRegistry` (`get(repo_id)` con un `common_dir` igual).
- El `common_dir` de la captura es el del registro.
- El worktree es el **cwd del proceso del hook leído por el daemon** (`process_cwd(peer.pid)`), nunca un parámetro. El solicitante es el ya resuelto por ascendencia (D8).
- **La misma puerta que #226**:
  - Se llama a `undo::tm_scope_for(backend, false, Some(&cwd), None)` con el backend del daemon. Su `repo_of(cwd)` debe dar el mismo `repo_id` del registro; si no, `failed` con causa `no-worktree`.
  - Después `capture_in_store` abre el worktree **solo** con `observe::open_registered_worktree(common_dir, root)` (→ `observe::open_worktree`).
  - Una carpeta que el repo no registra, o un worktree cuyo `.git` apunta a otro repo, se rechaza (`NoWorktree`, o `Capture(Read(Untrusted))`) **antes** de escribir la fila `pending` y de leer nada detrás.
  - No queda ninguna fila `hook-prior` ni ningún punto. La decisión de Guardrails no cambia: la operación sigue y la respuesta lleva `failed` con causa `no-worktree`.
- ⚠️ **ASSUMPTION**: el `repo_of` del backend del daemon resuelve el repo por la carpeta sin leer nada detrás de un `.git` que el repo no posee. El implementador lo verifica; si no, usa directamente `open_registered_worktree`, que sí lo garantiza.

### D15. Sin esperas: plazo y reloj inyectables — Decisión del orquestador (2026-10-04), validada por Arquitecto

- `capture_in_store(…, now_ms, deadline: Instant)` y `capture(deps, ask, now_ms, deadline: Instant)` reciben el reloj de la cuota y el plazo. La conexión pasa `wall_now_ms()` e `Instant::now() + hook_prior::deadline()`.
- Los tests pasan instantes del pasado o un plazo ya vencido. Ningún test espera los 5 s ni una ventana de cuota.
- `hook_prior::deadline()` devuelve `HOOK_PRIOR_DEADLINE`, o el override de debug (`GITRAPTOR_TEST_TM_HOOK_PRIOR_DEADLINE_MS`, que el e2e del escenario 3 pone a 0).

### D16. Ajustes del PO — Decisión del orquestador (2026-10-04), validada por Arquitecto/PO

- **Título nuevo** de la historia: "El borrado de ramas y el rebase con Git crudo tienen punto previo cuando el repo usa los hooks de Guardrails".
- **Para / Valor**: se quita "cierra el riesgo R2". R2 sigue **parcialmente abierto** para `checkout -f`, `restore` y `reset --hard` con Git crudo, que no tienen hook previo (ADR-GRD-002) y quedan cubiertos por observación (US-TMC-004).
- **Escenarios**:
  - El escenario 1 pasa a ser el borrado de "feat-x" (redacción de G0).
  - Se **añade un escenario de rebase**: un agente hace rebase con Git crudo y la entrada figura con "snapshot previo". Lo cubre `scenario_1_rebase_entry_shows_hook_prior`, que pasa a nombrarse con el escenario que le asigne el PO al reescribir la historia (el contrato filtra por `scenario_1_`).
- **Cuotas** (D6): quedan como ⚠️ **ASSUMPTION** en `docs/requirements/features/time-machine/business-rules.md`. La causa `quota-exceeded` es **visible en el aviso** del hook (`hookprior.cause.quota-exceeded`).
- **Huecos llevados a US-GRD-017 como preguntas abiertas**:
  - el modo degradado (sin daemon no hay snapshot);
  - el force-push y el borrado remoto (la Time Machine no guarda el remoto);
  - un archivo enorme que supera el plazo. El PO **prefiere la excepción consciente** a un tope de tamaño.
- **`failed` no bloquea hasta US-GRD-017**: en esta historia el hook avisa y deja pasar (D11).

### D1. Un solo punto de entrada: `guard.evaluate`, con la capacidad `guard.prior-snapshot`

- El daemon toma el `previo_hook` **dentro** de `guard.evaluate`, después de decidir y **antes** de escribir la entrada del registro de decisiones. La respuesta lleva `Decision.priorSnapshot` (`complete` o `failed`).
- **Por qué**: ADR-GRD-003 § 7 exige "en la misma llamada" para US-GRD-017, y la denegación `snapshot-failed` tiene que quedar en el registro. Con un método aparte habría dos viajes, dos registros y una carrera entre la decisión y el snapshot.
- El "comando de la CLI" de ADR-TMC-004 § 3 es `raptor hook`, que ya invoca el dispatcher. **No** se crea `raptor tm hook-snapshot` ni se implementa `timemachine.snapshot`: sería otra vía para pedir puntos `previo_hook` sin pasar por una evaluación. Requiere una enmienda (slice S4).
- Sin la capacidad, el campo no se sirve y el daemon **no toma** el snapshot. El cliente de `crates/core` pide la capacidad automáticamente.

### D2. Qué hooks piden el snapshot (momento A, ADR-GRD-002 § 1)

Política pura `hook_prior::wants_prior(&Operation) -> bool`:

| Hook / operación | ¿Pide `previo_hook`? | Por qué |
|---|---|---|
| `pre-rebase` (`Operation::Rebase`, también `pull --rebase`) | **Sí** | Corre antes de tocar el working tree. Medido con Git 2.50 (macOS): sin `index.lock`; con `--autostash`, el autostash ya está hecho y `rebase-merge/` ya existe |
| `reference-transaction` `prepared` con **algún borrado** (`new == 0`) de `refs/heads/*` (los *prunes* de `pack-refs` ya los filtra el cliente) | **Sí** | Borrar una rama no toca el working tree: el estado es coherente. La rama sigue legible en `prepared` y la captura ancla su punta (`Meta.branches`) |
| `reference-transaction` `prepared` sin borrado (creación, fast-forward, movimiento no fast-forward) | No | El mismo hook llega **después** de reescribir el working tree en `checkout -B`, `reset --hard` o un merge fast-forward (momento B/C). Un snapshot mezclaría el working tree nuevo con las refs viejas y mentiría como "previo" (BR-TMC-CONS-003). Lo cubre la observación |
| `pre-push` (push, force-push, borrado remoto) | No | No modifica el repo local; la Time Machine no guarda el remoto |
| `pre-commit`, `commit-msg`, segunda línea | No | Un commit no destruye nada |
| Consultas (`status`, `log`, `diff`, `branch --list`, `fetch`…) | No | No ejecutan hooks gobernados, o solo refs no gobernadas (vía rápida sin daemon) |

### D3. Coste: un snapshot por comando de Git

- Clave = `git` más cercano (`nearest_git`). Un borrado produce dos `prepared` (`packed-refs` y la ref suelta), y `git branch -D a b` puede producir más: **un solo** snapshot y una sola fila. Las invocaciones siguientes del mismo `git` reutilizan el resultado (`reused: true`), también un `failed`: no hay reintento dentro del mismo comando, así el coste máximo es un plazo.
- Memoria: `HookPriorBook`, FIFO de 256 `git` por almacén (como `second_line::Decided`). Si `nearest_git` no resuelve (`None`), no se deduplica: cada llamada cuenta para la cuota.
- **No se salta con el working tree limpio**: el snapshot también ancla las refs (la rama que se borra). El camino rápido de reutilización (ADR-TMC-004 § 1) lo deja en una fila.

### D4. Cuándo no se toma

Sin campo en la respuesta, en este orden: (1) la conexión no tiene `guard.prior-snapshot`; (2) `!wants_prior(op)`; (3) `applied_effect != allow`; (4) el `git` lo lanzó el ejecutor del daemon (`Resolution.executor_operation.is_some()`): ya tiene `previo_garantizado` (ADR-GRD-003, Enmienda Cockpit).

Con campo `failed`: el repo no está en `GuardRegistry` con ese `common_dir`, o `tm_engine` es `None` → `unavailable`.

### D5. Plazo: `HOOK_PRIOR_DEADLINE = 5 s`, y el plazo siempre gana

- Se mide desde que la conexión empieza a atender el snapshot. Cubre la espera de calma del motor, la cuota, la captura y el registro.
- **Fuente**: ADR-TMC-006 § 2 (objetivo p95 < 200 ms; el delta de 1.000 archivos, ~367 ms) es el objetivo. El techo lo fija el cliente: `CALL_TIMEOUT = 10 s` incluye la evaluación (< 100 ms, ADR-GRD-002 § 5) y deja más de 4 s de margen. Si el plazo se acercara a 10 s, el hook denegaría con `internal-error`.
- `capture_locked` consulta `abort()` en todos los niveles: `yield_now = (!prior && (prior_waiting || give_way)) || abort()`. Así, un previo de hook se corta en el siguiente punto de cesión, también a mitad de un blob grande.
- `ValidityGuard` = `Instant::now() < deadline && GitState::read(root) == at_start`. Se evalúa **una vez**, justo antes de la fila `pending` del punto de validez. Lo que llega tarde se descarta: nunca hay una fila `complete` después de responder `failed`.
- `Yielded` o `Discarded` con el plazo vencido → `TimeLimit`; sin vencer → `Discarded`.
- Override de prueba (solo debug): `GITRAPTOR_TEST_TM_HOOK_PRIOR_DEADLINE_MS`, leído por `hook_prior::deadline()` y añadido a `client.rs:debug_overrides`.
- Un archivo nuevo de 1 GB (~7 s) agota el plazo: `failed` declarado (Gaps G2).

### D6. Cuota SEC-TMC-12 "por repo y por cliente", contada en el oplog

- Las mismas ventanas que la captura manual, con sus propios límites (⚠️ **ASSUMPTION**, a validar por el PO): por solicitante y worktree, **10 por minuto** y **120 en 24 h**; por worktree, **300 en 24 h**; por repo, **1.000 en 24 h**. Cuenta todo intento que llegó a captura (fila `pending`, también `discarded`). Una reutilización (D3) no cuenta.
- **Cliente** = solicitante resuelto (D8). Todos los "sin atribuir" comparten un cubo (`requester_session IS NULL`).
- **Cupos globales solo para agentes (Q-GRD-37, DS-US-GRD-017 D13; ajuste del coordinador, 2026-10-09)**: las ventanas por worktree y por repo solo cuentan, y solo se aplican, a solicitantes que son agentes (fila con `requester_session` no nula). Un solicitante "sin atribuir" solo gasta y solo se mide contra su cupo propio por solicitante (10/min, 120/24 h). `hook_prior_quota_input` filtra `requester_session IS NOT NULL` en `worktree_ms` y `repo_ms`, y para "sin atribuir" la comprobación ignora esas dos ventanas. Sin migración nueva. Test fuera del contrato: un "sin atribuir" no consume el techo por repo de los agentes, y con el techo de agentes lleno puede tomar su previo.
- **Con la cuota llena**, el resultado es `failed` con causa `quota-exceeded`. Nunca se borra un punto.
- **Migración del oplog** (renumerable, D13; en `main` sería la cuarta): reconstruye `snapshots` con las mismas columnas y en el mismo orden (los hashes del formato 3 siguen verificando). Copia la plantilla de la migración 3: primera línea `-- migration: hook-prior requester` (D13), quitar los triggers, `CREATE TABLE snapshots_hook_prior` (un nombre sin número), copiar, renombrar e índices y triggers de nuevo. Los `CHECK` nuevos son:

  ```sql
  CHECK ((level = 'manual') = (label IS NOT NULL)),
  CHECK (level <> 'manual' OR (requester IS NOT NULL AND requester_session IS NOT NULL
      AND worktree_key IS NOT NULL AND channel IS NOT NULL)),
  CHECK (level <> 'hook-prior' OR (requester IS NOT NULL AND worktree_key IS NOT NULL
      AND channel = 'hook')),
  CHECK (level IN ('manual', 'hook-prior') OR (requester IS NULL AND requester_session IS NULL
      AND worktree_key IS NULL AND channel IS NULL))
  ```

  Índices parciales `WHERE level = 'hook-prior'`: `(requester_session, recorded_ms)`, `(worktree_key, recorded_ms)` y `(recorded_ms)`.
- API del oplog: `Oplog::begin_hook_prior_snapshot(&NewSnapshot, &HookPriorMeta) -> Result<String>` y `Oplog::hook_prior_quota_input(session: Option<&str>, worktree_key: &str, now_ms: i64) -> Result<QuotaInput>`. El cubo del solicitante se filtra con `requester_session IS ?1`, que es seguro con `NULL`.
- `capture.rs` generaliza la fila previa: `run_capture(…, begun: Option<Attempt<'_>>, …)` con `enum Attempt<'a> { Manual(&'a ManualMeta), HookPrior(&'a HookPriorMeta) }`, y `SnapshotStore::capture_hook_prior_until(oplog, req, meta, abort)`.

### D7. Disco: nunca desplaza al previo garantizado

Usa `ManualFloor`: el suelo de SEC-TMC-12 más la reserva del previo garantizado, calculada con `cached_reserve`. Se comprueba bajo el cerrojo de registro y otra vez en `abort` durante la captura. Por debajo del suelo → `failed` con causa `no-space`. Sin tope de 50 MB por archivo, como ya hace el escritor con `HookPrior`.

### D8. Solicitante (ADR-TMC-005 § 1)

- `channel::requester::resolve(peer, checks, Some(&marks))`. Un resultado `Err(Unverified)` cuenta como `Requester::Unattributed`.
- Atribuir solo pone un nombre a la fila y elige el cubo de cuota: no da ningún permiso. Por eso se acepta una atribución débil donde no hay prueba de terminal.
- Se congela en la fila con `channel = 'hook'`.

### D9. Coherencia sin rechazar "operación en curso"

- A diferencia de la captura manual, **no** se rechaza `operation_in_progress`: un `rebase --autostash` ya tiene `rebase-merge/` en `pre-rebase`, y rechazarlo daría `failed` en todos.
- Tampoco se rechaza un `index.lock` ajeno: el índice en disco es el anterior, y la guarda de D5 descarta la captura si cambia.
- Se mantiene la verificación del worktree: `resolve_worktree`, `open_registered_worktree(common_dir, root)` (#223 I-03) y `expecting_root`. El `git` del hook está detenido esperando la respuesta, así que su estado es estable por construcción.

### D10. Desempate en el timeline

En `Points::new`, se ordena por `(engine_mark, rango, seq)`, con rango 0 para la observación y la captura manual y rango 1 para `GuaranteedPrior` y `HookPrior`. Con la misma marca, gana el previo: es la misma foto y la promesa más fuerte. Esto toca `timeline.rs` (de la Time Machine).

### D11. Qué hace el hook con `failed` en US-TMC-005

- **Deja pasar** la operación (código de salida sin cambios) y escribe en stderr un aviso i18n con la causa.
- Con `complete` no escribe nada: así la salida del agente no lleva ruido.
- La denegación es de US-GRD-017: en el daemon, entre el snapshot y `log_decision`, convierte `failed` en `system_deny(Rule::SnapshotFailed)`. Con este diseño es un cambio de pocas líneas, sin tocar el cliente.

### D12. Ubicación del motor

La lógica vive en `crates/core/src/timemachine/hook_prior.rs`, de la Time Machine. La conexión solo resuelve el `peer` (cwd, `nearest_git`, solicitante) y llama a `hook_prior::capture`. El estado por almacén (`HookPriorState`: cerrojo de registro propio y `HookPriorBook`) vive en `SnapshotStore`, como `ManualState`. No se añade ningún campo a `ServerCtx`.

## Contratos

### `crates/core/src/timemachine/hook_prior.rs` (nuevo; stub ya escrito, ver "Stubs")

```rust
pub const HOOK_PRIOR_DEADLINE: Duration = Duration::from_secs(5);
pub const DEADLINE_ENV: &str = "GITRAPTOR_TEST_TM_HOOK_PRIOR_DEADLINE_MS";
pub const PER_MINUTE: usize = 10;          // per requester and worktree
pub const PER_DAY: usize = 120;            // per requester and worktree
pub const PER_WORKTREE_DAY: usize = 300;   // everybody, per worktree
pub const PER_REPO_DAY: usize = 1_000;     // everybody, per repo

pub fn deadline() -> Duration;                       // DEADLINE_ENV only with debug_assertions
pub fn wants_prior(op: &gitraptor_api::guard::Operation) -> bool;   // D2, pure
pub fn quota(input: &manual::QuotaInput, now_ms: i64) -> Result<(), manual::QuotaHit>; // D6, pure

pub struct HookPriorAsk {
    pub repo_id: String,
    pub worktree: PathBuf,          // canonical cwd of the hook process
    pub common_dir: PathBuf,        // from GuardRegistry, never from the client
    pub requester: Requester,
    pub git: Option<GitProcess>,    // dedup key (D3)
}
pub struct HookPriorTaken { pub snapshot_id: String, pub worktree: PathBuf, pub reused: bool }
pub enum HookPriorError {
    Quota(QuotaHit), NoSpace, TimeLimit, Discarded, NoWorktree, Unavailable, Capture(CaptureError),
}

#[allow(clippy::too_many_arguments)]
pub fn capture_in_store(store: &SnapshotStore, oplog: &Mutex<Oplog>, ask: &HookPriorAsk,
    engine_mark: Option<i64>, include_credentials: bool, floor: Option<&ManualFloor<'_>>,
    now_ms: i64, deadline: Instant) -> Result<HookPriorTaken, HookPriorError>;
pub fn capture(deps: &CaptureDeps, ask: &HookPriorAsk, now_ms: i64, deadline: Instant)
    -> Result<HookPriorTaken, HookPriorError>;   // D15: clock and deadline injected
```

**Orden de `capture_in_store`**:

1. Buscar en el book por `ask.git`; si hay resultado, devolverlo con `reused: true`.
2. Resolver y verificar el worktree.
3. Tomar el cerrojo de registro del previo de hook hasta `deadline` (si no se libera, `TimeLimit`).
4. Bajo el cerrojo: cuota (`hook_prior_quota_input` + `quota`), suelo y fila `pending` con `HookPriorMeta`.
5. `capture_hook_prior_until` con la guarda de D5 y `abort` = plazo o suelo.
6. Mapear el error, guardar el resultado en el book y devolverlo.

En el paso 2, el worktree que el repo no registra o cuyo `.git` no posee devuelve `NoWorktree` o `Capture(Read(Untrusted))` antes de la fila `pending` (D14).

**`capture(deps, …, deadline)`** llama a `deps.engine.settle(repo_id, [worktree], restante)`; si devuelve `None`, es `TimeLimit`. Después llama a `capture_in_store` con `include_credential_files(&deps.profile)` y `deps.free_space_floor`.

### Oplog (`oplog/model.rs`, `oplog/mod.rs`, `oplog/query.rs`, `oplog/schema.rs`)

```rust
pub struct HookPriorMeta { pub requester: Requester, pub worktree_key: String, pub requested_ms: i64 }
```

El canal es siempre `Channel::Hook`, sin campo. `SnapshotRecord` gana `pub hook_prior: Option<HookPriorMeta>`, que se lee como `manual` en `query.rs`.

### Wire (`crates/api`)

- `methods/guard.rs`: `pub const CAP_GUARD_PRIOR_SNAPSHOT: Capability = Capability::new("guard.prior-snapshot");`, que se añade a `GROUP.capabilities`.
- `timemachine.rs` (de la Time Machine):

  ```rust
  /// The `hook-prior` snapshot `guard.evaluate` took (capability `guard.prior-snapshot`).
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
  #[serde(tag = "outcome", rename_all = "kebab-case", deny_unknown_fields)]
  pub enum HookPriorSnapshot {
      /// Ref in the store and `complete` row exist before this answer.
      Complete { #[serde(rename = "snapshotId")] snapshot_id: String, reused: bool },
      /// No point was recorded as `hook-prior` for this request.
      Failed { cause: HookPriorFailure },
  }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
  #[serde(rename_all = "kebab-case")]
  pub enum HookPriorFailure { TimeLimit, NoSpace, QuotaExceeded, Discarded, NoWorktree, Unavailable, Internal }
  ```

- `guard.rs`: `Decision` gana `#[serde(default, skip_serializing_if = "Option::is_none")] pub prior_snapshot: Option<crate::timemachine::HookPriorSnapshot>` (en JSON, `priorSnapshot`). Todos los constructores de `Decision` lo inicializan a `None`.
- `client/mod.rs`: `CALL_TIMEOUT` pasa a `pub`.
- **Sin códigos de error nuevos**: `failed` es un resultado, no un error RPC.

### Conexión (`crates/core/src/channel/conn.rs`)

En el brazo `GUARD_EVALUATE`, justo después de `serve_audited` y **antes** de `commit_decisions`/`log_decision`:

```rust
decision.prior_snapshot = self.hook_prior(&p, &decision);
```

`fn hook_prior(&self, p: &EvaluateParams, d: &Decision) -> Option<HookPriorSnapshot>` va junto a `hook_claims` y aplica D4. Monta `HookPriorAsk` así:

- `worktree`: `process_cwd(self.peer.pid)` canonicalizado.
- `common_dir`: de `self.ctx.guard.get(&p.repo_id)` con un `common_dir` igual.
- `requester`: D8.
- `git`: `nearest_git(self.peer, &self.ctx.checks())`.

Llama a `hook_prior::capture` y mapea el error a `HookPriorFailure`. Registra en el log `hook_prior` con la causa y la duración, sin rutas.

### CLI

- `apps/cli/src/guard.rs:hook`: tras las líneas de la decisión, con `Some(HookPriorSnapshot::Failed { cause })`, escribe `t("hookprior.failed", &[("cause", &t(<clave de la causa>, &[]))])`. El código de salida no cambia.
- `apps/cli/i18n/en/hookprior.txt` (grupo nuevo `hookprior.`):
  - `hookprior.failed = GitRaptor: no prior snapshot was saved before this operation ({cause}); it goes ahead, covered only by observation.`
  - `hookprior.cause.time-limit = it took too long`, `.no-space = not enough disk space`, `.quota-exceeded = too many prior snapshots`, `.discarded = the repository changed while it was saved`, `.no-worktree = no working tree`, `.unavailable = the Time Machine is not available for this repository`, `.internal = internal error`.
  - `es`: `hookprior.failed = GitRaptor: no se guardó un snapshot previo antes de esta operación ({cause}); sigue adelante, cubierta solo por observación.` y las causas traducidas.

## File & Project Topology (slices disjuntos)

| Slice | Archivos | Dueño |
|---|---|---|
| **S1 — contrato API** | `crates/api/src/timemachine.rs` (tipos `HookPriorSnapshot`, `HookPriorFailure`), `crates/api/src/methods/guard.rs` (capacidad), `crates/api/src/guard.rs` (campo `prior_snapshot` + inicializaciones), `crates/api/src/client/mod.rs` (`pub CALL_TIMEOUT`) | rust-expert |
| **S2 — núcleo TM** | `crates/core/src/timemachine/hook_prior.rs` (reemplaza el stub), `crates/core/src/timemachine/store/capture.rs` (`Attempt`, `abort` en todos los niveles, `capture_hook_prior_until`), `crates/core/src/timemachine/store/mod.rs` (`HookPriorState`), `crates/core/src/timemachine/oplog/{model.rs,mod.rs,query.rs,schema.rs,migration_tests.rs}`, `crates/core/src/timemachine/timeline.rs` (D10), `crates/core/src/timemachine/manual.rs` (solo hacer `pub(crate)` lo que se reutiliza: `folder_id`, `cached_reserve`, `VolumeProbe`, `invalid`) | rust-expert |
| **S3 — daemon y hook** | `crates/core/src/channel/conn.rs` (brazo `GUARD_EVALUATE` + `fn hook_prior`), `crates/core/src/client.rs` (`debug_overrides` + `DEADLINE_ENV`), `apps/cli/src/guard.rs` (aviso), `apps/cli/i18n/{en,es}/hookprior.txt` (nuevos) | rust-expert |
| **S4 — docs** | `docs/architecture/decisions/ADR-TMC-004-cobertura-dos-niveles.md` (Enmienda abajo), `docs/architecture/decisions/ADR-GRD-003-motor-decision-contrato.md` (una fila de Enmienda: campo `priorSnapshot` + capacidad), `docs/requirements/features/time-machine/user-stories/US-TMC-005-…md` (título, Para/Valor y escenarios según D16, Requisitos Técnicos, enlace al Brief, `status: implemented` en el PR), `docs/requirements/features/time-machine/business-rules.md` (cuotas como ⚠️ ASSUMPTION, D16), `docs/requirements/features/guardrails/user-stories/US-GRD-017-…md` (preguntas abiertas de D16), `docs/requirements/release-plan.md` (si procede) | orquestador / docs |
| **Tests (ya escritos, rojos)** | `crates/core/tests/us_tmc_005.rs`, `apps/cli/tests/us_tmc_005_hook.rs`, `crates/core/src/timemachine/oplog/hook_prior_migration_tests.rs` | no se editan durante la ejecución (R4) |

- **Orden**: S1 → S2 → S3. S2 y S1 pueden ir en paralelo si S2 no usa los tipos del wire; S3 depende de los dos. S4 es independiente.
- El stub de `hook_prior.rs` y la línea `pub mod hook_prior;` de `timemachine/mod.rs` ya están en el árbol: forman parte de S2.

### Stubs añadidos para que compilen los tests rojos

- `crates/core/src/timemachine/hook_prior.rs`: constantes de D5/D6 con sus valores finales; tipos `HookPriorAsk`, `HookPriorTaken`, `HookPriorError`; `deadline`, `wants_prior`, `quota`, `capture_in_store` y `capture` con `unimplemented!()`.
- `crates/core/src/timemachine/mod.rs`: `pub mod hook_prior;`.
- `crates/core/src/timemachine/oplog/mod.rs`: `#[cfg(test)] mod hook_prior_migration_tests;` (solo declara el archivo de tests rojos de D13).

## Matriz de plataformas

| Plataforma | Estado | Verificación |
|---|---|---|
| macOS | Soportado | `cargo test -p gitraptor-core --test us_tmc_005` y `cargo test -p gitraptor-cli --test us_tmc_005_hook` (e2e con daemon y hooks reales) |
| Linux | Soportado, mismo código | La misma suite en CI ubuntu; el e2e usa `script -e -c` como `guard_us_grd_008.rs` |
| Windows | Mismo código: `folder_id` vía `gitraptor_winsys`, canal por named pipe. **Pendiente de validación** en la máquina real (receta de aceptación interactiva): `raptor guard install` exige la consola de una sesión de escritorio | Compila con clippy `x86_64-pc-windows-msvc`. El e2e es `cfg(unix)`; los tests de núcleo con `Canary` también son `cfg(unix)`, y el resto corre en Windows |
| Modo degradado (sin daemon) | Sin snapshot: no hay campo y el hook ya avisa del modo degradado | Ver Gaps G1 (US-GRD-017) |

## NFRs

- **Rendimiento**: p95 del previo de hook < 200 ms sobre el repo de referencia (ADR-TMC-006). Solo en el banco de release o en la máquina de referencia, **nunca** en un test debug. Techo duro: `HOOK_PRIOR_DEADLINE`, con un test que comprueba `HOOK_PRIOR_DEADLINE * 2 <= CALL_TIMEOUT`.
- **Seguridad**:
  - Ni el repo ni el `common_dir` salen del cliente: se toman del registro.
  - El worktree es el cwd del peer, verificado como worktree registrado del repo.
  - Ni el argv ni las rutas van al log.
  - Sin `git` ni hooks en la captura (gix en proceso).
  - **Marcar para `security-expert`**: cwd del peer, resolución de procesos y migración del oplog.
- **Observabilidad**: evento de log `hook_prior` con `repo` (id), `outcome`, `cause`, `ms` y `reused`.
- **NFR-01**: `complete` solo cuando la ref y la fila existen antes de responder; `failed` nunca deja una fila `complete`.

## Plan de pruebas

Aislamiento: repos, home y perfil temporales (`Fixture`, `GITRAPTOR_PROFILE_DIR`), nunca este repo. Sin esperas fijas: cada estado se espera con un plazo.

| Escenario / criterio | Test | Archivo |
|---|---|---|
| E1 (reformulado): un borrado de rama tiene un punto previo con `api.rs` | `scenario_1_branch_delete_keeps_api_rs_in_a_hook_prior` | `apps/cli/tests/us_tmc_005_hook.rs` |
| E1: el punto figura como "snapshot previo" (timeline de un rebase) | `scenario_1_rebase_entry_shows_hook_prior` | ídem |
| E2: sin hooks, cobertura por observación | **Ya garantizado**, no es criterio: `apps/cli/tests/raw_git_undo.rs`, `apps/cli/tests/continuous_observation.rs` (US-TMC-004). La TM no comprueba hooks (Q22) | — |
| E3: si el previo falla, no figura como previo y el hook avisa y deja pasar | `scenario_3_failed_prior_is_not_shown_as_prior` (plazo 0 por env) | `apps/cli/tests/us_tmc_005_hook.rs` |
| E4: una consulta no genera punto (con control positivo) | `scenario_4_read_only_queries_take_no_point` | ídem |
| Política de disparadores (D2) | `trigger_policy_only_rebase_and_branch_deletion` | `crates/core/tests/us_tmc_005.rs` |
| Plazo → `failed` y sin punto (D5) | `timeout_fails_and_records_no_point` | ídem |
| Cuota (D6) | `quota_refuses_the_eleventh_in_a_minute_and_counts_per_repo` | ídem |
| Sin recursión, un punto por comando (D3) | `one_point_per_git_command_and_no_hook_runs` | ídem |
| Plazo dentro de `CALL_TIMEOUT` | `timeout_deadline_fits_in_the_client_call_timeout` | ídem |
| Puerta `.git` de #226 (D14): núcleo | `untrusted_worktree_takes_no_hook_prior` (carpeta que se hace pasar por un worktree y `.git` reescrito hacia otro repo) | `crates/core/tests/us_tmc_005.rs` |
| Puerta `.git` de #226 (D14): e2e, el hook no se rompe | `untrusted_worktree_hook_takes_no_point_and_the_operation_proceeds` (la operación sigue, ninguna fila; control positivo desde el repo) | `apps/cli/tests/us_tmc_005_hook.rs` |
| Migración renumerable (D13) | `hook_prior_migration_fresh_database_…`, `hook_prior_migration_applies_cleanly_from_the_previous_version` | `crates/core/src/timemachine/oplog/hook_prior_migration_tests.rs` |

Ningún test duerme para esperar el plazo o una ventana de cuota (D15): el reloj (`now_ms`) y el plazo (`Instant`) se inyectan, y el e2e del escenario 3 usa el override de plazo 0. Bajo carga, se corre con `CARGO_BUILD_JOBS=3` y `-- --test-threads=4`. Un e2e que falle por tiempo se repite solo antes de tocar código.

Además, el implementador añade tests unitarios propios para la migración (`migration_tests.rs`: los hashes de filas antiguas verifican con `Oplog::open`), el desempate de D10 (`timeline.rs`), el omitido con el ejecutor (D4) y la serialización de `HookPriorSnapshot`.

**Aceptación**: `cargo clippy --workspace --all-targets -- -D warnings` y `node tools/test/nextest-junit.mjs`.

## No se construye (diferido)

- `raptor tm hook-snapshot` / `timemachine.snapshot` implementado — añadir cuando un cliente que no sea un hook de Guardrails necesite un `previo_hook`.
- Previo en `pre-push` (force-push, borrado remoto) — añadir cuando la Time Machine guarde refs remotas.
- Previo en movimientos no fast-forward de ramas — añadir cuando el hook pueda probar que el working tree no cambió (momento A).
- Tope de 50 MB o previo parcial para que un archivo enorme no agote el plazo — añadir si US-GRD-017 lo exige (G2).
- Deny `snapshot-failed` y excepción consciente — US-GRD-017.
- Cuota configurable en el perfil — añadir cuando el PO pida otros valores.

## Gaps y preguntas abiertas

- **G0 (PO)**: el escenario 1 no se puede cumplir tal como está. Ningún hook corre antes de que `checkout` sobrescriba el working tree (`post-checkout` corre después; `reference-transaction` de `checkout -B`, también). Además, Git rechaza un `checkout` o un `rebase` que pisaría cambios sin commitear (medido con Git 2.50: `rebase` falla antes de `pre-rebase`). Solo `checkout -f`, `checkout -- <ruta>`, `restore` y `reset --hard` los pisan, y ninguno tiene un hook previo (ADR-GRD-002: C). **Redacción propuesta**:

  > **Escenario: Con hooks de Guardrails, la operación de Git crudo tiene punto previo**
  > Dado un repo con los hooks de Guardrails activos
  >   Y la rama "feat-x" con un commit que añade "api.rs" y que no está en ninguna otra rama
  > Cuando un agente borra "feat-x" con Git crudo
  > Entonces existe un punto recuperable anterior al borrado con "api.rs" de "feat-x"
  >   Y ese punto figura como "snapshot previo"

  Se añade una nota: "`checkout` y `reset --hard` con Git crudo no tienen hook previo (ADR-GRD-002); los cubre la observación (US-TMC-004, riesgo R2)". Los tests cubren además el rebase.
- **G1 (US-GRD-017)**:
  - En modo degradado no hay snapshot: decidir si se deniega la operación destructiva.
  - Decidir si force-push y el borrado remoto cuentan como "destructivos" que exigen previo (la TM no puede guardar el remoto).
- **G2 (US-GRD-017)**: con `HOOK_PRIOR_DEADLINE = 5 s`, un worktree con un archivo nuevo de cientos de MB da `failed` siempre. En US-TMC-005 solo es un aviso; con US-GRD-017 bloquearía. Hay dos opciones: el tope de 50 MB con exclusión declarada, o la excepción consciente.
- **G3 (PO)**: los límites de cuota de D6 son ⚠️ **ASSUMPTION**.
- **G4**: `timemachine.snapshot` sigue declarado (nombre congelado por `legacy_protocols.rs`) y responde "no implementado" con `implemented_by: "US-TMC-005"`. Propuesta: dejarlo así en esta historia y retirarlo en el próximo cambio de protocolo. Hay que confirmarlo con el orquestador.
- **G5**: la migración añade una entrada a `OPLOG_MIGRATIONS`, que es una lista compartida. `feat/US-TMC-019-interruption-robustness` puede añadir otra. **Quien mergee segundo renumera** (pone la suya detrás); los tests la localizan por el marcador, no por el índice (D13).
- **G6**: ⚠️ **ASSUMPTION**: el `repo_id` del dispatcher (`GuardRegistry`) es el mismo id de `TmRepos::repo`. El implementador lo verifica.
- **Sin contradicción con ADRs aceptados**: ADR-TMC-004 § 3 dice "comando de la CLI"; D1 lo concreta como `raptor hook` + `guard.evaluate`. Va en la enmienda de abajo.

### Texto de la enmienda (slice S4) — ADR-TMC-004

> ## Enmienda (2026-10-09, US-TMC-005)
>
> **Decisión del orquestador (2026-10-04), validada por Arquitecto** (Brief `docs/dev-briefs/us-tmc-005-pre-hook-snapshot.md`). El `status` sigue en `accepted`.
>
> - **El comando de la CLI del § 3 es `raptor hook`**: el daemon toma el `previo_hook` dentro de `guard.evaluate`, en la misma llamada que decide (ADR-GRD-003 § 7), y responde `Decision.priorSnapshot` = `complete` o `failed`, solo a una conexión con la capacidad `guard.prior-snapshot`. No hay otro método para pedirlo; `timemachine.snapshot` queda declarado y sin implementar.
> - **Cuándo**: solo con `appliedEffect = allow`, fuera del ejecutor, y en hooks de momento A: `pre-rebase` y `reference-transaction` `prepared` con un borrado de `refs/heads/*`. Nunca en `pre-push`, commits, consultas ni movimientos de rama sin borrado (llegan después de reescribir el working tree y el punto mezclaría estados). Un snapshot por comando de Git (el `git` más cercano).
> - **Tiempo máximo**: 5 s desde que el daemon lo atiende, por debajo del tiempo de llamada del cliente (10 s). Si se agota, `failed` y ninguna fila `complete`.
> - **Cuota (SEC-TMC-12)**: por solicitante y worktree, 10 por minuto y 120 en 24 h; 300 por worktree y 1.000 por repo en 24 h. Cuenta cada intento y nunca borra un punto. No consume la reserva del previo garantizado.
> - **Con `failed`** (US-TMC-005): el hook avisa y deja pasar; la denegación es de US-GRD-017.
> - **Escenario 1 de US-TMC-005**: un `checkout` con Git crudo no tiene hook previo (ADR-GRD-002); el escenario se verifica con el borrado de una rama y con un rebase.

## Handoff

`rust-expert`: implementa S1 → S2 → S3 según este Brief, sin editar los tests rojos (`crates/core/tests/us_tmc_005.rs`, `apps/cli/tests/us_tmc_005_hook.rs`), y verifica con el contrato `docs/dev-briefs/us-tmc-005-pre-hook-snapshot.contract.json`. Antes de abrir el PR, `security-expert` revisa la resolución del peer (cwd, `nearest_git`, solicitante) y la migración 4. El orquestador obtiene el visto bueno del PO para G0 y G3 y aplica S4.
