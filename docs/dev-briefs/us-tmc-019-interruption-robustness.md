---
title: "Brief — US-TMC-019: el repo sigue recuperable aunque GitRaptor muera a mitad de un snapshot, un undo o una restauración"
status: draft
created: 2026-10-09
domain: GRP
feature: time-machine
related: [US-TMC-019, ADR-TMC-003, ADR-TMC-002, ADR-GRP-016, INF-TMC-001, INF-GRP-001, TS-TMC-002, US-TMC-013]
tags: [time-machine, robustez, nfr-01, nfr-12, caos]
---

# Brief — US-TMC-019: robustez ante interrupciones

## Objetivo

La recuperación al arrancar (ADR-TMC-003 § 6) ya existe y el arnés de caos (INF-TMC-001) la prueba en cada punto de un undo. Faltan dos piezas para cerrar la historia: (1) **puntos de caos en una captura que no es el previo garantizado** (escenario 1: un snapshot cortado no cuenta y el worktree no cambia) y (2) **la entrega del aviso pendiente** al siguiente cliente del worktree (escenarios 2 y 6). El aviso ya se registra en el oplog, pero ningún cliente lo recibe. Lo demás ya está construido y queda fijado con tests de regresión.

## Survey

| Escenario | Qué existe hoy | Qué falta |
|---|---|---|
| 1 Snapshot cortado | La recuperación descarta los `pending` y borra su ref (`crates/core/src/timemachine/oplog/recovery.rs::Oplog::recover_snapshots`). Hay puntos de caos solo para el previo garantizado (`store/capture.rs::record`, `chaos::PRIOR_PENDING` y `PRIOR_REF`), probados en `apps/cli/tests/tm_chaos.rs::crash_at_prior_*` | Puntos `capture:pending` y `capture:ref` en la captura por observación o previa de hook, y su test |
| 2 Undo o restauración cortados | El undo cortado en cada punto se recupera (`tm_chaos.rs::crash_scenario`). La restauración pasa por el mismo aplicador y los mismos puntos; se verificó en el survey que se recupera (test temporal, ya borrado). La recuperación registra un aviso por worktree (`recovery.rs::recover_operations`) y existen `Oplog::pending_notices` y `Oplog::mark_notice_delivered` (`oplog/query.rs`, `oplog/mod.rs`) | **Ningún método ni cliente entrega el aviso.** Solo lo lee un test (`crates/core/tests/daemon_lifecycle.rs:383`) |
| 3 Fallo detectado a mitad | `apply/mod.rs::Applier::apply` deja la operación en `interrupted` sin vuelta atrás (`ApplyError::Interrupted`). La CLI dice "run raptor undo again…" (`apps/cli/i18n/en/timemachine.txt: undo.interrupted`, `restore.interrupted`) y el timeline muestra `interrupted` (`timeline.rs`, `commands/timeline.rs`). En Windows hay un test del aplicador con un editor que bloquea el archivo (`crates/core/tests/tm_apply.rs::a_file_open_in_an_editor_interrupts_and_undo_recovers_once_closed`) | Nada de producción. Faltaba un test de proceso en Unix: añadido y **en verde** (`tm_interruption.rs::s3_*`) |
| 4 Lock propio frente a ajeno | `recovery.rs::release_locks` + `release_own_lock` (identidad inodo + fecha de creación). Probado en proceso (`daemon_lifecycle.rs`) y con caos (`tm_chaos.rs::crash_at_apply_*`, que exige que no queden locks) | Nada de producción. El caso combinado de dos worktrees: añadido y **en verde** (`tm_interruption.rs::s4_*`) |
| 5 Undo sin atribuir tras recuperar | `tm_chaos.rs::crash_scenario`: `raptor undo` (solicitante sin atribuir) devuelve S1 y después S0. Lo mismo tras una restauración cortada: lo ejercita `s2_a_restore_*` | Nada. Ya cubierto |
| 6 Reinicio limpio | `daemon_lifecycle.rs`: `recovery.is_clean()` tras un cierre ordenado | Comprobar que el cliente **no** recibe aviso, lo que exige el método de entrega; el historial y la huella se comparan antes y después |

### Convenciones observadas

