# Preparación de la corrida · US-GRD-006 PR-A (2026-10-27)

Sin diseño: [DS-US-GRD-006](../requirements/features/guardrails/dev-specs/US-GRD-006-excepcion-consciente.md) es el plan. Alcance: **solo PR-A** (D22) = T001–T004: contrato en `crates/api`, normalización pura en `crates/policy/src/guard/exception.rs`, almacén de tokens puro en `crates/core/src/guardrails/exception.rs` + `pending::Entry.exception` + campo en `registry.rs`, y sus pruebas. Nada de daemon, canal, hook, CLI, plantilla 4 ni e2e. `guardrails::exception::{apply, facts}` son de PR-B.

## Deriva de citas

| Cita de la spec | Hoy | Efecto |
|---|---|---|
| `crates/api/src/guard.rs:217-224` `ExceptionState` | 217 es el `///`, el enum en 218-224 con `None, Applied, Rejected` | Ninguno. Se corrige el comentario «Always `none` until US-GRD-006» |
| `PendingKind` (`guard.rs:583`) | `Uninstall, ConfirmTeam`; `match` exhaustivo en `crates/core/src/daemon/guard.rs:549-552` (`audit_pending`) | Añadir `Exception` rompe un archivo de daemon (fuera de PR-A). Ver S1 |
| `LogKind` (`guard.rs:1095`) | `Denial, Notice, ProtectionState`; `match` exhaustivos en `apps/cli/src/guard.rs:1227-1231` y `:1297-1300` | Añadir las tres variantes rompe la CLI (fuera de PR-A). Ver S1 |
| `PendingAction` (`guard.rs:570`) | Sin `rename_all` (cable en snake_case: `action_id`, `applies_at_ms`); único literal en `pending.rs:74` | Campo `exception` sin problema |
| `EvaluateParams` (`guard.rs:426`) | Sin `rename_all`: el cable es snake_case (`common_dir`, `crates/core/src/guardrails/hook.rs:318`). La spec escribe `exceptionToken` | Ver S2 y la lista de literales (PC-1) |
| `GuardLogSummary` (`guard.rs:1253`) | camelCase, `Default`; literales en `crates/core/src/guardrails/config_guard.rs:221` y `crates/core/src/profile/guard_log.rs:177` | Campos nuevos con `0` en esos dos literales (core, permitido) |
| `methods/guard.rs:42` `BLOCK` y `GROUP` | `BLOCK` en :46; usados `BLOCK` y `BLOCK - 1`; `BLOCK - 2` libre | Ver S3: registrar el error en `GROUP.errors` rompe `apps/cli/src/codes.rs::every_contract_code_has_both_messages` |
| `git_command.rs` tabla de opciones globales | `WITH_VALUE`, `FLAGS`, `WITH_EQUALS` (privadas, :10-49) | Pasan a `pub(super)` para `exception.rs`; no se duplican |
| `pending.rs` `Requester`/`Entry`/`WINDOW`/`GRACE`/`new_action_id` | Existen (:48, :54, :12, :14, :100); `Entry` solo se construye en `announce` (:139) | Ninguno |
| `registry.rs` | `GuardRegistry` `#[derive(Debug, Default)]` + `for_profile` con literal de campos | `exceptions: Tokens` necesita `Debug + Default` |
| `evaluate.rs:171` `getrandom` | `getrandom::fill` en :171 (`decision_id`) | Ninguno |
| `crates/core/Cargo.toml` `sha2`, `getrandom` | `getrandom = "0.4.3"` (:13), `sha2 = "0.11.0"` (:29) | Sin deps nuevas |
| `schema.rs:236-237` kinds `exception*` | Ahora en **:276-277** | Solo cambia la línea |
| D5 `policy.diff-size`, `policy.commit-format` | **No existen** en `Rule` (DS-US-GRD-009 sin merge) | `exceptable` las clasifica cuando 009 las añada: su `match` sin comodín obliga |

Ninguna deriva cambia lo que se construye de forma irreversible: **sin BLOQUEO**. S1–S3 ajustan el corte A/B para respetar `deny_paths`.

### Esqueleto

