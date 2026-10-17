---
schema: nassa.approval-request.v1
plan: "docs/dev-briefs/ux-timeline-status.md"
---

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: no
- Generada: `2026-10-17T16:37:38.682Z`
- Sha de aprobación: `d1a2d30839ebe8ec7b9dda1a439c0149fbe706d8a6ed4d1e4e09ab708d1bf7b1:dcd2b7d62f5764438560243bbfc5dbca8612ae5f455908fad2f76b33340e7405:8f0a25630d6ffb4eb9c496d8d4fcb8f98ce858829cc582ab195616410133a686`

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES 001929c83589>>>
```text
docs/dev-briefs/ux-timeline-status.md
```
<<<FIN-DATOS-NO-CONFIABLES 001929c83589>>>

planSha: `d1a2d30839ebe8ec7b9dda1a439c0149fbe706d8a6ed4d1e4e09ab708d1bf7b1` (base de la cadena de re-planes)

Contenido:

<<<DATOS-NO-CONFIABLES 293e1e725cdf>>>
````text
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
````
<<<FIN-DATOS-NO-CONFIABLES 293e1e725cdf>>>

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES b1eb3a1fd194>>>
```text
docs/dev-briefs/ux-timeline-status.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES b1eb3a1fd194>>>

contractSha: `dcd2b7d62f5764438560243bbfc5dbca8612ae5f455908fad2f76b33340e7405`

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 9d556fca7d0d>>>
```text
[1] id: TMC023-FORMAT-EN
statement: El timeline sin opciones, con la actividad de E1, escribe una línea por evento con columnas alineadas, agrupado por día de más antiguo a más reciente, con "restore <id corto>" en la misma línea, "you or other" para lo no atribuido y la leyenda una vez; coincide con E1 en inglés
verify: cargo test -p gitraptor-cli --bin raptor commands::timeline::compact_tests::the_timeline_matches_e1_in_english -- --exact
tests:
  - apps/cli/src/commands/timeline_compact_tests.rs

[2] id: TMC023-FORMAT-ES
statement: La misma salida coincide con E2 en español ("tú u otro", niveles previo/observado/ninguno, leyenda y pista de restore)
verify: cargo test -p gitraptor-cli --bin raptor commands::timeline::compact_tests::the_timeline_matches_e2_in_spanish -- --exact
tests:
  - apps/cli/src/commands/timeline_compact_tests.rs

[3] id: TMC023-SUMMARY
statement: Sin --verbose, dos ramas creadas y borradas y tres resets sin mover la rama se resumen en una línea cada uno ("(x3)"), un reset con punto de restauración conserva su línea con id, y la cabecera cuenta entradas y resumidas
verify: cargo test -p gitraptor-cli --bin raptor commands::timeline::compact_tests::low_value_entries_are_summarized_but_recoverable_ones_never -- --exact
tests:
  - apps/cli/src/commands/timeline_compact_tests.rs

[4] id: TMC023-EMPTY-WINDOW
statement: Con actividad de hace tres días y sin --since, la ventana por defecto vacía imprime el aviso E4 (en/es) y no "no activity yet"; un repo sin ninguna actividad sigue diciendo "no activity yet"
verify: cargo test -p gitraptor-cli --bin raptor commands::timeline::compact_tests::an_empty_default_window_says_how_to_widen_it -- --exact
tests:
  - apps/cli/src/commands/timeline_compact_tests.rs

[5] id: TMC023-VERBOSE
statement: Con --verbose se ven todas las entradas sin resumir, con actor y origen, nivel completo, id completo, lo que deshizo cada undo y la lista de archivos solo si hay archivos
verify: cargo test -p gitraptor-cli --bin raptor commands::timeline::compact_tests::verbose_shows_every_entry_with_full_detail -- --exact
tests:
  - apps/cli/src/commands/timeline_compact_tests.rs