- Los puntos de caos son nombres estables de una lista cerrada, detrás de la feature `chaos` y con muerte por `SIGKILL`: `crates/core/src/timemachine/chaos.rs::{POINTS, crash_point, die}`.
- La versión de `crash_point` sin la feature tiene que ser vacía, con este texto exacto. Lo comprueba `crates/core/tests/tm_chaos_gate.rs::without_the_feature_the_crash_point_is_empty`.
- `tm_chaos.rs::every_crash_point_has_a_scenario` exige que `POINTS` sea exactamente el conjunto de puntos de un undo.
- Un snapshot vale solo con la fila `complete` y su ref. La fila `pending` va antes de la ref: `store/capture.rs::record`.
- La puerta del `.git` que el repo no posee está en `backend.repo_of`, al que se llega por `crates/core/src/timemachine/undo.rs::tm_scope_for`. La usa `channel/conn.rs::tm_timeline` y la fija `crates/core/tests/tm_untrusted_git_channel.rs`.
- Un método nuevo va en el archivo de su módulo y lleva `.since(CAPABILITIES_PROTOCOL)`. Lo exige `crates/api/tests/legacy_protocols.rs::protocol_9_adds_only_the_capability_handshake`, aunque la guía ADR-GRP-016 dice "sin `.since`" (ver Mejoras). Ejemplo: `crates/api/src/methods/discovery.rs::GROUP`.
- `RequestChannel` se convierte en el `Channel` del oplog con `crates/core/src/executor/mod.rs::oplog_channel`.
- Un cliente de la CLI comprueba si el daemon ofrece un método con `apps/cli/src/support.rs::offers`. Un daemon antiguo sin el método no rompe nada.
- Los mensajes de la CLI van en `apps/cli/i18n/{en,es}/timemachine.txt`, cada grupo de claves en un solo archivo (`build.rs` y los tests de i18n).
- Los tests de proceso arrancan el `raptor` real con `env_clear`, un perfil temporal y `GITRAPTOR_AGENT_EXECUTABLES=raptor-fake-agent`: `apps/cli/tests/tm_chaos.rs::Machine`.

### Mejoras detectadas

- `docs/architecture/extender-sin-archivos-compartidos.md` § "Añadir un método" dice "Sin `.since(...)`", pero `legacy_protocols.rs` exige `.since(CAPABILITIES_PROTOCOL)` y añadir el nombre a `AFTER_FREEZE_FULL`. Hay que corregir la guía (fuera de este brief; anotarlo en el PR).
- La sonda de mayúsculas y minúsculas del aplicador (`apply/mod.rs::check_tree`, `probe_folding`) cambia el mtime de la raíz del worktree aunque no cambie ningún archivo. No es una pérdida de datos, pero la huella de INF-GRP-001 lo detecta: los tests miden la huella después de la muerte, no antes del undo.
- El riesgo residual de ADR-TMC-003 (avisar cuando un lock propio se conserva por identidad desconocida) sigue pendiente: diferido.

## Diseño

Solo las piezas que faltan.

### D1 — Puntos de caos de captura (cubierto por ADR-TMC-003 Validación 2 e INF-TMC-001 § 6)

En `crates/core/src/timemachine/chaos.rs`:

```rust
/// Crash points of a capture that is not a guaranteed prior (observation, hook prior), in the
/// order it reaches them. Apart from [`POINTS`], which are an undo's.
pub const CAPTURE_POINTS: &[&str] = &[CAPTURE_PENDING, CAPTURE_REF];
/// Row `pending` of a capture, no ref yet.
pub const CAPTURE_PENDING: &str = "capture:pending";
/// Ref of a capture created, row still `pending`.
pub const CAPTURE_REF: &str = "capture:ref";

/// Whether `name` is a crash point of any list.
pub fn is_point(name: &str) -> bool { POINTS.contains(&name) || CAPTURE_POINTS.contains(&name) }
```

- En la versión con la feature, `crash_point` usa `is_point` en lugar de `POINTS.contains` (en el `debug_assert` y en el `assert` de la variable). **No toques** la versión sin la feature ni `POINTS`.
- En `store/capture.rs::record`: `if prior { PRIOR_PENDING } else { CAPTURE_PENDING }` justo después de `begin_snapshot`, y `if prior { PRIOR_REF } else { CAPTURE_REF }` justo después de `create_ref`.
- `finish_manual` (capturas manuales por MCP) queda sin puntos: ver Not Built.
- Tests unitarios en el `mod tests` de `chaos.rs`: los puntos son únicos entre las dos listas e `is_point` acepta los 14 nombres.

