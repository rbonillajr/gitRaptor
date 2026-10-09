---
title: "Brief de implementación — configuración por niveles: endurecer sin relajar (US-GRD-010) y configuración protegida frente a agentes (US-GRD-012)"
type: dev-brief
status: draft
created: 2026-10-08
updated: 2026-10-08
feature: guardrails
stories: [US-GRD-010, US-GRD-012]
related:
  adrs: [ADR-GRP-007, ADR-GRP-008, ADR-GRD-003, ADR-GRD-004, ADR-GRD-006, ADR-GRD-008, ADR-GRP-016]
  rules: [BR-CONS-001, BR-VAL-001, BR-AUTH-004, BR-AUTH-001, BR-EDGE-004]
  specs: [DS-TS-GRD-001, DS-US-GRD-008, DS-US-GRD-018]
contract: docs/dev-briefs/layered-config.contract.json
---

# Brief — configuración por niveles (US-GRD-010 + US-GRD-012)

Autor: `rust-architect`. Ejecuta: `rust-expert`. Rama: `feat/US-GRD-010-012-layered-config`.
Los briefs anteriores vivían en `.claude/dev-briefs/`; este va en `docs/dev-briefs/` porque así lo pidió el orquestador.

## 1. Contexto y objetivo

- **US-GRD-010**: el perfil (`settings.json`) y el nivel local (`<config>/repos/<id-repo>/settings.local.json`, ADR-GRP-008) endurecen las reglas del equipo y nunca las relajan. Entre los dos personales gana el local (BR-CONS-001).
- **US-GRD-012**: un agente no puede commitear cambios en `.gitraptor/` (BR-AUTH-004, Q-GRD-7). Una edición sin commitear no cuenta (Q-GRD-17). Una configuración laxa commiteada en el worktree de un agente tampoco cuenta (Q-GRD-20).
- **Requisito añadido por el orquestador**: si un agente edita la configuración local personal o la del repo para relajar una regla, la regla efectiva no cambia y el intento queda en el registro de decisiones.

## 2. Qué existe y qué falta

| Pieza | Estado en `main` (d84c1990) | Delta de este brief |
|---|---|---|
| Suelo y worktree leídos de objetos commiteados, nunca del working tree | **Hecho**: `crates/policy/src/team.rs:TeamLoader::load` (TS-GRD-001) | Ninguno. Q-GRD-17 y Q-GRD-20 se cumplen por construcción |
| El worktree solo endurece (sin `allow` ni `disableSafeMinimum`) | **Hecho**: `team.rs:combine` (`with_allow = false` para los endurecedores) | Ninguno |
| Rama base solo del suelo; `engine.baseBranch` fuera de nivel se quita al parsear | **Hecho**: `team.rs:TeamLoader::load` (§ base branch) y `settings/document.rs` (`Code::KeyNotAllowedAtLevel`) con `x-gitraptor-levels = ["team"]` en `settings/model.rs:Engine::base_branch` | Test de contrato con un nivel **local** |
| Suelo confirmado y relajación pendiente (D7) | **Hecho**: `team.rs:EffectivePermissions::relaxes`, `Code::FloorRelaxPending` | Ninguno |
| Unión de ramas protegidas y rutas prohibidas (suelo, suelo confirmado, worktree, perfil) | **Hecho**: `crates/core/src/guardrails/policies.rs:policies` + `crates/policy/src/guard/policies.rs:combine` | Añadir el nivel **local** |
| Autoría: solo el suelo relaja a `flexible` | **Hecho**: `crates/policy/src/guard/authorship.rs:combine`, `crates/core/src/guardrails/authorship.rs:policy` | Añadir el local con "local antes que perfil" |
| Lector del nivel local `settings.local.json` | **No existe** (`core/guardrails/policies.rs` y `authorship.rs` lo declaran pendiente en su doc) | **Nuevo** |
| Permisos por operación con los niveles personales (local > perfil, nunca bajo el equipo) | **No existe**: `team.rs:combine` solo combina el suelo y el worktree | **Nuevo**: función pura de combinación |
| Aplicar los permisos en la decisión (`rebase: ask`, `push: deny`) | **No existe**: `crates/policy/src/guard/mod.rs:evaluate` aplica solo el mínimo y las políticas. Es US-GRD-007 (`draft`) | **Fuera de alcance** (ver § 9) |
| Límite de diff (filas 5 y 6 de US-GRD-010) | **No existe**: la clave es de US-GRD-009 (`draft`) | **Fuera de alcance** (ver § 9) |
| `.gitraptor/` como ruta prohibida para agentes por defecto | **No existe** | **Nuevo**: regla de producto `policy.config-protected` |
| Registro del intento de relajar | **No existe** (`authorship::combine` produce `RelaxationNotAllowed`, pero `core` lo descarta) | **Nuevo**: aviso `config.relax-ignored` en el registro |

> **Dependencia declarada y no cumplida**: US-GRD-010 lista US-GRD-007 como dependencia. US-GRD-007 está en `draft`, así que el guard todavía no aplica los permisos de la configuración, solo el mínimo. Este brief entrega la **combinación**, que US-GRD-007 consumirá. Las filas 1 a 4 se verifican sobre el **valor efectivo**, que es lo que pide el criterio ("el valor efectivo es …"). Mientras falte US-GRD-007, la decisión real es **más** estricta que ese valor: el mínimo sigue denegando force-push. Es la dirección segura.

### Convenciones observadas