Lo escribe el criterion-tester junto a las pruebas; compila con cuerpos `todo!()` y **no toca tipos existentes** (ni variantes, ni campos, ni `GROUP`). Rojo por la razón correcta: tipos nuevos de `crates/api` **sin ningún `#[serde(...)]`** (sin `rename_all`, `deny_unknown_fields` ni `skip_serializing_if`); `ExceptionToken` con `#[derive(Debug)]`; métodos y capacidad declarados como `const` pero fuera de `GROUP`; las pruebas de forma del cable sobre tipos existentes usan JSON en cadenas (`deny_unknown_fields` las rechaza hasta que existan los campos).

**`crates/api/src/methods/guard.rs`** (el build los mete en `GROUP` sin `.since` y declarados por adelantado con `implemented_by: Some("US-GRD-006")`, ver S4; el error, S3)
- `pub const GUARD_EXCEPTION: &str = "guard.exception";` → `method(GUARD_EXCEPTION, true, false).ahead("US-GRD-006")`
- `pub const GUARD_EXCEPTION_CHILD: &str = "guard.exception-child";` → `method(…, true, false).ahead("US-GRD-006")`
- `pub const GUARD_EXCEPTION_END: &str = "guard.exception-end";` → `method(…, false, false).ahead("US-GRD-006")`
- `pub const CAP_GUARD_EXCEPTION: Capability = Capability::new("guard.exception");`
- `pub const GUARD_EXCEPTION_REFUSED: ErrorSpec = ErrorSpec::new(BLOCK - 2, "guard-exception-refused");`

**`crates/api/src/guard.rs`** (nuevos; cable camelCase y kebab-case como dice § 5)
- `pub enum ExceptionRefusal { NotInstalled, OutdatedInstall, AlreadyPending, WindowOpen, NoPending, NotTheRequester, UnsupportedForm, UnsupportedOperation, NotDenied, NotExceptable, ChildNotOurs }` — `Copy, Eq, Hash`, `rename_all = "kebab-case"`.
- `pub struct GuardExceptionRefusedData { pub reason: ExceptionRefusal, pub remaining_ms: Option<u64> }`
- `pub struct GuardExceptionParams { pub path: String, pub argv: Vec<String>, pub confirm: Option<String> }`
- `pub struct ExceptionPlanView { pub operation: LoggedOperation, pub rules: Vec<Rule> }`
- `pub struct IssuedException { pub token: ExceptionToken, pub git: Untrusted, pub config: Vec<String>, pub ttl_ms: u64 }` — `token` con `serialize_with` a una `fn` privada del módulo.
- `pub struct GuardExceptionResult { pub pending: Option<PendingAction>, pub plan: ExceptionPlanView, pub issued: Option<IssuedException> }`
- `pub struct GuardExceptionChildParams { pub path: String, pub action_id: String, pub pid: u32 }`
- `pub struct GuardExceptionEndParams { pub path: String, pub action_id: String }` / `pub struct GuardExceptionEndResult { pub consumed: bool }`
- `#[derive(Clone, PartialEq, Eq)] pub struct ExceptionToken(String)`: `pub fn from_secret(hex: String) -> Option<Self>` (exactamente 32 hex en minúscula), `pub fn expose(&self) -> &str`; `Debug` manual → `ExceptionToken(<redacted>)`; `Deserialize` manual que valida con `from_secret`; `JsonSchema` como `String`; **sin `Serialize`**. Dos `fn` privadas del módulo para `serialize_with`: `expose_token` (en `IssuedException.token`) y `expose_token_opt` (en `EvaluateParams.exception_token`).
- `pub const MAX_EXCEPTION_ARGV: usize = 64;` `pub const MAX_EXCEPTION_ARG_BYTES: usize = 4096;`
- En tipos existentes (solo el build): `EvaluateParams.exception_token: Option<ExceptionToken>` (cable `exception_token`; `default`, `skip_serializing_if = "Option::is_none"`, `serialize_with = "expose_token_opt"`; el `Deserialize` de `ExceptionToken` valida); `PendingAction.exception: Option<ExceptionPlanView>` (ídem); `GuardLogSummary.{exceptions, exceptions_rejected}: u64` (`default`, omitidos en cero).