[6] id: TMC023-JSON-UNCHANGED
statement: Con --json la petición no lleva ventana por defecto y la salida no se resume: default_since devuelve None con json y Some("24h") solo en texto sin --since
verify: cargo test -p gitraptor-cli --bin raptor commands::timeline::compact_tests::json_keeps_no_default_window -- --exact
tests:
  - apps/cli/src/commands/timeline_compact_tests.rs

[7] id: TMC023-RESTORE-PREFIX
statement: Restaurar con un prefijo de 6 o más caracteres único restaura igual que con el id completo (misma confirmación y mismo estado previo guardado)
verify: cargo test -p gitraptor-core --test us_tmc_023_restore_prefix a_unique_prefix_restores_like_the_full_id -- --exact
tests:
  - crates/core/tests/us_tmc_023_restore_prefix.rs

[8] id: TMC023-RESTORE-AMBIGUOUS
statement: Un prefijo de 6 o más caracteres que coincide con dos puntos se rechaza como uso incorrecto, sin cambiar nada ni registrar operación, y devuelve los candidatos con id, fecha, worktree y qué pasó; un prefijo de menos de 6 caracteres también se rechaza
verify: cargo test -p gitraptor-core --test us_tmc_023_restore_prefix an_ambiguous_prefix_is_refused_with_its_candidates -- --exact
tests:
  - crates/core/tests/us_tmc_023_restore_prefix.rs

[9] id: TMC023-SCOPE
statement: Desde el repo A, el prefijo (de 6 caracteres o más) de un punto del repo B da NOT_FOUND, no restaura ni registra nada y no filtra el id de B; el prefijo de un punto propio de A sí restaura (la búsqueda pasa por tm_scope_for y solo ve los puntos del repo del solicitante, todos sus worktrees)
verify: cargo test -p gitraptor-core --test us_tmc_023_restore_prefix a_prefix_never_resolves_or_lists_ids_of_another_repo -- --exact
tests:
  - crates/core/tests/us_tmc_023_restore_prefix.rs

[10] id: TMC023-SHORT-ID
statement: El motor entrega, solo si la petición lo pide, la longitud única del id corto de cada punto frente a todos los puntos del repo (mínimo 8, sin terminar en guion); sin la petición el JSON del timeline no cambia
verify: cargo test -p gitraptor-core --test us_tmc_023_restore_prefix the_engine_returns_the_unique_short_id_length_only_when_asked -- --exact
tests:
  - crates/core/tests/us_tmc_023_restore_prefix.rs

[11] id: TMC023-AMBIGUOUS-TEXT
statement: El mensaje de prefijo ambiguo de raptor restore coincide con E3 (prefijo 47bfc8) en inglés y español, lista como mucho 5 candidatos (con "+K" si hay más) y el prefijo de menos de 6 caracteres dice restore.too-short ("at least 6 characters" / "al menos 6 caracteres") en ambos idiomas
verify: cargo test -p gitraptor-cli --bin raptor commands::restore::prefix_tests::the_ambiguous_prefix_message_matches_e3_in_en_and_es -- --exact
tests:
  - apps/cli/src/commands/restore_prefix_tests.rs

[12] id: GRP023-SUMMARY
statement: Con 223 cambios (1 preparado, 2 sin preparar, 220 sin seguimiento: 214 bitacora/, 4 docs/, 2 raíz) status lista preparado y luego sin preparar, resume los sin seguimiento por carpeta de mayor a menor y añade el pie una vez; coincide con E1 en inglés
verify: cargo test -p gitraptor-cli --bin raptor status::summary_tests::many_untracked_files_are_summarized_by_folder_in_english -- --exact
tests:
  - apps/cli/src/status_summary_tests.rs

[13] id: GRP023-SUMMARY-ES
statement: La misma salida coincide con E2 en español ("sin rastrear", "(raíz)")
verify: cargo test -p gitraptor-cli --bin raptor status::summary_tests::many_untracked_files_are_summarized_by_folder_in_spanish -- --exact
tests:
  - apps/cli/src/status_summary_tests.rs