- Un solo cargador del nivel de equipo, que lee objetos commiteados y nunca el working tree: `crates/policy/src/team.rs:TeamLoader::load`.
- Los endurecedores no aportan `allow` ni `disableSafeMinimum`, y el efecto es el máximo `deny > ask > allow`: `crates/policy/src/team.rs:combine`.
- `Permission` está ordenado por restricción (`Allow < Ask < Deny`): `crates/policy/src/team.rs:Permission`.
- Una relajación se mide comparando decisiones normalizadas, no documentos: `crates/policy/src/team.rs:EffectivePermissions::relaxes`.
- La rama base sale solo del suelo, y Guardrails protege la unión mientras haya un cambio pendiente: `crates/policy/src/team.rs:TeamConfig::guarded_base_branches`.
- Los niveles admitidos se declaran en el schema con `x-gitraptor-levels`: `crates/policy/src/settings/model.rs:Permissions::disable_safe_minimum` y `Engine::base_branch` (solo `team`).
- Una clave fuera de nivel se quita sola y deja un diagnóstico con puntero JSON, sin contenido: `crates/policy/src/settings/document.rs` (`Code::KeyNotAllowedAtLevel`) y `crates/policy/src/settings/diagnostic.rs:pointer`.
- Los diagnósticos son códigos estables sin valores leídos: `crates/policy/src/settings/diagnostic.rs:Code` (`RelaxationNotAllowed`, `FloorOnlyKey`).
- Las listas se unen, cada regla conserva su nivel y su `appliesTo`, y nadie quita el patrón de otro: `crates/policy/src/guard/policies.rs:combine`.
- Una regla `agents` solo deniega con un actor agente: `crates/policy/src/guard/policies.rs:Scope::governs`.
- Lo que no se puede leer o contar deniega, nunca "no coincide": `crates/policy/src/guard/policies.rs:forbidden_paths` (`Touched::unverifiable`, `Cause::Unverifiable`).
- Se nombran todas las razones del efecto máximo (BR-CALC-001): `crates/policy/src/guard/mod.rs:Evaluation::add`.
- Las razones de las políticas se evalúan en su propia pasada, sin depender de los `return` tempranos del mínimo: `crates/policy/src/guard/mod.rs:policies_of`.
- Solo el suelo confirmado y legible relaja: `crates/policy/src/guard/authorship.rs:Source::may_relax` y `crates/core/src/guardrails/authorship.rs:floor_may_relax`.
- Si el equipo no se puede leer, el movimiento de un agente se deniega: `crates/core/src/guardrails/policies.rs:policies` (`Policies::unreadable`).
- Los commits de un movimiento solo se leen si alguna regla de rutas gobierna al actor: `crates/core/src/guardrails/policies.rs:touched` (`Policies::needs_paths`).
- El perfil se lee en cada uso, sin seguir enlaces y con tope de 64 KiB: `crates/core/src/profile/settings.rs:profile_settings` y `open_no_follow`.
- El registro de repos protegidos usa como clave el `repo_id` del índice del perfil: `crates/core/src/guardrails/install.rs:publish`. Una entrada solo vale si su `common_dir` coincide: `crates/core/src/guardrails/evaluate.rs:serve_logged`.
- `ask` se aplica como `deny`, y un aviso solo viaja con un `allow`: `crates/core/src/guardrails/evaluate.rs:decision`.
- Una entrada del registro es una denegación o un aviso, y nunca lleva argv, oids ni contenido (M-06): `crates/core/src/guardrails/log.rs:entry`.
- Un cambio de forma lleva una capacidad declarada en el `GROUP` del módulo: `crates/api/src/methods/guard.rs:GROUP` (`CAP_GUARD_POLICIES`). El daemon la consulta con `self.has(...)` en `crates/core/src/channel/conn.rs:guard_caller` y `guard_log` (allí con `retain` por `LogKind`).
- Las reglas viajan como código kebab con prefijo de familia (`policy.*`, `authorship.*`): `crates/api/src/guard.rs:Rule`.
- El cliente traduce cada par `(Rule, Cause)` con una plantilla fija, y cada regla del registro con `guard.log.rule.*` en `apps/cli/i18n/{en,es}/guard.txt`: `apps/cli/src/guard.rs` (los dos `match` sobre `reason.rule`).
- Lo nuevo va en archivos propios, y en los archivos compartidos solo se añaden líneas de registro: `docs/architecture/extender-sin-archivos-compartidos.md` (ADR-GRP-016).
- Las pruebas e2e usan un agente simulado, perfil y repos temporales y ninguna espera fija, y son `#![cfg(unix)]`: `apps/cli/tests/guard_us_grd_008.rs:Machine`.
- Las pruebas del daemon sin canal usan `serve_as` con un `GuardRegistry` y un `Confirmed` explícito: `crates/core/tests/guard_evaluate.rs:commit_authorship::registry`.

### Mejoras detectadas

1. **Dos cachés del mismo cargador**: `core/guardrails/policies.rs:LOADER` y `core/guardrails/authorship.rs:LOADER`. **Se corrige aquí**: queda un solo `LOADER` en `guardrails/layers.rs` (decisión D3).
2. **Diagnósticos de autoría descartados**: `core/guardrails/authorship.rs:policy` usa solo `.effective` de `authorship::combine`. **Se corrige aquí**: alimentan la detección de relajaciones (D7).
3. **Comentario desactualizado**: `crates/policy/src/guard/mod.rs` (doc del módulo) dice "US-GRD-001 evaluates the safe minimum only: no configuration is read". **Se corrige aquí** (una línea, en el slice de `crates/policy`).
4. **`configStatus` fijo en `NotRead`**: `core/guardrails/evaluate.rs:decision`, aunque desde US-GRD-008 y US-GRD-018 la configuración sí se lee. Viola ADR-GRD-003 § 3. **No se corrige aquí**: queda para US-GRD-011, que define la reacción por estado de fuente.
5. **`MAX_BYTES` duplicado**: `core/profile/settings.rs:MAX_BYTES` y `policy/settings/strict.rs:MAX_BYTES`. **Se corrige aquí** al refactorizar el lector (D4): se usa el de `policy`.
6. **Dos arneses e2e de Guardrails**: `apps/cli/tests/guard_machine/mod.rs` y el `Machine` propio de `guard_us_grd_008.rs`. No se unifican aquí; la suite nueva copia el de US-GRD-008 (§ 8).
7. **El mínimo no lee `disableSafeMinimum`**: `policy/guard/mod.rs:evaluate` deniega force-push siempre. Es más estricto que ADR-GRD-003 § 2. Lo cierra US-GRD-007; lo anoto porque las filas 2 y 3 de US-GRD-010 dan `permitir` como valor efectivo y el guard sigue denegando.

## 3. Decisiones de arquitectura

Cada decisión lleva su motivo. **(ADR)** = ya fijada por un ADR o por una regla. **(NUEVA)** = la tomo aquí y la tiene que validar Rene (§ 13).

- **D1 (ADR)**: la fuente de relajaciones es solo el suelo confirmado y legible: `team.rs:TeamLoader::load` con `Confirmed` (ADR-GRD-004 § 2 y § 4, D6 y D7). No se toca.
- **D2 (NUEVA, aplica BR-CONS-001)**: semántica de combinación por clave. "Personal" es el valor del local si lo declara y, si no, el del perfil. "Equipo" es el resultado de `team.rs:combine`: suelo efectivo, mínimo, suelo endurecedor y worktree.

  | Clave | Combinación | Dónde |
  |---|---|---|
  | Permiso de cada operación | Por operación: `personal = declarado(local) ?? declarado(perfil)`, donde `declarado` es la lista más restrictiva que nombra la operación. `efectivo = máx(equipo, personal)`. Un `allow` personal nunca baja | `policy/layers.rs:harden` |
  | `disableSafeMinimum` | Solo el suelo, ya resuelto. En perfil o local lo quita el parser | existente |
  | Ramas protegidas | Unión de todas las fuentes, incluido el local. Ninguna quita patrones de otra | existente + local |
  | Rutas prohibidas | Unión, más la regla de producto `/.gitraptor/` (D5) | existente + local + D5 |
  | `commitAuthorship` | `personal = local ?? perfil`. `efectivo` = el máximo de `authorship::combine` sobre {suelo (`may_relax`), worktree, personal} | `core/guardrails/layers.rs` |
  | Rama base | No se combina: solo el suelo y la confirmada (ADR-GRD-004 § 3). En perfil o local se ignora y se registra (D7) | existente |
  | Límite de diff | `mín(equipo, personal)`. **No se construye**: la clave es de US-GRD-009 | § 9 |
  | Formato de commit, plazo de la cola | El del equipo si lo fija, o el personal; el plazo es el menor. **No se construye** | § 9 |

  **Motivo de "local ?? perfil" y no "máximo de todos"**: la fila 3 de US-GRD-010 (equipo permite, perfil deniega, local permite → **permitir**) solo sale así.