**`crates/policy/src/guard/exception.rs`** (puro: sin E/S, reloj ni azar)
- `pub enum Unsupported { Form, Operation }` + `pub fn refusal(self) -> ExceptionRefusal`
- `pub struct Request { pub dirs: Vec<String>, pub form: Form }` (`dirs`: los `-C` en orden)
- `pub enum Form { Push { remote: Option<String>, refspecs: Vec<Refspec>, force: bool }, BranchDelete { names: Vec<String>, force: bool }, Rebase { onto: Option<String>, upstream: Option<String>, branch: Option<String> } }`
- `pub struct Refspec { pub src: Option<String>, pub dst: Option<String>, pub force: bool }` (`src: None` = borrado; `dst: None` = mismo nombre que `src`; `--delete x` → `{src: None, dst: Some(x)}`)
- `pub struct RepoFacts { pub head: Option<String>, pub push_default: Option<String>, pub remotes: Vec<String>, pub push_remote: Option<String>, pub upstreams: BTreeMap<String, Upstream>, pub tracking: BTreeMap<(String, String), String>, pub revs: BTreeMap<String, String> }` — `head` rama corta (`None` = detached); `tracking[(remote, refs/heads/x)]` = oid de la copia; `revs[nombre tal cual]` = oid.
- `pub struct Upstream { pub remote: String, pub merge: String }`
- `pub enum Transition { Ref { remote: Option<String>, refname: String, old: Option<String>, new: Option<String> }, Rebase { refname: String, base: String } }`
- `pub struct Binding { pub operation: LoggedOperation, pub transitions: Vec<Transition> }`
- `pub fn parse(argv: &[String]) -> Result<Request, Unsupported>`
- `pub fn resolve(req: &Request, facts: &RepoFacts) -> Result<Binding, Unsupported>`
- `pub fn exceptable(rule: Rule) -> bool` (`match` exhaustivo, sin `_`)
- `pub fn announce(reasons: &[Reason], wants_prior: bool) -> Result<Vec<Rule>, ExceptionRefusal>` (orden de aparición sin duplicados, `SnapshotFailed` al final si `wants_prior`; `NotExceptable` si alguna no es saltable; `NotDenied` si queda vacía)
- `pub fn covers(announced: &[Rule], reasons: &[Rule]) -> bool`
- Todo `Debug, Clone, PartialEq, Eq`.

**`crates/core/src/guardrails/exception.rs`**
- `pub use gitraptor_api::guard::ExceptionToken;`
- `pub const TOKEN_TTL: Duration = 60 s; pub const CHILD_WAIT: Duration = 2 s; pub const CONSUMED_MAX: Duration = 10 min;`
- `pub struct ExceptionPlan { pub worktree: String, pub binding: Binding, pub rules: Vec<Rule> }` + `pub fn view(&self) -> ExceptionPlanView`
- `pub struct Bound { pub action_id: String, pub repo_id: String, pub requester: Requester, pub plan: ExceptionPlan }`
- `pub struct Presentation<'a> { pub repo_id: &'a str, pub worktree: &'a str, pub token: &'a ExceptionToken, pub git: (u32, u64), pub git_parent: Requester, pub transitions: &'a [Transition], pub reasons: &'a [Rule] }`
- `pub enum Presented { Applied { action_id: String, first: bool }, Rejected(Rejection) }`
- `pub enum Rejection { Unknown, Expired, OtherWorktree, ChildNotBound, NotTheChild, OtherTransition, RuleNotAnnounced }`
- `pub fn digest(token: &str) -> [u8; 32]` (Sha256) · `pub fn ct_eq(a: &[u8; 32], b: &[u8; 32]) -> bool` (fold XOR de los 32 bytes, sin salida temprana)
- `pub struct Tokens` (`Mutex<HashMap<String /*repo_id*/, Slot>>` + `Condvar` + `child_wait`); `Default` = `new(CHILD_WAIT)`; `Debug` manual que solo cuenta entradas.
- `impl Tokens`: `pub fn new(child_wait: Duration) -> Self`; `pub fn issue(&self, bound: Bound, now_ms: i64) -> Option<ExceptionToken>` (`None` si `getrandom::fill` falla); `pub fn bind_child(&self, action_id: &str, requester: Requester, child: (u32, u64)) -> Result<(), ExceptionRefusal>`; `pub fn present(&self, p: &Presentation<'_>, now_ms: i64) -> Presented`; `pub fn end(&self, action_id: &str, requester: Requester) -> Result<bool, ExceptionRefusal>`; `pub fn sweep(&self, now_ms: i64) -> Vec<String>`; `pub fn digest_of(&self, action_id: &str) -> Option<[u8; 32]>`.
- Orden de `present` (fija qué `Rejection` sale): slot del repo + `ct_eq` → `Unknown`; sin consumir y `now_ms > issued + TTL` → `Expired`; `worktree` → `OtherWorktree`; hijo sin ligar tras esperar `child_wait` en el `Condvar` → `ChildNotBound`; `git`/`git_parent` ≠ hijo/solicitante → `NotTheChild`; transición no ligada (rebase: misma ref y base) → `OtherTransition`; `!covers(rules, reasons)` → `RuleNotAnnounced`; si no, consume bajo el `Mutex`: `first = true` solo la primera vez.