### D2 — Método `timemachine.notices` (Decisión nueva)

ADR-TMC-003 § 6.5 dice que el aviso llega al siguiente cliente que se conecta desde el worktree, pero no fija cómo. **Decisión nueva**: un método propio del módulo `timemachine`, que no cambia la forma de nada que ya existe y por eso no pide capacidad (ADR-GRP-016).

`crates/api/src/methods/timemachine.rs`:

```rust
/// The pending notices of the caller's worktree (US-TMC-019): each one is answered once and
/// marked delivered on the connection's surface.
pub const TM_NOTICES: &str = "timemachine.notices";
// GROUP.methods, after TM_TIMELINE:
time_machine(TM_NOTICES, false, RepoWrite::None, "US-TMC-019").since(CAPABILITIES_PROTOCOL),
```

`crates/api/src/timemachine.rs` (al final, junto a `TimelineParams`):

```rust
/// `timemachine.notices` parameters (US-TMC-019).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoticesParams {
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub surface: Option<Surface>,
}

/// `timemachine.notices` result: what the caller's worktree has not been told yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoticesResult {
    /// The oldest first.
    pub notices: Vec<TmNotice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TmNotice {
    pub notice_id: String,
    pub kind: TmNoticeKind,
    /// The operation the notice is about.
    pub operation_id: Option<String>,
    pub operation_kind: Option<TimelineOperationKind>,
    /// Where `raptor undo` takes the worktree back to.
    pub prior_snapshot_id: Option<String>,
    pub recorded_utc_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TmNoticeKind {
    /// An operation was cut half-way; `raptor undo` returns to its prior snapshot.
    Interruption,
}
```

Forma JSON que fijan los tests: `{"notices":[{"notice_id","kind":"interruption","operation_id","operation_kind":"undo"|"restore"|"redo"|"protected","prior_snapshot_id","recorded_utc_ms"}]}`.

Núcleo, módulo nuevo `crates/core/src/timemachine/notices.rs` (una línea `pub mod notices;` en `timemachine/mod.rs`):

```rust
/// Takes the interruption notices of `worktree` (and those of the whole repo) that no client
/// received, marks each one delivered on `channel` and returns them, oldest first. Under the
/// caller's lock of the oplog, so two clients never both receive one.
pub fn take_interruptions(
    oplog: &mut Oplog,
    worktree: &Path,
    channel: Channel,
    now_ms: i64,
) -> Result<Vec<TmNotice>, ProfileError>
```

- **Ajuste del Arquitecto 1**: la clave del worktree se obtiene con la misma función que escribió el ámbito de la operación (`root_key` / `to_string_lossy` del worktree resuelto), nunca con una cadena rehecha; si no coincide, ningún cliente recibe el aviso. Lee `oplog.pending_notices(Some(<esa clave>))` y filtra `kind == NoticeKind::Interruption`. Los avisos de purga son de US-TMC-016: nunca se entregan aquí, porque eso iniciaría su periodo de gracia (ADR-TMC-007 § 4.2).
- `operation_kind` y `prior_snapshot_id` salen de `oplog.operation(id)` (`OperationView.record.kind` y `.prior_snapshot`). Correspondencia: `Protected → Protected`, `Undo → Undo`, `Redo → Redo`, `Restore → Restore`. `recorded_utc_ms = notice.recorded_ms`.
- **Decisión nueva D3, entrega al menos una vez**: si `mark_notice_delivered` falla, el aviso se devuelve igualmente y se escribe un log de aviso que lleva solo el `notice_id` y el error, sin rutas. Mostrar un aviso dos veces no cuesta nada; perderlo deja al usuario sin saber que su repo está a medias. Riesgo aceptado: si la CLI muere tras recibirlo y antes de imprimirlo, el aviso se pierde; el timeline sigue mostrando `interrupted`.
- **Ajuste del Arquitecto 2**: la entrega se marca por aviso, no por worktree (`first_delivered_ms` es global). Un aviso de repo entero (ámbito sin worktrees) lo recibe solo el primer cliente de cualquier worktree.
- Tests unitarios en `crates/core/src/timemachine/notices_tests.rs` (`#[cfg(test)] #[path = "notices_tests.rs"] mod tests;`): una vez y nunca más; otro worktree no recibe nada; el aviso de repo entero llega al primer cliente de cualquier worktree y a ningún otro después; la purga nunca se entrega.

