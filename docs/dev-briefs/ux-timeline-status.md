# Brief: timeline legible, status resumido y mensajes de guard (US-TMC-023, US-GRP-023, US-GRD-020)

Tres historias de presentación, sin dependencias nuevas. Las historias son la fuente de verdad (formatos E1..E4 y Gherkin); este brief fija dónde se implementa y en qué orden. `--json` queda idéntico byte a byte, el Cockpit no cambia y los tests usan repos o directorios temporales (NFR-01). Revisión 2 (crítico, ronda 1): PC-1..PC-11 resueltos.

## Decisiones de arquitectura

- **Renderizado puro y probable en ambos idiomas.** El catálogo (`apps/cli/src/i18n.rs`) fija un idioma por proceso (`OnceLock`). Se añade `i18n::Tr { spanish: bool }` con `Tr::current()` y `Tr::t(key, args)`; los renderizadores nuevos reciben `Tr`, los comandos pasan `Tr::current()`.
- **Tests de formato unitarios del binario** (`--bin raptor`, precedente `commands::timeline::inferred_tests`) contra los literales E1/E2/E3 de las historias; sin `insta`.
- **Ventana de 24 h y resúmenes en el CLI**: el CLI manda `since = "24h"` solo si no hay `--since` y no es `--json`.
- **E4 frente a "sin actividad alguna" (PC-6)**: si la ventana por defecto vuelve vacía y sin fuentes caídas, el CLI repite la petición una vez con `since = None, limit = 1`; si esa trae algo, imprime E4; si no, `timeline.empty`. Con `--since` explícito no hay segunda petición.
- **Ids (PC-3, PC-5).** `snapshot_id` es un UUID de 36 caracteres con guiones en las posiciones 8, 13, 18 y 23. El motor calcula la longitud única por punto frente a **todos** los puntos del repo y la devuelve en `TimelineEntry.short_id_len: Option<u8>`, solo si la petición lleva `short_ids: true` (ambos campos con `skip_serializing_if`, así que `--json` no cambia: el CLI no los manda en JSON). `short_id(id, len)` corta `id[..len]` con `len >= 8`; si el último carácter cortado es `-`, toma uno más (nunca termina en guion). El prefijo que acepta `restore` es un prefijo de UUID: 6..=36 caracteres de `[0-9a-f-]` con guiones solo en esas posiciones.
- **Prefijo ambiguo (PC-4)**: el motor devuelve `INVALID_PARAMS` con `data` = `AmbiguousData`, definido en `gitraptor-api`; el CLI pinta E3 en cada idioma con los datos, reutilizando el texto de "qué pasó" del timeline. Un prefijo de menos de 6 caracteres (mínimo contra la enumeración) se rechaza en `RestoreParams::validate` y el CLI lo traduce a `restore.too-short` ("type at least 6 characters" / "escribe al menos 6 caracteres"). La búsqueda del prefijo y `data.candidates` solo recorren los puntos del repo del solicitante (todos sus worktrees), según `tm_scope_for`, tras la puerta `tm_scope_for` (mismo alcance que timeline y restore); nunca ids de otro repo.
- **Mensajes en archivos propios (PC-1)** (ADR-GRP-016). **No se edita `guard.txt`** (W77 lo posee): `guard.state.hooks-only`, `guard.status.base-confirmed`, `guard.status.base-unconfirmed` y `guard.degraded.retry` quedan sin usar al quitar sus puntos de llamada. Las claves nuevas: `guardstate.{protected,protected-unconfirmed,partial}` y `guardstate.retry.{push,branch-delete,rebase,other}` en `guardstate.txt` (nuevo, ambos idiomas); `snapshotreq.deny.{branch-delete,rebase,rebase-unnamed}` y `snapshotreq.hint.start.{push,branch-delete,rebase,other}` en `snapshotreq.txt`. La frase "a person's operation is told apart from an agent's" / "la operación de una persona se distingue de la de un agente" va solo en `guardstate.retry.*`.
- **Dónde se imprime la pista (PC-1, PC-2)**: `apps/cli/src/guard.rs` (bloque `if outcome.degraded.is_some()` tras `outcome.allowed()`) deja de escribir `guard.degraded.retry` cuando la denegación incluye una razón `Rule::SnapshotFailed` (esa razón ya lleva su pista por operación, sin párrafo del conjunto mínimo ni del push); en cualquier otra denegación degradada escribe `guardstate.retry.<op>`.
- **Fail closed en status**: el resumen nunca presenta una lista parcial como total.

## Contratos