- **D3 (NUEVA)**: un único punto de carga por evaluación, `core/guardrails/layers.rs:load`, que lee el equipo (con un solo `LOADER`), el perfil y el local. `policies.rs` y `authorship.rs` pasan a ser envolturas finas. **Motivo**: un solo criterio para las cinco fuentes, y se elimina la caché duplicada (Mejora 1).
- **D4 (ADR-GRP-008)**: el local está en `ProfileDirs.config/repos/<repo_id>/settings.local.json`. El `repo_id` sale **solo** del registro del daemon, de una entrada cuyo `common_dir` coincide (`evaluate.rs:serve_logged`), nunca del texto del cliente. Se valida otra vez (hex y `-`, de 1 a 64 caracteres). Se lee como el perfil: sin seguir enlaces, solo archivo regular y tope de 64 KiB. Un repo que no está en el registro → local `Absent`. **Motivo**: el `repo_id` del hook no es de fiar, y el registro ya lo ata al directorio común.
- **D5 (NUEVA, aplica BR-AUTH-004)**: regla de producto **`policy.config-protected`**, nivel `minimum`, ámbito `agents`.
  - **Qué deniega**: toda transacción o push de un **agente** cuyos commits nuevos modifiquen, creen o borren algo bajo `/.gitraptor/`: el directorio anclado en la raíz, no solo `settings.json`.
  - **Cómo detecta**: reutiliza `Touched` y el matcher de US-GRD-008 D5/D6, con la normalización NFC, de mayúsculas, de HFS+ y de NTFS.
  - **No se puede desactivar**: ninguna clave la apaga, tampoco `disableSafeMinimum`.
  - **La persona pasa**: un actor "sin atribuir" cumple el escenario 6.
  - **Motivo de reutilizar D5**: es el mecanismo de "un commit toca una ruta" que ya pasó la revisión de seguridad, y evita un segundo criterio.
  - **Consecuencias**:
    - **Coste**: los commits de **todo** movimiento de un agente se leen siempre (antes, solo si había `forbiddenPaths`).
    - **Fail-closed**: más de 256 commits nuevos se deniegan como `unverifiable` a un agente aunque no haya reglas de rutas.
- **D6 (NUEVA)**: el **intento de relajar** se detecta con una función pura, `policy/layers.rs:ignored_relaxations`. Compara lo que declara cada fuente que solo endurece (worktree, y el personal en vigor, `local ?? perfil`) con el valor de **equipo**. Cuenta como intento:
  - un permiso declarado menor que el del equipo, con clave `Permission(op)`;
  - `disableSafeMinimum: true` en el worktree con el mínimo activo, o quitado por nivel en el perfil o el local (diagnóstico `KeyNotAllowedAtLevel` en `/permissions/disableSafeMinimum`), con clave `SafeMinimum`;
  - `engine.baseBranch` distinta en el worktree (la condición de `FloorOnlyKey` en `team.rs`), o en el perfil o el local (`KeyNotAllowedAtLevel` en `/engine/baseBranch`), con clave `BaseBranch`;
  - `RelaxationNotAllowed` de `authorship::combine`, con clave `CommitAuthorship`.

  **No cuenta**:
  - el local que relaja un endurecimiento del **perfil** sin bajar del equipo, porque es la fila 3, válida;
  - el valor del perfil que el local tapa;
  - las listas vacías, porque una unión no se relaja.

  **No cambia nunca la decisión**: es solo observación.
- **D7 (NUEVA)**: dónde se registra el intento.
  - **Cuándo**: en las evaluaciones `ref-transaction` y `push` que ya cargan la configuración (`caller.policies`), con un actor **agente** y fuera del ejecutor.
  - **Qué**: el daemon escribe una **segunda entrada**, de tipo `notice`, con el mismo `decision_id`, la regla nueva **`config.relax-ignored`** y una razón por nivel distinto (`worktree`, `profile` o `local`), sin parámetros.
  - **Motivos**:
    - no se mezcla con las razones que producen el efecto (BR-CALC-001);
    - no cuenta en el KPI de bloqueos (solo `denial`);
    - no viaja al cliente del hook, así que el agente no recibe pistas sobre la configuración.
  - **El commit queda cubierto**: Git emite una `ref-transaction` por cada commit en rama, así que un commit deja **un** aviso, no dos.
- **D8 (NUEVA)**: las ediciones **sin commitear** de `.gitraptor/settings.json` ni rigen ni se registran. **Motivo**: ADR-GRD-004 Validación 4 (el working tree no se lee y no da aviso). Además, quien las escribe no se puede atribuir, y la persona edita ese archivo legítimamente antes de su commit. El intento queda registrado cuando el agente intenta **commitearlo** (denegación `policy.config-protected` con su actor, escenario 1).
- **D9 (ADR-GRP-016)**: capacidad nueva **`guard.config-protection`**.
  - **Con la capacidad**: el daemon envía las reglas nuevas.
  - **Sin la capacidad**, la protección **se aplica igual**:
    - en la decisión, `policy.config-protected` se envía como `policy.forbidden-path` con `path` y `pattern` = `/.gitraptor/`;
    - en `guard.log`, las razones `config-protected` se reescriben así y se quitan las entradas que llevan `config.relax-ignored`.
  - **Motivo**: un cliente antiguo no deserializa variantes desconocidas (`deny_unknown_fields`, enum cerrado), y una denegación nunca se degrada a `allow`.
- **D10 (ADR-GRD-003 § 4)**: modo degradado sin cambios. El actor es "sin atribuir", así que D5 no aplica. El residuo `policy-actor` se amplía a la configuración.
- **D11 (NUEVA)**: la persona commitea la configuración del equipo **sin un paso de confirmación extra** en el commit. La "confirmación consciente" del escenario 6 es su propio commit. La relajación solo rige al llegar a la rama principal y confirmarla en la máquina (D7 de ADR-GRD-004, US-GRD-014). **Motivo**: pedir confirmación en el commit no protege nada que D7 no proteja ya, y exige el mecanismo de US-GRD-006 (`draft`).