Canal, `crates/core/src/channel/conn.rs`:
- En el `match` de dispatch, junto a `methods::TM_TIMELINE`, añade el brazo `methods::TM_NOTICES => { let result = self.tm_notices(request); self.reply(&request.id, result); }`.
- `fn tm_notices(&self, request: &Request) -> Result<serde_json::Value, ErrorObject>`, junto a `tm_timeline` y con su mismo patrón: `NoticesParams` → `request_channel(p.surface)` → `named_worktree(p.worktree)` → sin `self.ctx.time_machine`, `NOT_IMPLEMENTED` con `implemented_by: "US-TMC-019"` → **`tm_scope_for(tm.backend.as_ref(), false, named.as_deref(), None).map_err(scope_refused)`**. Es la puerta del `.git`, la única forma de llegar al oplog: nunca abras un oplog por un `repo_id` de los parámetros. Después bloquea `repo.repo.oplog`, llama a `notices::take_interruptions(&mut log, &repo.repo.worktree, oplog_channel(channel), now_ms())` y serializa `NoticesResult`. Un error del oplog devuelve `INTERNAL` "oplog unavailable" sin datos.
- No se ofrece por MCP (`mcp: false`): el despachador ya lo rechaza.

### D4 — Superficies que muestran el aviso (Decisión nueva)

En este corte lo muestran `raptor undo`, `raptor restore` y `raptor timeline`, por stderr y antes de su salida, también con `--json`. TUI y MCP quedan diferidos. Un aviso que nadie recoge sigue pendiente y el timeline muestra igualmente la operación `interrupted` (§ 6.5 "y el timeline": lo cubre el estado de la entrada, no hace falta un campo nuevo).

`apps/cli/src/undo/notices.rs`, nuevo (`pub(crate) mod notices;` en `undo/mod.rs`):

```rust
/// Shows the pending notices of `worktree` on stderr, prefixed by `command`. Silent when the
/// daemon does not offer `timemachine.notices` or the call fails: it never stops the command.
pub(crate) fn show_pending(client: &mut Client, worktree: &Path, command: &str)
```

- Llama a `TM_NOTICES` con `{"worktree": worktree, "surface": "cli"}`. Por cada aviso escribe `eprintln!("{command}: {}", t(key, &[("id", &sanitize(op_id))]))`, con la clave según `operation_kind`: `tm-notice.interrupted-undo`, `-restore`, `-redo` y `-operation` (protegida o desconocida).
- Lo llaman `undo/mod.rs::run` (después de `engine(CMD)`), `commands/restore.rs` y `commands/timeline.rs` (después de su `engine(CMD)`, con el worktree que ya calculan).
- Claves, grupo nuevo `tm-notice.` solo en `timemachine.txt`:
  - en: `tm-notice.interrupted-undo = the undo {id} of this worktree was interrupted when GitRaptor stopped; raptor undo returns to the state before it` (y lo mismo con `the restore`, `the redo` y `the operation`).
  - es: `tm-notice.interrupted-undo = el undo {id} de este worktree se interrumpió al detenerse GitRaptor; raptor undo vuelve al estado anterior` (`la restauración`, `el redo` y `la operación`).
  - **Ajuste del Arquitecto (redacción neutra)**: el primer cliente puede ser el propio `raptor undo`, así que el texto no le ordena ejecutarlo.
  - Los tests buscan `was interrupted` / `se interrumpió`, el id y `raptor undo`.
- Un test unitario en `notices.rs` con `has_key` para las ocho claves.

### D5 — Fallo detectado: aviso solo en la respuesta (Decisión nueva)

En el escenario 3, el solicitante recibe el aviso en la propia respuesta del error, como ya hace hoy (`undo.interrupted` y `restore.interrupted`). No se registra un aviso pendiente: así no lo ve dos veces. No hay cambio de código. La enmienda de ADR-TMC-003 aclara que § 6.5 cubre las interrupciones que encuentra la recuperación, no las que se informan en la propia respuesta.

### Validación de D2 a D5