**Archivos existentes que el esqueleto toca (solo añadidos que compilan)**: `crates/policy/src/guard/mod.rs` y `crates/core/src/guardrails/mod.rs` (+`pub mod exception;`), `pending.rs` (`pub fn note_exception(&mut self, repo_id: &str, action_id: &str, plan: ExceptionPlan) { todo!() }`), `registry.rs` (`pub fn exceptions(&self) -> &super::exception::Tokens { todo!() }`).

## Supuestos

- S1 · `PendingKind::Exception` y `LogKind::{Exception, ExceptionRejected, ExceptionCancelled}` pasan a **PR-B**: sus `match` exhaustivos viven en `crates/core/src/daemon/guard.rs` y `apps/cli/src/guard.rs`, ambos en `deny_paths`. En PR-A, `PendingActions::note_exception` rellena `Entry.exception` y `ExceptionPlan::view` da la vista; que `guard.status` la muestre (con `PendingKind::Exception`) es de PR-B, y hasta entonces `PendingAction.exception` siempre es `None`. GRD006-PENDING-PLAN prueba solo `note_exception` + `Entry.exception` + `ExceptionPlan::view` (PC-2).
- S2 · Campos nuevos en tipos existentes siguen la convención del tipo: `EvaluateParams.exception_token` sale como `exception_token` (el tipo es snake_case); `PendingAction.exception`; `GuardLogSummary.exceptionsRejected`. Tipos nuevos en camelCase como § 5 (`remainingMs`, `ttlMs`, `actionId`).
- S3 · `GUARD_EXCEPTION_REFUSED` se declara en PR-A pero entra en `GROUP.errors` en **PR-B** junto a `error.guard-exception-refused` de `apps/cli/i18n/{en,es}/contract.txt`. Métodos y capacidad sí entran en `GROUP` en PR-A (§ 5).
- S4 · Los tres métodos entran en `GROUP` en PR-A declarados por adelantado (`implemented_by: Some("US-GRD-006")`, PC-5); PR-B lo quita al darles brazo en `conn.rs`. Hasta entonces caen en el brazo `_` de `conn.rs:972`: auditados y `NOT_IMPLEMENTED`. La API real solo tiene `pending(name, story)`, que fuerza `reserved: true` y por eso no sirve para `guard.exception-end`: el build añade en `crates/api/src/methods/mod.rs` `pub(crate) const fn ahead(self, story: &'static str) -> Self` y ajusta su test `pending_methods_name_their_story` (hoy exige `reserved` y prefijo `US-GRP-` fuera de `timemachine.`) para admitir los métodos `guard.exception*` con `US-GRD-006`. La capacidad se anuncia en `hello` sin implementación: no se corta release entre A y B.
- S5 · `pull` es `Unsupported::Form` (D2 literal); alias y subcomandos no admitidos son `Operation`; `branch` sin `-d/-D/--delete` es `Operation`.
- S6 · Opciones de `push` admitidas: lista cerrada `-f`, `--force`, `--force-with-lease[=…]`, `--force-if-includes`, `--delete`, `-d`; cualquier otra (`--all`, `--mirror`, `--tags`, `--no-verify`, `-u`) es `Form`. Remoto URL (`://`, forma scp `x:`, empieza por `/`, `.` o `~`) es `Form`; remoto fuera de `facts.remotes` es `Form` en `resolve`.
- S7 · `resolve`: `Ref.old` = copia de seguimiento (`None` si no hay = creación); `Ref.new` = `revs[src]` (`None` = borrado); `src = HEAD` va a la rama de `head`. Refs completas como `refs/heads/<x>` con `refs::branch_ref`.
- S8 · `RefChange` del `LoggedOperation`: `Delete` sin `new`, `Force` con `force` o `+`, `Create` sin `old`, si no `Update`.
- S9 · `issue` devuelve `Option` (la spec: `-> ExceptionToken`): sin CSPRNG no hay token (fail-closed). `end` devuelve `Result<bool, ExceptionRefusal>` para distinguir `no-pending` y `not-the-requester`. Un segundo `bind_child` de la misma acción es `ChildNotOurs`. Un `issue` sobre un repo con token lo sustituye (un pendiente por repo).
- S10 · Un token consumido vive hasta `end` o `CONSUMED_MAX` (10 min) desde el consumo; `sweep` quita además los no presentados con TTL vencido.
- S11 · Tiempo constante: `ct_eq` público. La prueba fija su tabla de verdad y que un token que difiere solo en el primer o el último carácter se rechaza; que `present` pase por `ct_eq` lo comprueba la revisión (no es observable de forma fiable por tiempos).
- S12 · Que `ExceptionToken` no implemente `Serialize` lo garantiza el compilador y la revisión; la prueba cubre `Debug` y la validación al leer. `PartialEq` derivado de `ExceptionToken` no es de tiempo constante: lo exige `EvaluateParams: PartialEq` y el almacén no lo usa (compara digests con `ct_eq`).
- S14 · Que `Slot` (el estado por repo de `Tokens`) no guarda el secreto —ningún `String` ni `ExceptionToken` con el token, solo `[u8; 32]`— lo comprueba la revisión; GRD006-TOKENS-HASH-ONLY prueba solo lo observable (PC-4).
- S13 · Plataformas: PR-A es puro (sin E/S de proceso); el mismo código en Linux, macOS y Windows. La verificación de identidad por plataforma es de PR-B.