## 4. Contratos

### 4.1 `crates/api` (slice A)

```rust
// crates/api/src/guard.rs, en enum Rule (después de ForbiddenPath)
/// An agent's commit changes the Guardrails configuration (`.gitraptor/`, BR-AUTH-004). A
/// product rule: no key turns it off. Only with `guard.config-protection`; without it the
/// daemon sends it as `policy.forbidden-path`.
#[serde(rename = "policy.config-protected")]
ConfigProtected,
/// A level that only hardens (worktree, profile, local) tried to relax a team rule; it was
/// ignored. Only in decision-log notices, never in a decision sent to a hook.
#[serde(rename = "config.relax-ignored")]
RelaxIgnored,

// crates/api/src/methods/guard.rs
/// The configuration protection (US-GRD-012): `policy.config-protected` in decisions and
/// `config.relax-ignored` notices in `guard.log`.
pub const CAP_GUARD_CONFIG_PROTECTION: Capability = Capability::new("guard.config-protection");
// y en GROUP.capabilities, tras CAP_GUARD_POLICIES
```

Doc de `NotPreventable::PolicyActor`: añadir "and the protection of the Guardrails configuration". No hay código de error nuevo, `LogKind` nuevo ni campo nuevo.

### 4.2 `crates/policy` (slice B)

```rust
// crates/policy/src/layers.rs (NUEVO) — pub mod layers; en lib.rs
use gitraptor_api::guard::Level;
use crate::settings::document::Parsed;
use crate::settings::model::{Operation, Settings};
use crate::team::{EffectivePermissions, Permission, TeamConfig};

/// The two personal levels as read (BR-CONS-001): the profile's `settings.json` and the repo's
/// `settings.local.json`. `None` = absent or ignored.
#[derive(Debug, Clone, Copy, Default)]
pub struct Personal<'a> { pub profile: Option<&'a Settings>, pub local: Option<&'a Settings> }

/// The permission `settings` declares for `op`: the most restrictive of its lists that names
/// it; `None` when none does.
pub fn declared(settings: &Settings, op: Operation) -> Option<Permission>;

/// The team permissions hardened by the personal levels: per operation the personal value is
/// the local one when the local declares it, else the profile one; the effective one is the
/// maximum of the team and that value. Never below `team`. Sources: the team's, plus
/// `RuleSource::Source(SourceKind::Local | Profile)` when the personal value is the maximum.
pub fn harden(team: &EffectivePermissions, personal: Personal<'_>) -> EffectivePermissions;

/// What a level that only hardens tried to relax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelaxKey { Permission(Operation), SafeMinimum, BaseBranch, CommitAuthorship }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IgnoredRelaxation { pub level: Level, pub key: RelaxKey }

/// Every relaxation the worktree and the personal level in force declared against the team
/// and that was ignored (D6). Sorted and deduplicated. Pure: never changes a decision.
/// `floor_may_relax`: the floor is the confirmed one and fully readable (authorship, D2).
pub fn ignored_relaxations(
    team: &TeamConfig,
    profile: &Parsed,
    local: &Parsed,
    floor_may_relax: bool,
) -> Vec<IgnoredRelaxation>;
```

`level` toma solo `Level::Worktree`, `Level::Profile` o `Level::Local`. `Operation` necesita `Hash` (ya lo tiene) y `Ord` (ya lo tiene).

```rust
// crates/policy/src/guard/config.rs (NUEVO) — pub mod config; en guard/mod.rs
/// The Guardrails configuration directory, anchored at the root (D5).
pub const CONFIG_PATTERN: &str = "/.gitraptor/";

/// BR-AUTH-004: an agent's movement whose new commits touch `/.gitraptor/` is denied with
/// `policy.config-protected` (level `minimum`, params `path` = first such path, `pattern`);
/// `touched.unverifiable` with an agent → deny with `Cause::Unverifiable`, no params. The
/// person (`actor = None`) is never governed.
pub fn protect_config(
    out: &mut Evaluation,
    touched: &Touched,
    actor: Option<AgentKind>,
    budget: &mut Budget,
);
```

Cambios mínimos en archivos compartidos de `crates/policy`:

- `guard/mod.rs:policies_of`: primero `if !refs::is_governed(refname) { return; }`. Después, `if let Some(t) = touched { config::protect_config(out, t, ctx.actor, budget) }`. Luego la guarda `ctx.policies.is_empty()` y lo que ya había.
- `guard/policies.rs:Policies::needs_paths`: `actor.is_some() || <lo de ahora>`. `everyone_only` no cambia.

### 4.3 `crates/core/src/profile` (slice C)

```rust
// crates/core/src/profile/settings.rs
pub const LOCAL_SETTINGS_FILE: &str = "settings.local.json";
/// `<config>/repos/<repo_id>/settings.local.json` (ADR-GRP-008); `None` when `repo_id` is not
/// 1..=64 of `[0-9a-fA-F-]`.
pub fn local_settings_path(dirs: &ProfileDirs, repo_id: &str) -> Option<PathBuf>;
/// The local document of a repo, read like the profile one (no link followed, regular file,
/// 64 KiB, `Level::Local`, `SourceKind::Local`). Absent when the id is invalid or there is no
/// file. Never creates the file or the folder.
pub fn local_settings(dirs: &ProfileDirs, repo_id: &str) -> Parsed;
```

Refactor interno: `profile_settings` y `local_settings` comparten `fn read_settings(path: &Path, level: Level, kind: SourceKind) -> Parsed`, con el tope `gitraptor_policy::settings::strict::MAX_BYTES` (Mejora 5).

### 4.4 `crates/core/src/guardrails` y canal (slice D)

```rust
// crates/core/src/guardrails/layers.rs (NUEVO) — pub mod layers; en guardrails/mod.rs
/// The one team loader of the evaluation (bounded cache, shared by the connection threads).
pub(crate) static LOADER: LazyLock<TeamLoader> = LazyLock::new(TeamLoader::default);

/// The five sources of one evaluation (ADR-GRD-003 § 1): team (floor, confirmed floor,
/// worktree), profile and local.
#[derive(Debug, Clone)]
pub struct Layers { pub team: TeamConfig, pub profile: Parsed, pub local: Parsed, confirmed: Option<Confirmed> }

/// Reads every level for the worktree `reader` was opened on. `repo_id` is the registry key
/// (never the client's text); `None` → no local level. Errors only when the team level cannot
/// be read at all (the callers keep their fail-closed answers).
pub fn load(
    reader: &RepoReader,
    confirmed: Option<&Confirmed>,
    profile: Option<&ProfileDirs>,
    repo_id: Option<&str>,
) -> Result<Layers, gitraptor_git::ReadError>;

impl Layers {
    /// Team permissions hardened by the personal levels (`policy::layers::harden`).
    pub fn permissions(&self) -> EffectivePermissions;
    /// Protected branches and forbidden paths: floor, confirmed floor, worktree, profile, local.
    pub fn policies(&self) -> Policies;
    /// `commitAuthorship` in force: floor (may relax only if confirmed and readable), worktree,
    /// personal = local ?? profile.
    pub fn authorship(&self) -> Effective;
    /// `policy::layers::ignored_relaxations` of these sources.
    pub fn ignored(&self) -> Vec<IgnoredRelaxation>;
}
```