Decisión del orquestador (2026-10-09), validada por Arquitecto (D2 a D5, aprobadas con los ajustes incorporados arriba) y PO (D4 y D5: cumplen el escenario 2 y la historia cierra como `implemented`; TUI y MCP quedan anotados como pendientes en el PR y en el backlog, no en la historia). Supuesto del PO: en M2a no se espera que un agente que solo consulta por MCP reciba el aviso; si Rene lo contradice, la historia pasa a `partially-implemented`.

## File & Project Topology

### Slice A — contrato del API

- `crates/api/src/methods/timemachine.rs`
- `crates/api/src/timemachine.rs`
- `crates/api/tests/legacy_protocols.rs` (añadir `"timemachine.notices"` a `AFTER_FREEZE_FULL` y US-TMC-019 a su comentario)

### Slice B — núcleo: caos y avisos

- `crates/core/src/timemachine/chaos.rs`
- `crates/core/src/timemachine/store/capture.rs`
- `crates/core/src/timemachine/mod.rs` (una línea)
- `crates/core/src/timemachine/notices.rs` (nuevo)
- `crates/core/src/timemachine/notices_tests.rs` (nuevo)
- `crates/core/src/channel/conn.rs`

### Slice C — CLI

- `apps/cli/src/undo/mod.rs`
- `apps/cli/src/undo/notices.rs` (nuevo)
- `apps/cli/src/commands/restore.rs`
- `apps/cli/src/commands/timeline.rs`
- `apps/cli/i18n/en/timemachine.txt`
- `apps/cli/i18n/es/timemachine.txt`

### Slice D — documentación (orquestador, tras validar D2 a D5)

- `docs/requirements/features/time-machine/user-stories/US-TMC-019-robustez-interrupcion.md` (status)
- `docs/requirements/features/time-machine/dev-specs/INF-TMC-001-arnes-caos-recuperable.md` (§ 6: la captura de observación y la restauración ya están cubiertas)
- `docs/architecture/decisions/ADR-TMC-003-oplog-diario-recuperacion.md` (enmienda «(2026-10-09, US-TMC-019)» con D2 a D5: método `timemachine.notices` solo en el canal completo y detrás de la puerta del `.git`, leído y marcado bajo el lock del oplog, entrega al menos una vez, el aviso de repo entero va al primer cliente, superficies CLI, TUI, MCP y stream diferidos, el fallo informado en la respuesta no genera aviso pendiente y los avisos de purga no salen por este método)
- `docs/requirements/backlog.md` y `docs/requirements/release-status.md` (este último regenerado)

Orden: B y C dependen de los tipos de A. A es pequeño: hazlo primero (o que lo haga el experto de B) y después corre B y C en paralelo. El archivo de tests de contrato (ver Tests) ya está escrito y no se toca.

## Tests

Archivo de contrato nuevo, ya escrito, que compila con la API de hoy: `apps/cli/tests/tm_interruption.rs` (macOS y Linux). Usa el `raptor` real, puntos de caos con nombre y `SIGKILL`, sin `sleep` fijos (solo sondeo con plazo) y la huella de INF-GRP-001 sobre los dos worktrees.

| Test | Escenario | Hoy |
|---|---|---|
| `s1_a_capture_cut_before_its_ref_is_never_a_point`, `s1_a_capture_cut_after_its_ref_is_never_a_point` | 1 | Rojo: la captura termina y el daemon no muere (no hay punto) |
| `s2_an_undo_cut_half_way_is_recoverable_and_noticed_once`, `s2_a_restore_cut_half_way_is_recoverable_and_noticed_once` | 2 y 5 | Rojo: `method not found` (-32601) |
| `s2_the_cli_shows_the_notice_once_in_english`, `..._in_spanish` | 2 (CLI, i18n) | Rojo: stderr vacío |
| `s2_a_folder_the_repo_does_not_register_gets_no_notice` | 2 (puerta del `.git`) | Rojo: -32601 en lugar de `SCOPE_REFUSED/not-observed` |
| `s2_another_repo_gets_no_notice` | 2 (solo el worktree que pasa `tm_scope_for`; condición del coordinador) | Rojo: -32601 |
| `s3_a_write_refused_half_way_interrupts_without_rollback` | 3 | Verde (regresión) |
| `s4_only_the_own_git_lock_is_released_at_start` | 4 | Verde (regresión) |
| `s6_a_clean_restart_gives_no_notice_and_a_complete_history` | 6 | Rojo: -32601. Todo lo anterior a esa llamada ya pasa |