### Literales de `EvaluateParams` que gana `exception_token: None` (PC-1)

Ninguno en `apps/**`. `crates/core/src/guardrails/hook.rs` sale de `deny_paths` solo para esto; el resto no está en ningún `deny_path`.

- `crates/core/src/guardrails/hook.rs:495`, `:600`, `:779`
- `crates/core/src/guardrails/config_guard.rs:119` · `crates/core/src/guardrails/link_late_tests.rs:43` · `crates/core/src/detect/hook.rs:328`
- `crates/core/tests/us_grd_017.rs:141` · `us_grd_007.rs:107` · `us_grd_007_actor.rs:79` · `us_grd_010.rs:220` · `ts_ckp_003_log_layers.rs:135` · `ts_ckp_003_deadlock.rs:206` · `us_grd_012_config_guard.rs:174` · `us_grd_012_config_bypass.rs:85`, `:309` · `guard_log.rs:36`, `:185`, `:241` · `td_grd_001_evaluate.rs:88` · `guard_evaluate.rs:33`, `:187`, `:376`, `:592`

### Convenciones observadas

- Pruebas de integración por historia en `crates/<crate>/tests/us_grd_0xx*.rs`, solo API pública y sin E/S en policy: `crates/policy/tests/us_grd_007_permissions.rs` (cabecera `//!`, oids `const A`/`B`).
- Helpers de core en `crates/core/tests/common/mod.rs` (`TempProfile`, `git`, `init_repo`): las pruebas `tokens_*` no los necesitan.
- Tablas de pendientes probadas con `const ME: Requester` y relojes en ms inyectados: `crates/core/src/guardrails/pending.rs::tests`.
- Arquitectura del contrato: `crates/api/tests/architecture.rs::{the_method_registry_declares_no_method, the_shared_error_list_is_frozen, no_method_waits_for_a_newer_protocol}`.
- Flags de método: `crates/api/src/methods/mod.rs::{spec, METHODS, GROUPS, MethodSpec}`; SEC-14 en `mcp_never_gets_a_reserved_command`.
- Errores por módulo con sus textos: `apps/cli/src/codes.rs::every_contract_code_has_both_messages` (motivo de S3).