- `guardrails/policies.rs`: borrar su `LOADER`. `policies(...)` delega en `Layers`. `policies_for(reader, confirmed, profile, repo_id: Option<&str>)` gana `repo_id`. `degraded` usa `layers::LOADER`.
- `guardrails/authorship.rs`: borrar su `LOADER`. `policy_for(reader, confirmed, profile, repo_id: Option<&str>)` gana `repo_id` y delega en `Layers::authorship`.
- `daemon/authorship.rs`: pasar `Some(&batch.repo_id)` (una línea).
- `guardrails/evaluate.rs`:

  ```rust
  /// What `guard.evaluate` decided, and what only the decision log needs.
  pub struct Served {
      pub decision: Decision,
      /// The authorship policy applied to a commit (`flexible`, …), when one was.
      pub authorship_policy: Option<&'static str>,
      /// Relaxations the levels that only harden declared and that were ignored (D6).
      pub ignored: Vec<IgnoredRelaxation>,
  }
  pub fn serve_audited(registry: &GuardRegistry, params: &EvaluateParams, caller: &Caller) -> Served;
  // serve_logged = (serve_audited().decision, .authorship_policy): misma firma que hoy.
  ```

  - **Rama `RefTransaction | Push if caller.policies`**: un solo `layers::load(..., registry.profile().as_ref(), entry.map(|_| params.repo_id.as_str()))`. De ahí salen `policies` e `ignored`. Si `load` falla, se mantiene `Policies::unreadable()` y `ignored` queda vacío.
  - **Rama `Commit`**: `Layers::authorship`; `ignored` vacío (D7).
- `guardrails/config_guard.rs` (NUEVO), `pub mod config_guard;`:

  ```rust
  /// The `config.relax-ignored` notice of an evaluation (D7): `None` without an agent actor,
  /// under the executor, or with nothing ignored. Same normalized operation, decision id and
  /// effects as the decision; one reason per distinct level, no params.
  pub fn relax_entry(params: &EvaluateParams, decision: &Decision, ignored: &[IgnoredRelaxation], ctx: &LogContext) -> Option<LogEntry>;
  /// A decision for a connection without `guard.config-protection` (D9).
  pub fn legacy_decision(decision: &mut Decision);
  /// A `guard.log` page for a connection without `guard.config-protection` (D9).
  pub fn legacy_log(log: &mut GuardLogResult);
  ```

  `log.rs:normalize` pasa a `pub(crate)`; es el único cambio en `log.rs`.
- `channel/conn.rs`, en el brazo `GUARD_EVALUATE`:
  1. `serve_audited`;
  2. `log_decision(&p, &decision, &caller, policy, &served.ignored)`. Dentro, tras la entrada actual, `config_guard::relax_entry` por el mismo camino `try_reserve` / `guard_record` / `overflow`;
  3. **después de registrar**, `if !self.has(CAP_GUARD_CONFIG_PROTECTION.name) { legacy_decision(&mut decision) }`.

  En `guard_log`, el mismo `if` con `legacy_log`.

### 4.5 `apps/cli` (slice E)

- `apps/cli/src/guard.rs`:
  - en el `match (reason.rule, reason.cause)`:
    - `(Rule::ConfigProtected, Some(Cause::Unverifiable))` → `guard.reason.policy-unverifiable`;
    - `(Rule::ConfigProtected, _)` → `guard.reason.config-protected` con `path`;
    - `(Rule::RelaxIgnored, _)` → `guard.reason.relax-ignored` con `level`;
  - en el `match` del registro: `guard.log.rule.config-protected` y `guard.log.rule.relax-ignored`.
- `apps/cli/i18n/en/guard.txt` y `es/guard.txt`, con las mismas claves y los mismos marcadores:
  - `guard.reason.config-protected = GitRaptor: {path} belongs to the Guardrails configuration: an agent cannot change it.` / `GitRaptor: {path} es de la configuración de Guardrails: un agente no puede cambiarla.`
  - `guard.reason.relax-ignored = GitRaptor: a relaxation in the {level} configuration was ignored.` / `GitRaptor: se ignoró una relajación en la configuración {level}.`
  - `guard.log.rule.config-protected = Guardrails configuration protected` / `configuración de Guardrails protegida`
  - `guard.log.rule.relax-ignored = relaxation ignored` / `relajación ignorada`

  Sin instrucciones para desactivar nada (M-05).

## 5. Topología: slices disjuntos

Orden: **A** → (**B** ‖ **C**) → (**D** ‖ **E**) → **F**. Nunca hay más de dos en paralelo, y los dos de cada par son de crates distintos. Ningún archivo está en dos slices.

| Slice | Crate | Archivos | Depende de |
|---|---|---|---|
| **A** | `crates/api` | `src/guard.rs`, `src/methods/guard.rs` | — |
| **B** | `crates/policy` | NUEVO `src/layers.rs`, `src/lib.rs` (1 línea), NUEVO `src/guard/config.rs`, `src/guard/mod.rs` (`mod` + `policies_of` + doc), `src/guard/policies.rs` (`needs_paths`) | A |
| **C** | `crates/core` (perfil) | `src/profile/settings.rs` | — (en paralelo con B) |
| **D** | `crates/core` (guardrails, canal) | NUEVO `src/guardrails/layers.rs`, NUEVO `src/guardrails/config_guard.rs`, `src/guardrails/mod.rs` (2 líneas), `src/guardrails/policies.rs`, `src/guardrails/authorship.rs`, `src/guardrails/evaluate.rs`, `src/guardrails/log.rs` (1 palabra), `src/daemon/authorship.rs` (1 línea), `src/channel/conn.rs` | A, B, C |
| **E** | `apps/cli` | `src/guard.rs`, `i18n/en/guard.txt`, `i18n/es/guard.txt` | A (en paralelo con D) |
| **F** | docs | § 12 | D, E |

- **Pruebas de contrato**: las escribe en rojo el orquestador antes de A (§ 8). No son de ningún slice, y el guard R4 las protege.
- **Stubs para que compilen**: entran con A a D y **reproducen el comportamiento actual**, no `todo!()`. `harden` devuelve `team.clone()`; `ignored_relaxations` devuelve `vec![]`; `protect_config` no hace nada; `local_settings` devuelve `Parsed::absent()`; `relax_entry` devuelve `None`; `legacy_*` no hacen nada. Así el rojo se debe al comportamiento y no a un pánico.
- **Fuera de los límites**: `crates/git/src/tm_write/**` y `crates/git` entero (no hace falta ninguna lectura nueva: `fresh_commit_paths` ya existe).