```rust
// apps/cli/src/i18n.rs (additive, slice 1)
#[derive(Clone, Copy)] pub struct Tr { pub spanish: bool }
impl Tr { pub fn current() -> Self; pub fn t(self, key: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String; }

// apps/cli/src/commands/timeline_compact.rs (new, slice 1; mod `compact`)
pub(super) struct CompactOpts { pub verbose: bool, pub today: LocalDay, pub window_applied: bool }
pub(super) fn render_compact(result: &TimelineResult, opts: &CompactOpts, tr: Tr) -> String;
pub(super) fn short_id(id: &str, len: u8) -> String;
pub(super) fn default_since(json: bool, since: Option<String>) -> Option<String>; // Some("24h") only if !json && since.is_none()
pub(super) fn empty_text(window_applied: bool, activity_outside_window: bool, tr: Tr) -> String; // E4 or timeline.empty

// crates/api/src/timemachine.rs (slice 1; additive)
TimelineParams.short_ids: bool;  TimelineEntry.short_id_len: Option<u8>;
pub struct AmbiguousCandidate { pub snapshot_id: String, pub origin: EntryOrigin, pub worktrees: Vec<Untrusted>, pub occurred_utc_ms: i64, pub utc_offset_s: i32 }
pub struct AmbiguousData { pub candidates: Vec<AmbiguousCandidate> /* <= 5 */, pub total: u32 }

// crates/core/src/timemachine/restore.rs (slice 1)
pub enum RestoreError { /* existing */ Ambiguous(AmbiguousData) }
pub fn resolve_point_prefix(points: &[SnapshotView], text: &str) -> Result<String, RestoreError>;

// apps/cli/src/status.rs (slice 2)
pub fn text(snapshot: &Snapshot, sessions: &SessionsInfo, verbose: bool, tr: Tr) -> String;
pub(crate) const UNTRACKED_LIST_LIMIT: usize = 10;

// apps/cli/src/guard.rs and guard/snapshot_required.rs (slice 3; PC-10)
fn status_lines(status: &GuardStatus, tr: Tr) -> Vec<String>;                       // guard.rs
pub(super) enum DeniedOp { BranchDelete { branch: Option<String> }, Rebase { branch: Option<String> }, Push, Other }  // snapshot_required.rs
pub(super) fn reason_text(reason: &Reason, denied: DeniedOp, tr: Tr) -> String;     // snapshot_required.rs
```

Códigos estables: sin código nuevo; el ambiguo reutiliza `INVALID_PARAMS` con `data` tipado, el CLI no parsea `Display`.

## File & Project Topology (rebanadas disjuntas)

**Slice 1 — US-TMC-023 (tracer: formato compacto).** Orden: (a) `Tr` en `i18n.rs`, commit aparte; (b) formato compacto; (c) ventana, `--verbose`, E4; (d) id corto y prefijo.
- `apps/cli/src/i18n.rs`, `apps/cli/src/commands/timeline.rs`, `apps/cli/src/commands/timeline_compact.rs` (nuevo), `apps/cli/src/commands/restore.rs`, `apps/cli/i18n/{en,es}/timemachine.txt` (claves `timeline.*`, `restore.*`, incl. `restore.too-short`).
- `crates/api/src/timemachine.rs` (`validate` del restore acepta prefijo; campos y DTO nuevos), `crates/core/src/timemachine/timeline.rs` (`short_id_len`), `crates/core/src/timemachine/restore.rs`, y solo el mapeo de `Ambiguous` en `crates/core/src/channel/conn.rs`.
- Tests: `apps/cli/src/commands/timeline_compact_tests.rs`, `apps/cli/src/commands/restore_prefix_tests.rs`, `crates/core/tests/us_tmc_023_restore_prefix.rs` (prefijo, ambiguo y `short_id_len`).

**Slice 2 — US-GRP-023.** `apps/cli/src/status.rs`, `apps/cli/src/commands/status.rs` (`--verbose`), `apps/cli/i18n/{en,es}/status.txt` (`status.untracked-*`, `status.hint.verbose`). Test: `apps/cli/src/status_summary_tests.rs`.

**Slice 3 — US-GRD-020.** `apps/cli/src/guard.rs`, `apps/cli/src/guard/snapshot_required.rs`, `apps/cli/i18n/{en,es}/snapshotreq.txt`, `apps/cli/i18n/{en,es}/guardstate.txt` (nuevo). Tests: `apps/cli/src/guard/status_text_tests.rs`, `apps/cli/src/guard/deny_text_tests.rs`, `apps/cli/tests/us_grd_020_hook_stderr.rs` (stderr completo del hook sin servicio, repo temporal). Los tests que fijan el texto viejo (`guard/snapshot_required_tests.rs`, `apps/cli/tests/us_grd_017_hook.rs`) los actualiza el tester.

**Módulos de test y stubs (PC-7)** — paso 0 de cada rebanada, antes de que corra ningún `verify` (con `-- --exact`, un test no declarado sale 0 sin ejecutar nada). Las líneas `mod` son lo único que se añade a esos archivos fuera de su lógica:
- `commands/timeline.rs`: `#[path = "timeline_compact.rs"] mod compact;` y `#[cfg(test)] #[path = "timeline_compact_tests.rs"] mod compact_tests;`
- `commands/restore.rs`: `#[cfg(test)] #[path = "restore_prefix_tests.rs"] mod prefix_tests;`
- `status.rs`: `#[cfg(test)] #[path = "status_summary_tests.rs"] mod summary_tests;`
- `guard.rs`: `#[cfg(test)] mod status_text_tests;` y `#[cfg(test)] mod deny_text_tests;`
- Stubs que compilan y fallan por la razón correcta (cuerpo con el comportamiento actual o `unimplemented!()`): `Tr`, `render_compact`, `short_id`, `default_since`, `empty_text`, `resolve_point_prefix`, los campos y DTO de la API (`Default`/`None`), `status::text` con `verbose`, `status_lines(.., tr)`, `DeniedOp`, `reason_text(.., denied, tr)`.