[14] id: GRP023-FEW
statement: Con 3 archivos sin seguimiento se listan uno por uno, sin línea de resumen ni pista de --verbose
verify: cargo test -p gitraptor-cli --bin raptor status::summary_tests::few_untracked_files_are_still_listed -- --exact
tests:
  - apps/cli/src/status_summary_tests.rs

[15] id: GRP023-VERBOSE
statement: Con --verbose cada archivo sin seguimiento que el motor envió sale en su línea y no hay pista
verify: cargo test -p gitraptor-cli --bin raptor status::summary_tests::verbose_lists_every_untracked_file_sent -- --exact
tests:
  - apps/cli/src/status_summary_tests.rs

[16] id: GRP023-PARTIAL
statement: Con 5000 archivos sin seguimiento de los que el motor envió una parte, el resumen cuenta lo enviado por carpeta y añade cuántos quedaron sin listar, de modo que la suma da el total real
verify: cargo test -p gitraptor-cli --bin raptor status::summary_tests::a_partial_list_never_looks_like_the_total -- --exact
tests:
  - apps/cli/src/status_summary_tests.rs

[17] id: GRP023-JSON-UNCHANGED
statement: La salida --json de status es idéntica a la de antes de la historia
verify: cargo test -p gitraptor-cli --bin raptor status::summary_tests::json_output_is_unchanged_by_verbose_and_summary -- --exact
tests:
  - apps/cli/src/status_summary_tests.rs

[18] id: GRD020-STATE-CONFIRMED
statement: Con hooks activos y rama base main confirmada, la primera línea de guard status es "state: protected, base branch main" / "estado: protegido, rama base main", sin "hooks only" y sin la línea aparte de la rama base
verify: cargo test -p gitraptor-cli --bin raptor guard::status_text_tests::a_confirmed_base_reads_protected_with_the_base -- --exact
tests:
  - apps/cli/src/guard/status_text_tests.rs

[19] id: GRD020-STATE-UNCONFIRMED
statement: Con la rama base sin confirmar la primera línea dice protegido, rama base sin confirmar y las ramas que se protegen mientras tanto (en/es)
verify: cargo test -p gitraptor-cli --bin raptor guard::status_text_tests::an_unconfirmed_base_names_the_branches_protected_meanwhile -- --exact
tests:
  - apps/cli/src/guard/status_text_tests.rs

[20] id: GRD020-STATE-PARTIAL
statement: Con hooks instalados pero no activos la primera línea dice protección incompleta (en/es), nunca protegido, y las líneas siguientes siguen explicando la causa
verify: cargo test -p gitraptor-cli --bin raptor guard::status_text_tests::inactive_hooks_read_partly_protected_never_protected -- --exact
tests:
  - apps/cli/src/guard/status_text_tests.rs

[21] id: GRD020-DENY-DELETE
statement: Un borrado de rama denegado sin el servicio coincide con E2 en inglés y español: nombra el borrado de feat-x, pide volver a borrar la rama y no menciona push, remoto ni conjunto mínimo; el stderr completo del hook es solo ese mensaje (sin la pista del push ni el aviso del conjunto mínimo)
verify: cargo test -p gitraptor-cli --bin raptor guard::deny_text_tests::a_branch_delete_denied_without_the_service_matches_e2 -- --exact && cargo test -p gitraptor-cli --test us_grd_020_hook_stderr delete_denied_without_the_service_prints_only_e2 -- --exact
tests:
  - apps/cli/src/guard/deny_text_tests.rs
  - apps/cli/tests/us_grd_020_hook_stderr.rs
  - apps/cli/tests/us_grd_017_hook.rs