## 6. Matriz de plataformas

| Aspecto | macOS | Linux | Windows |
|---|---|---|---|
| Combinación y detección (puras) | Soportado; tests en todas | Soportado (CI ubuntu) | Soportado; `cargo test -p gitraptor-policy` |
| Lector del local | `O_NOFOLLOW` (`open_no_follow`); ruta en `config/` del perfil (ADR-GRP-006) | Igual | `symlink_metadata` y después `open`. TOCTOU y puntos de reparación como residuo, igual que el perfil. **Pendiente de validación** |
| Patrón `/.gitraptor/` | APFS no distingue mayúsculas ni formas NFD: el matcher pliega mayúsculas y normaliza a NFC | Distingue mayúsculas: `.GITRAPTOR/` se deniega igual. Exceso inocuo, documentado | NTFS: puntos y espacios finales normalizados (US-GRD-008 D6). Nombres 8.3: residuo `policy-reach` |
| E2E por el canal (`guard_us_grd_012.rs`) | Soportado | Soportado (CI) | Sin transporte de canal: `#![cfg(unix)]`. **Pendiente: etapa de validación multiplataforma** (`docs/architecture/xplat-pendientes.md`) |
| Tests del daemon sin canal (`us_grd_010.rs`, `us_grd_012_config_guard.rs`) | Soportado | Soportado | Deben compilar y pasar; usan `git` del PATH, como `guard_evaluate.rs` |

## 7. NFR

- **Rendimiento**:
  - Se vuelve a medir `crates/core/tests/guard_evaluate.rs` `policies_cost` en release (2 000 ramas). Con D5, un agente sin reglas de rutas paga la lectura de commits: debe seguir por debajo de **100 ms p95** (ADR-GRD-002 § 5). El resultado va en § 10 de la ficha de US-GRD-012.
  - El local añade, como mucho, un `stat` y una lectura de ≤ 64 KiB por evaluación.
  - Sin caché personal (§ 9).
- **Observabilidad**: el aviso `config.relax-ignored` lleva actor, worktree, rama, operación normalizada y nivel. Se agrega por la clave de `log.rs:LogEntry::agg_key` y respeta el tope en vuelo (D4 de US-GRD-005). No cuenta en el KPI de bloqueos.
- **Seguridad**: ningún valor del documento entra en una razón, un parámetro o un registro (SEC-11). Lo que no se puede leer deniega a un agente (D5) y nunca relaja. El `repo_id` del local sale solo del registro (D4).

## 8. Plan de pruebas (contrato)

**Aislamiento**:

- repos temporales (`gitraptor_testkit::Fixture`, `tempfile`) y perfil temporal (`ProfileDirs::under_root`, `GITRAPTOR_PROFILE_DIR` en debug);
- `HOME` temporal, `GIT_CONFIG_NOSYSTEM=1` y `PATH` acotado;
- nunca este repo ni el perfil real, y ninguna espera fija;
- las E2E exigen `debug_assertions`.

**Archivo 1 — `crates/core/tests/us_grd_010.rs`** (US-GRD-010, sin canal, multiplataforma). Usa `gitraptor_core::guardrails::layers::load`, con `Confirmed { base_branch: main, floor: ConfirmedFloor::Blob(<id del suelo>) }` cuando el equipo relaja:

| Test | Escenario |
|---|---|
| `precedence_row1_team_deny_local_allow_is_deny` | Fila 1. Equipo `deny:[force-push]`, local `allow:[force-push]` → `Deny`, e `ignored()` contiene `(Local, Permission(ForcePush))` |
| `precedence_row2_team_allow_profile_deny_is_deny` | Fila 2. Equipo `{disableSafeMinimum:true, allow:[force-push]}` confirmado, perfil `deny` → `Deny`, con fuente `Profile` |
| `precedence_row3_team_allow_profile_deny_local_allow_is_allow` | Fila 3 → `Allow` e `ignored()` vacío |
| `precedence_row4_team_ask_local_allow_is_ask` | Fila 4 (`rebase`) → `Ask`, e `ignored()` contiene `(Local, Permission(Rebase))` |
| `precedence_row7_protected_branches_union` | Fila 7. `policies()` lleva `main` (`Floor`) y `release` (`Profile`), y `serve_as` deniega a un agente mover `release` |
| `a_local_hardening_does_not_reach_another_clone` | Dos clones con `repo_id` distintos en el registro y el local solo en el primero. En el primero `push` → `Deny`; en el segundo `push` → `Allow` |
| `a_base_branch_in_a_personal_level_is_ignored` | Sin rama base en el equipo, local `engine.baseBranch: develop` → `guarded_base_branches() == [main]`, e `ignored()` contiene `(Local, BaseBranch)` |

Las filas 5 y 6 (límite de diff) **no se escriben**: son de US-GRD-009 (§ 9).

**Archivo 2 — `apps/cli/tests/guard_us_grd_012.rs`** (US-GRD-012, E2E, `#![cfg(unix)]`). Copia el `Machine` de `guard_us_grd_008.rs` y le añade `local_settings(json)`, que escribe en `dirs.config/repos/<repo_id>/settings.local.json` con el `repo_id` de `raptor guard status --json`:

| Test | Escenario | Hoy |
|---|---|---|
| `an_agent_commit_relaxing_the_team_config_is_denied_and_logged` | 1. El commit del agente que cambia `.gitraptor/settings.json` → no se ejecuta; el texto contiene "Guardrails configuration"; `raptor guard log --json` muestra `policy.config-protected` con actor `claude-code`; la rama y el índice quedan intactos | rojo |
| `an_uncommitted_relaxation_changes_nothing` | 2. Edición sin commitear que permite force-push, y force-push del agente → denegado por `minimum.force-push` | verde (control) |
| `a_lax_config_committed_in_the_agent_worktree_relaxes_nothing` | 3. Worktree `feat-x` en un commit de la persona con `{disableSafeMinimum:true, allow:[force-push]}`. Force-push del agente desde `feat-x` → denegado, **y** el registro tiene un `notice` `config.relax-ignored` de nivel `worktree` con actor `claude-code` | rojo (por el aviso) |
| `an_agent_cannot_delete_the_team_config` | 4. Commit del agente que borra `.gitraptor/settings.json` → no se ejecuta, y `git ls-tree HEAD .gitraptor/settings.json` sigue | rojo |
| `an_agent_commit_outside_the_config_goes_ahead` | 5. Commit del agente que solo toca `src/main.rs` → se ejecuta | verde (control) |
| `the_developer_commits_a_team_config_change` | 6. La persona commitea un cambio en `.gitraptor/settings.json` → se ejecuta y queda en su rama | verde (control) |
| `an_agent_relaxing_the_local_config_changes_nothing_and_is_logged` | Requisito del orquestador. El local `allow:[force-push]` lo escribe el agente (`agent_sh` con `printf > <ruta>`); force-push del agente → denegado, y el registro tiene `config.relax-ignored` de nivel `local` con actor `claude-code` | rojo |