Lo que no es del método de avisos (la recuperación de la restauración cortada, el undo de vuelta, el historial antes y después del reinicio limpio) se verificó en verde con una copia temporal ya borrada.

Además, sin ser criterios: los tests unitarios de `chaos.rs`, `notices_tests.rs` y `undo/notices.rs`. Ciclo de desarrollo: `cargo test -p gitraptor-cli --test tm_interruption`, `cargo test -p gitraptor-core --lib timemachine::`, `cargo test -p gitraptor-api --test legacy_protocols`, `cargo test -p gitraptor-cli --test tm_chaos`, `cargo test -p gitraptor-core --test tm_chaos_gate`, `cargo test -p gitraptor-core --test tm_untrusted_git_channel`. Una sola vez antes del PR: `cargo clippy --workspace --all-targets -- -D warnings`. Sin `cargo clean` y sin tocar `RUSTC_WRAPPER` ni `CARGO_TARGET_DIR`.

## Not Built (deferred)

- Aviso en la TUI: añadirlo cuando el Cockpit tenga una cola de avisos (patrón de US-GRP-020).
- Aviso por MCP: añadirlo cuando una historia de MCP lo exponga (con forma acotada, SEC-12).
- Entrega de avisos de purga: es de US-TMC-016, que la necesita para la gracia de 24 h.
- Publicar el aviso en el stream en vivo (Enmienda Cockpit): añadirlo cuando haya clientes conectados durante la recuperación. Hoy la recuperación ocurre antes de aceptar clientes.
- Puntos de caos en `finish_manual` (capturas manuales por MCP): añadirlos con un arnés de caos para MCP. La fila `pending` se recupera igual.
- Aviso de un lock propio conservado por identidad desconocida (riesgo residual de ADR-TMC-003): añadirlo con el TD correspondiente.
- `raptor status` como superficie del aviso: añadirlo si hace falta tras el dogfooding.
- Corregir la guía ADR-GRP-016 sobre `.since`: un PR de docs aparte.

## Riesgos

- **R1, carrera de entrega**: dos clientes a la vez. `take_interruptions` corre con el mutex del oplog tomado y lee y marca en la misma sección crítica.
- **R2, puerta del `.git`**: `tm_notices` tiene que pasar por `tm_scope_for`. Si abriera el oplog por `repo_id` saltaría #223 I-03. Lo fija `s2_a_folder_the_repo_does_not_register_gets_no_notice`. Para security-expert: entrada nueva desde la ruta del cliente.
- **R3, texto del repo en el terminal**: los ids salen del oplog. Se sanean con `sanitize` antes de imprimirlos.
- **R4, puntos de captura**: `capture:*` también afecta al ancla después de un undo (nivel observación). Ningún test de `tm_chaos` arma esos puntos, así que no se ve afectado.
- **R5, huella**: la sonda del aplicador cambia el mtime de la raíz. Los tests miden lo que cambia la recuperación, no el undo.

## Plataformas

| Objetivo | Producción | Tests de proceso |
|---|---|---|
| macOS | Soportado | `tm_interruption.rs` (verificado aquí) |
| Linux | Soportado | `tm_interruption.rs` en el CI de ubuntu (la liberación del lock necesita la fecha de creación, ADR-TMC-003 § 6.4) |
| Windows | Compila: los puntos usan `die()` → `abort` fuera de Unix y el método y la CLI no tienen `cfg` | Excluidos (sin `SIGKILL`, el CI no corre Git en Windows). Pendiente: etapa de validación multiplataforma. El fallo detectado en Windows ya lo cubre `tm_apply.rs::a_file_open_in_an_editor_*` |

`cargo clippy --target x86_64-pc-windows-msvc` no compila en macOS (`libsqlite3-sys` necesita MSVC). Solo `-p gitraptor-api` se puede comprobar en local; el resto lo valida el job de Windows del CI.

## Traspaso

`rust-expert`: implementa los slices A, B y C de este brief con el contrato `docs/dev-briefs/us-tmc-019-interruption-robustness.contract.json`. Las decisiones D2 a D5 son nuevas: el orquestador las valida antes de aprobar el plan. `security-expert`: revisa R2 y R3.