[22] id: GRD020-DENY-REBASE
statement: Un rebase denegado sin el servicio coincide con E3 en inglés y español, y sin rama conocida dice "the rebase" / "el rebase"; el stderr completo del hook es solo ese mensaje
verify: cargo test -p gitraptor-cli --bin raptor guard::deny_text_tests::a_rebase_denied_without_the_service_matches_e3 -- --exact && cargo test -p gitraptor-cli --test us_grd_020_hook_stderr rebase_denied_without_the_service_prints_only_e3 -- --exact
tests:
  - apps/cli/src/guard/deny_text_tests.rs
  - apps/cli/tests/us_grd_020_hook_stderr.rs

[23] id: GRD020-DENY-PUSH
statement: Un push denegado sin el servicio sigue pidiendo volver a hacer el push; la pista del stderr es guardstate.retry.push
verify: cargo test -p gitraptor-cli --bin raptor guard::deny_text_tests::a_push_denied_without_the_service_still_asks_to_push_again -- --exact && cargo test -p gitraptor-cli --test us_grd_020_hook_stderr push_denied_without_the_service_asks_to_push_again -- --exact
tests:
  - apps/cli/src/guard/deny_text_tests.rs
  - apps/cli/tests/us_grd_020_hook_stderr.rs
  - apps/cli/src/guard/snapshot_required_tests.rs
```
<<<FIN-DATOS-NO-CONFIABLES 9d556fca7d0d>>>

## Tests de criterio

testsSha: `8f0a25630d6ffb4eb9c496d8d4fcb8f98ce858829cc582ab195616410133a686`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES 6ca8c60c3ad9>>>
```text
4bacedcd5e30a75392847517212aa5bba442f1cb68a838c0614928a9e3f43c99  apps/cli/src/commands/restore_prefix_tests.rs
6caa9fddd174b726886a738fd3b20bbd5f53e2f89289ba144b779207b29cfe11  apps/cli/src/commands/timeline_compact_tests.rs
d55e6f36c3913d7a0593542972a28f68b8319ef708b432f4349db773de4206a6  apps/cli/src/guard/deny_text_tests.rs
12ec77f6af1b22e0b9df18955db2a45606a4ec7b4f32532506092f9854466237  apps/cli/src/guard/snapshot_required_tests.rs
f3a87841d1047878a333a03b2554c4b99729dea767cf8d8b72e464e9489ae497  apps/cli/src/guard/status_text_tests.rs
e4e7bec39c60badd09ea5154c4481c0b770c98e68ee14977d35fa060b3b9441c  apps/cli/src/status_summary_tests.rs
bc7d476d5e290ea9d3686ed2f4320a570d2d7c0352e4bdc097b6224e4ce13555  apps/cli/tests/us_grd_017_hook.rs
4bf81edfe37f377e7320dae9e8e69793c809251c148cf839eeed5b65be93bb3e  apps/cli/tests/us_grd_020_hook_stderr.rs
7e70e62e314fae3287ce2a5b364f6067b9df5af6fde0edb606d0557a6eeb839b  crates/core/tests/us_tmc_023_restore_prefix.rs
```
<<<FIN-DATOS-NO-CONFIABLES 6ca8c60c3ad9>>>

## Crítico

- Rondas (plan): 1
  - ronda 1: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
- Último veredicto: APROBABLE
- Excepción: no

## Diferidos

Ninguno: el crítico dio el plan por aprobable.

## Firmas

- planSha (contexto, no firmado): `d1a2d30839ebe8ec7b9dda1a439c0149fbe706d8a6ed4d1e4e09ab708d1bf7b1`
- contractSha: `dcd2b7d62f5764438560243bbfc5dbca8612ae5f455908fad2f76b33340e7405`
- testsSha: `8f0a25630d6ffb4eb9c496d8d4fcb8f98ce858829cc582ab195616410133a686`
- approvalSha: `d1a2d30839ebe8ec7b9dda1a439c0149fbe706d8a6ed4d1e4e09ab708d1bf7b1:dcd2b7d62f5764438560243bbfc5dbca8612ae5f455908fad2f76b33340e7405:8f0a25630d6ffb4eb9c496d8d4fcb8f98ce858829cc582ab195616410133a686`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