`i18n.rs` solo lo toca la slice 1; ningún otro archivo está en dos rebanadas.

## Convenciones observadas

- Catálogo por feature, una clave en un solo archivo: `apps/cli/build.rs` (registra `i18n/<lang>/<feature>.txt`) y `apps/cli/src/i18n.rs:each_key_group_belongs_to_one_file`.
- Sin concatenar: `commands/timeline.rs:entry_text` y `status.rs:counts_text` pasan `&[("name", &valor)]` a `t`.
- Texto del repo saneado: `gitraptor_api::untrusted::sanitize` en `commands/timeline.rs:entry_text`; `WorktreeView.path.sanitized()` en `status.rs:worktree_text`.
- Hora local: `apps/cli/src/events.rs:local_time`.
- Tests del binario en `*_tests.rs` con `#[cfg(test)] mod`: `guard.rs:35 snapshot_required_tests`, `commands/timeline.rs:351 inferred_tests`.
- Plantilla de denegación sin cómo saltarse la protección: `guard/snapshot_required.rs:reason_text` (ADR-GRD-003 § 3).
- Códigos de error estables: `crates/api/src/rpc.rs`; el CLI los traduce en `commands/restore.rs:failure_text`.
- Campos opcionales nuevos de la API con `skip_serializing_if` (un daemon viejo rechaza campos desconocidos): `TimelineParams.only_worktree` en `crates/api/src/timemachine.rs`.

## Mejoras detectadas

- `RestoreParams::validate` usa `check_oplog_id` (UUID completo): se relaja solo para `restore`, no para `undo`/`redo`.
- `guard.txt` queda con claves sin usar (`guard.state.hooks-only`, `guard.status.base-*`, `guard.degraded.retry`); limpiarlas cuando W77 mergee.
- `ProtectionState` no tiene "parcial": se deriva en el CLI de los datos que `protection_lines` ya usa.

## Plataformas y NFRs

macOS validado; Linux y Windows pendientes de la etapa de validación cruzada. Sin `cfg` nuevos, sin `unsafe`. Render O(n) sobre ≤200 entradas. Para `security-expert`: `data.candidates` lleva texto del repo y se sanea al imprimirlo. Aceptación: `node tools/test/nextest-junit.mjs`; calidad: `cargo clippy --workspace --all-targets -- -D warnings`. La certificación (TTY, 80 columnas, color, repo real en lectura) la hace un humano.

## Supuestos

- Los tests de formato son unitarios del binario con literales E1/E2, no `apps/cli/tests/*.rs` ni `insta`: el binario no expone módulos a la integración. La excepción es el stderr del hook (slice 3), que es de proceso.
- El prefijo lo resuelve el motor, no el CLI (el timeline está limitado a 200 entradas y a la ventana).
- Los tests en/es usan `Tr` explícito, no un binario por idioma.
- "Protección incompleta" se deriva en el CLI, sin añadir `Partial` a la API (cambiaría el JSON).
- El hook conoce la operación denegada (`Operation::Rebase{branch}`, borrado de rama en `RefTransaction`); sin rama se usa "the rebase" / "el rebase". El experto localiza dónde se construye la razón con el daemon parado antes de tocar nada.
- Resumir agrupa por día y worktree y solo entradas sin punto de restauración (decisión 6 de US-TMC-023).
- Umbral de status 10 por worktree; la carpeta es el primer componente de la ruta.
- Convivencia con `status.truncated` y `status.lists-dropped` (PC-11): sin `--verbose` y con resumen, "N sin listar" sustituye a `status.truncated` en las líneas sin seguimiento; con `--verbose` o con ≤10 archivos, `status.truncated` queda como hoy; `status.lists-dropped` (el motor no envió ninguna lista) queda igual y no se resume.
- `--since 7d` ya lo filtra el motor sin cambios; no se añade test de daemon para ello, y se verifica a mano en la certificación.

## Not Built (deferred)

- Colores y recorte a 80 columnas en TTY — se añade cuando se certifique en TTY.
- Alinear el "Sin agente" del Cockpit con "tú u otro" — se añade cuando el Cockpit lo pida.
- Resumen de preparados y modificados en status — se añade cuando un worktree real los desborde.
- `insta` para estos formatos — se añade si los literales se vuelven inmanejables.
- Enmienda de `docs/design-system/README.md` § 2.2 (símbolo `↺`) — la hace quien mantenga el design system.
- Mensajes de `guard install`/`uninstall` y cualquier cambio en `guard.txt` — los edita W77.