**Archivo 3 — `crates/core/tests/us_grd_012_config_guard.rs`** (técnico, sin canal):

| Test | Criterio |
|---|---|
| `without_the_capability_the_reason_reads_as_a_forbidden_path` | `legacy_decision` y `legacy_log` reescriben `config-protected` a `forbidden-path` (con `pattern` `/.gitraptor/`) y quitan los avisos `relax-ignored`; el efecto sigue siendo `deny` |
| `an_unverifiable_movement_by_an_agent_is_denied` | Un agente que mueve una rama con más de 256 commits nuevos y sin reglas de rutas → `deny` `policy.config-protected` `unverifiable`; la persona → `allow` |

**Atención con los controles**: los escenarios 2, 5 y 6 ya están en verde con el código de hoy. `deliver-contract baseline` bloquea un `verify` que ya está en verde, así que hay que **capturar la línea base antes de escribir las pruebas**, como indica el propio script. Si eso no es posible, los tres se quitan del contrato y se quedan en la suite como regresión.

**Pruebas unitarias propias** (no son de contrato; las escribe `rust-expert`):

- `layers.rs`: `declared` con dos listas, `harden` nunca por debajo del equipo, y cada `RelaxKey` de `ignored_relaxations`;
- `guard/config.rs`: anclaje (`a/.gitraptor/x` no, `.GitRaptor/x` sí), borrado y la persona;
- `profile/settings.rs`: enlace, FIFO o no regular, tamaño e id inválido para el local.

**Aceptación**:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p gitraptor-api -p gitraptor-policy
cargo test -p gitraptor-core --test us_grd_010 --test us_grd_012_config_guard --test guard_evaluate
cargo test -p gitraptor-cli --test guard_us_grd_012 --test guard_us_grd_008 --test guard_us_grd_005
```

Regresión obligatoria: `guard_us_grd_008`, `guard_us_grd_018`, `guard_us_grd_005` y `guard_evaluate`.

## 9. No se construye (diferido)

- **Trinquete personal (Q-GRD-32, `personal-relax-pending`)**: es lo único que impide que un agente quite un endurecimiento **del perfil** editando el local, o borrando o corrompiendo el local. Se añade cuando exista la historia del PO y el factor de ADR-GRD-008 (SPIKE-GRD-002).
- **Aplicar los permisos combinados en la decisión** (`push: deny`, `rebase: ask`, `disableSafeMinimum` en el mínimo): se añade con US-GRD-007.
- **Filas 5 y 6 (límite de diff)**, el formato de commit y el plazo de la cola: se añaden con US-GRD-009 y US-GRD-015, con la semántica ya fijada en D2.
- **Leer el working tree para registrar ediciones sin commitear**: rechazado por D8 y por ADR-GRD-004 Validación 4.
- **Atribuir escrituras de archivos del perfil a un agente**: se añade si el observador llega a vigilar el perfil (no hay ADR).
- **Exponer la ruta del local por el canal** (ADR-GRP-008): se añade con US-GRP-013 o US-GRD-013.
- **Nivel local para claves del motor** (`idleThresholdMinutes`): se añade con US-GRP-013, que reutiliza `local_settings`.
- **Mostrar la configuración efectiva y los diagnósticos en `raptor guard status`**: se añade con la historia de estado.
- **La clave relajada dentro de la entrada del registro**: se añade si el contrato del registro llega a admitir parámetros.
- **Una clave del equipo para apagar `policy.config-protected`**: solo con un ADR. Hoy no existe.
- **`config-protected` en modo degradado**: se añade si el modo degradado llega a conocer el actor.
- **Marca de agua del suelo (H-01, `policy-floor`)**: se añade con US-GRD-014.
- **Caché de documentos personales**: se añade si la medición de § 7 supera el presupuesto.
- **`configStatus` real en la decisión** (Mejora 4): se añade con US-GRD-011.
- **Unificar los arneses e2e de Guardrails**: se añade en una tarea de deuda técnica.

## 10. ¿Un solo run o dos?

**Recomendación: un solo run, un PR**, con los slices en el orden de § 5. Motivos:

1. El requisito del orquestador (intento registrado) une las dos historias: la detección es de US-GRD-010 (D6) y el registro, de US-GRD-012 (D7).
2. Las dos tocan los mismos archivos (`evaluate.rs`, `guardrails/policies.rs`, `conn.rs`). En dos runs paralelos chocarían; en dos runs seguidos, el segundo rehace lo del primero.
3. El tamaño cabe en un run: unas 6 piezas nuevas pequeñas, sin dependencias nuevas, sin migraciones y sin código de error nuevo.

**Plan B**, si el presupuesto se agota tras el slice D:

- **PR 1**: US-GRD-012 completa (A, B solo `guard/config.rs`, D solo `config_guard::legacy_*` y la cadena de `policies_of`, y E).
- **PR 2**: US-GRD-010 y los avisos `relax-ignored`.

El corte está en la frontera de los slices, así que ningún archivo queda a medias.

**Estado al cerrar**: US-GRD-012 `implemented`. US-GRD-010 `partially-implemented` (pendientes: aplicar los permisos, US-GRD-007; filas 5 y 6, US-GRD-009).

## 11. Seguridad (para `/security-review`)

Tocan rutas de archivos, la salida al terminal y el canal; hay que pasarlo a `security-expert`.

1. **Lector del local**:
   - enlaces y FIFO;
   - TOCTOU en Windows y puntos de reparación;
   - un directorio padre enlazado (`repos/<id>`): `O_NOFOLLOW` solo protege el último componente, el mismo residuo que el perfil;
   - un `repo_id` inyectado: solo vale el del registro y se valida otra vez.
2. **Vías para saltarse `policy.config-protected`**:
   - variantes de mayúsculas, Unicode y NTFS;
   - `.gitraptor` convertido en enlace, archivo o submódulo;
   - `commit-tree` + `update-ref` o `push`;
   - `--no-verify`: cubierto por `reference-transaction`;
   - `core.hooksPath`: residuo `voluntary-skips`;
   - un agente no detectado o el modo degradado: residuo `policy-actor`;
   - un push solo a tags: TD-GRD-001.
3. **Disponibilidad**: más de 256 commits nuevos de un agente se deniegan aunque no haya reglas de rutas (D5). Además, un agente no puede subir un commit de la persona que toca `.gitraptor/`: es el mismo residuo que el "rebase con cambio de la persona" de US-GRD-008.
4. **Canal**: la bajada de versión por capacidad nunca convierte una denegación en un `allow` (D9). Los avisos `relax-ignored` no salen hacia el cliente del hook.
5. **Registro**: sin valores del documento. Un agente puede inundar el registro con avisos: lo contienen la agregación y el tope en vuelo.
6. **Pureza**: hay que probar que `ignored_relaxations` nunca cambia `effect` (unitario).
7. **Riesgo abierto R-GRD-4**: sin Q-GRD-32, un agente que escribe en el perfil puede borrar o corromper los endurecimientos **personales**. Nunca puede bajar del equipo. Hay que declararlo en el PR.

## 12. Documentación que se actualiza (slice F)

- `docs/requirements/features/guardrails/user-stories/US-GRD-012-agente-no-relaja-configuracion.md`:
  - `status: implemented` y `updated`;
  - "Requisitos Técnicos" y "Dev Spec": enlace a este brief;
  - "Estado de la implementación": el PR, macOS verificado, Linux por CI y Windows pendiente.
- `US-GRD-010-endurecer-sin-relajar.md`: `status: partially-implemented`, con lo mismo y los pendientes (US-GRD-007, US-GRD-009).
- `docs/requirements/features/guardrails/user-stories.md`: el índice, si lleva la columna de estado.
- `docs/requirements/backlog.md`: la línea "Implementación (2026-10-08)" de Guardrails.
- `docs/architecture/decisions/ADR-GRD-003-motor-decision-contrato.md`, con una **Enmienda**:
  - la regla `policy.config-protected` (§ 1 y § 2: no es el mínimo de BR-EDGE-001 y `disableSafeMinimum` no la apaga);
  - § 4 (`policy-actor` ampliado);
  - § 6 (una operación puede dejar su entrada y un aviso `config.relax-ignored` con el mismo `decision_id`).
- `ADR-GRD-006-registro-decisiones.md`: nota del aviso `config.relax-ignored`.
- `docs/architecture/xplat-pendientes.md`: el lector del local en Windows y la E2E de US-GRD-012.
- `docs/requirements/release-status.md`: se regenera con `node tools/status/release-status.mjs`, nunca a mano.

## 13. Preguntas abiertas y supuestos

- ⚠️ **ASSUMPTION**: "la configuración del repo" del requisito del orquestador es `.gitraptor/settings.json`, y no `.git/config`. Si era `.git/config` (por ejemplo, `core.hooksPath`), cae en `voluntary-skips` y queda fuera de este brief.
- ⚠️ **ASSUMPTION**: "relajar una regla" del requisito significa relajar **una regla del equipo**. Relajar un endurecimiento del propio perfil es válido por la fila 3 de US-GRD-010; impedirlo es Q-GRD-32 (§ 9).
- ⚠️ **ASSUMPTION**: el agente "codex registrado" del escenario 1 se prueba con el agente simulado `claude-code` del arnés, porque `AgentKind` solo distingue `claude-code` y `other`.
- **Decisiones (NUEVA) que tiene que validar Rene**: D2 (semántica "local ?? perfil" para permisos y autoría), D5 (regla de producto, nivel `minimum`, todo `/.gitraptor/`, no desactivable, fail-closed sobre 256 commits), D6 (qué cuenta como intento), D7 (segunda entrada `notice`, solo con actor agente y solo en `ref-transaction`/`push`), D8 (sin commitear: no se registra), D9 (capacidad y bajada a `forbidden-path`) y D11 (sin confirmación extra en el commit de la persona).

## Traspaso

`rust-expert`: implementa este brief slice a slice (§ 5) contra el contrato `docs/dev-briefs/layered-config.contract.json`. No reabras las decisiones D1 a D11; si una no se sostiene en el código, detente y avisa. Pasa el resultado a `security-expert` (§ 11) antes del PR.

## Validación de decisiones (2026-10-08)

Decisión del orquestador (2026-10-08), validada por Arquitecto y PO:

- **Arquitecto:** sin bloqueantes. D2, D3, D6, D8, D9, D11 OK. D5 OK con tres cambios: el patrón casa también la ruta exacta `.gitraptor` (archivo, enlace, submódulo) con prueba; probar los casos de más de 256 commits (rebase largo, primer push, clon superficial, sin ramas de seguimiento) y decir en la Enmienda que «unverifiable solo con regla aplicable» ya no vale para agentes; `raptor guard status` lista `policy.config-protected`. D7: la enmienda de ADR-GRD-003 § 6 es aceptable, redactada como aclaración («una entrada de decisión por operación; los avisos de configuración con el mismo `decision_id` son aparte, no cuentan en el KPI ni aplican al ejecutor»); comprobar que `agg_key` incluye `kind`. D2: añadir prueba de propiedades de monotonía (`efectivo ≥ equipo`); R-GRD-4 se declara en el PR y el texto del aviso dice «relajación ignorada mientras actuaba el agente X», sin afirmar autoría. D9: verificar que quitar entradas en `legacy_log` no rompe paginación, cursor ni total.
- **PO:** alcance OK. Cambios: (1) el test «un endurecimiento local no afecta a otro clon» se verifica sobre el valor efectivo (`Layers::permissions()`), no sobre un `push → Deny` mientras US-GRD-007 siga en draft; (2) la ficha de US-GRD-012 documenta que «confirmación consciente» se lee como el commit propio de la persona (D11) y que perfil y local no están protegidos contra escritura del agente (R-GRD-4); (3) las fichas anotan la dependencia de US-GRD-007, el cambio de comportamiento de D5 (más de 256 commits → deny para agentes) y Windows/Linux pendientes.
- **Línea base:** los escenarios 2, 5 y 6 de US-GRD-012 ya pasan con el código de `main`; se escriben como pruebas de regresión fuera del contrato (B3).

## Ajuste del coordinador a D11 (aprobación del plan, 2026-10-08)

Relajar es una acción reservada al humano que exige una confirmación que un agente no pueda dar desde su canal (BR-AUTH-001, R-GRD-3). Con el modelo de autoría vigente el agente commitea con la identidad de la persona, así que «commit propio de la persona» **no** se lee del autor, del committer ni de la ausencia del trailer: se decide por el **actor** que ve el hook en ese commit (ascendencia del proceso). Actor agente → la relajación se ignora y se registra el aviso; actor «sin atribuir» → cuenta como la persona (riesgo residual aceptado del MVP, Q35/R-GRD-3; se declara en el PR y en la enmienda). Un commit sin hooks (`--no-verify`, `commit-tree`) no confirma nada: fail-closed. Contrato: criterio `GRD012-IDENTITY`.
