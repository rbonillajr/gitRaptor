---
id: DS-US-GRD-011
title: "Dev Spec — US-GRD-011: una configuración rota no deja pasar las operaciones peligrosas"
type: dev-spec
status: draft
created: 2026-10-09
updated: 2026-10-09
story: US-GRD-011
feature: guardrails
domain: GRP
scope: backend
frontend_surface: false
stack: rust
profile: backend-service
tooling: [cargo]
related:
  context: ../context.md
  story: ../user-stories/US-GRD-011-configuracion-ilegible.md
  stories: [US-GRD-011, US-GRD-007, US-GRD-010, US-GRD-014, US-GRD-008, US-GRD-004]
  adrs: [ADR-GRD-003, ADR-GRD-004, ADR-GRD-005, ADR-GRP-007, ADR-GRP-016]
  rules: [BR-EDGE-004, BR-EDGE-001, BR-VAL-001, BR-CONS-001, BR-CONS-004, BR-WF-002]
  api_spec: null
  design_spec: null
  contracts: []
must_read:
  - ../user-stories/US-GRD-011-configuracion-ilegible.md
  - ../business-rules.md
  - ./US-GRD-007-permisos-por-operacion.md
  - ./US-GRD-008-ramas-protegidas-rutas-prohibidas.md
  - ../../../../architecture/decisions/ADR-GRD-003-motor-decision-contrato.md
  - ../../../../architecture/decisions/ADR-GRD-004-configuracion-efectiva.md
  - ../../../../architecture/decisions/ADR-GRD-005-estado-proteccion.md
  - ../../../../architecture/extender-sin-archivos-compartidos.md
  - ../../../../dev-briefs/layered-config.md
  - ../../../../../crates/policy/src/team.rs
  - ../../../../../crates/policy/src/layers.rs
  - ../../../../../crates/policy/src/settings/document.rs
  - ../../../../../crates/policy/tests/team_config.rs
  - ../../../../../crates/core/src/guardrails/layers.rs
  - ../../../../../crates/core/src/guardrails/evaluate.rs
  - ../../../../../crates/core/src/guardrails/hook.rs
  - ../../../../../crates/core/src/guardrails/install.rs
  - ../../../../../crates/api/src/guard.rs
  - ../../../../../apps/cli/src/guard.rs
lineage:
  supersedes: []
  superseded_by: []
  migration_adr: null
  migration_guide: null
constitution_gates: []
validation:
  must_read_resolved: true
  gaps_blocking: 0
  ready_to_implement: true
  gaps_release: 1
  ready_to_release: false
tags: [guardrails, fail-safe, config-status, config-ref, configuracion-ilegible, parcial, br-edge-004, minimo-seguro]
---

# Dev Spec — US-GRD-011: una configuración rota no deja pasar las operaciones peligrosas

Plano compacto (AADD ligero) de [US-GRD-011](../user-stories/US-GRD-011-configuracion-ilegible.md). La regla es [BR-EDGE-004](../business-rules.md) (Q-GRD-12 y Q-GRD-26): ante un nivel ilegible o parcial, GitRaptor **avisa**, aplica el conjunto mínimo más lo legible y nunca cae en "todo permitido". Un nivel personal ilegible solo pierde sus endurecimientos. El cargador ya distingue `Absent`, `Readable`, `Partial` e `Ignored` ([TS-GRD-001](./TS-GRD-001-configuracion-commiteada.md)), y un conflicto sin commitear ya no cuenta. Esta historia hace **visible** ese estado (`configStatus` y `configRef` en cada decisión, aviso en el hook y en `raptor guard status`) y cierra los huecos en los que un nivel roto **no** fuerza hoy el mínimo. Cierra la Mejora 4 del [brief de US-GRD-010/012](../../../../dev-briefs/layered-config.md).

**Invariante**: un estado roto solo puede endurecer. Ninguna rama de este código relaja nada ni lee el working tree.

**Restricciones de arquitectura**: no hay `architecture-constitution.md` en la cascada. Rigen `AGENTS.md` y los ADRs citados, como en las Dev Specs hermanas.

## 1. Decisiones

Cada fila es una **Decisión del orquestador (2026-10-09), validada por el Arquitecto**. Las marcadas **(PO)** cambian lo que se promete al usuario: las validó también el PO y llevan su Q-GRD (§ 9).

| # | Decisión |
|---|---|
| D1 | **`configStatus[]` real** (ADR-GRD-003 § 3). Cuando la evaluación carga las capas (con `guard.permissions`, DS-US-GRD-007 D11), la decisión lleva siempre cuatro entradas en este orden: `floor`, `worktree`, `profile` y `local`. Correspondencia: `Absent` → `absent`, `Readable` → `ok`, `Partial` → `partial`, `Ignored` → `unreadable`. El suelo es `pending-confirmation` cuando el nivel de equipo trae `floor-relax-pending` (con el criterio que deje US-GRD-014, que también cubre la configuración sin confirmación inicial que relaja) o `confirmed-floor-missing`. Precedencia: `unreadable`/`partial` > `pending-confirmation` > `ok`/`absent`. Sin capas cargadas, sigue como hoy, `[{floor, not-read}]`. No hace falta capacidad: la forma y los valores ya existen en el contrato |
| D2 | **`configRef`**: los blobs del equipo que se leyeron, como `<fuente>:<oid>` en orden fijo (`floor:…`, `confirmed-floor:…`, `worktree:…`). Si una fuente no tiene blob, no aparece. Los niveles personales no son blobs y no aparecen; su estado está en D1. Hoy la lista va siempre vacía y ningún cliente la lee, así que fijar el formato no rompe nada |
| D3 | **(PO, Q-GRD-43) Fuentes que fuerzan el mínimo** (BR-EDGE-004; Q-GRD-26, precisada por Q-GRD-43; ADR-GRD-004 D12). El mínimo está activo, aunque el suelo confirmado lo desactive, si el suelo, el suelo confirmado en vigor o **el worktree de la operación** está en `Partial` o `Ignored`. **Un nivel personal nunca lo fuerza**: si está en `Partial`, aplica lo legible, pierde lo que no se pudo leer y avisa; si está en `Ignored`, pierde sus endurecimientos y avisa. Un nivel personal no puede relajar (Q-GRD-14), así que una errata suya no puede abrir nada, y forzar el mínimo por un archivo personal lo trataría peor estando parcial que ilegible. Q-GRD-43 queda pendiente de que Rene la confirme en el PR. Hoy `team::combine` solo mira si el suelo es legible: un worktree con marcas de conflicto commiteadas, o con una errata, **no** fuerza el mínimo. Es el cambio central de esta historia, y vive solo en `TeamLoader::load`: `Layers::permissions()` y `layers::harden` no cambian |
| D4 | **Con una relajación pendiente, el mínimo sigue salvo que lo desactiven los dos suelos.** Con `floor-relax-pending`, el suelo efectivo es el confirmado y el nuevo solo endurece. Si el nuevo suelo **quita** `disableSafeMinimum`, eso endurece y se aplica al momento (Q-GRD-21). Hoy no ocurre, porque los endurecedores no leen esa clave. Regla: `safe_minimum_active = !(confirmado legible y lo desactiva) \|\| (suelo endurecedor presente y no lo desactiva) \|\| D3` |
| D5 | **(PO, Q-GRD-44) Un suelo roto no es una relajación pendiente.** Con el suelo `Partial` o `Ignored` no se emite `floor-relax-pending`: US-GRD-014 no confirma un suelo así (`floor-unreadable`, su D4), así que ofrecer la acción sería falso. Si hay un suelo confirmado legible, sigue rigiendo como hoy, con el mínimo forzado (D3), y lo legible de un suelo `Partial` entra como endurecedor. Sin confirmación, el suelo solo endurece (hoy). El estado lo muestra como configuración del equipo ilegible o parcial (D8), no como relajación pendiente. Cambia el test `a_forged_floor_without_settings_or_with_broken_settings_is_a_relaxation`, que se parte en dos (§ 6). Un suelo **sin** configuración frente a uno confirmado sigue siendo una relajación pendiente |
| D6 | **Sin commitear no cuenta y corregir restablece sin reinstalar.** Ya funciona así: el equipo se lee de objetos commiteados, y la caché del cargador va por blob, así que un `HEAD` nuevo es un blob nuevo. No se construye nada: lo prueban los escenarios 2 y 6, y siguen los tests `uncommitted_edit_or_conflict_does_not_change_the_team_level` y `a_real_merge_conflict_in_the_worktree_is_not_read`. El escenario 6 se observa con un endurecimiento del worktree (`push` en `deny`): roto, no aplica (con aviso); corregido y commiteado, vuelve. Así no depende de una confirmación |
| D7 | **(PO, Q-GRD-45) Dónde avisa GitRaptor: en el hook.** El cliente escribe por stderr una línea de aviso por cada fuente `unreadable` o `partial` de `configStatus`, con plantilla fija, `{repo}` (nombre de la carpeta del repo) y `{worktree}` (nombre de la carpeta del worktree), saneados y sin contenido del archivo (SEC-11). Lo ve quien opere, agente o persona. No cambia el código de salida. Para no repetirlo en cada proceso de hook de un mismo comando, avisan `pre-commit`, `pre-push` y `pre-rebase` siempre, y `reference-transaction` solo junto a una denegación. Así, un borrado de rama permitido no avisa: lo hace el estado (D8). En degradado avisa igual, sin niveles personales |
| D8 | **Dónde avisa GitRaptor: en `raptor guard status`.** `GuardStatus` gana `config_issues: Vec<ConfigIssue>`, con `{ source, status, worktree? }` y solo las fuentes `unreadable` o `partial`. El suelo, el perfil y el local salen una vez por repo; el worktree, por cada worktree del repo (la lista que ya usa `guard.plan`, hasta 64), leído con el mismo cargador y la misma caché. **Capacidad `guard.config-status`** (ADR-GRP-016): `GuardStatus` es `deny_unknown_fields`. Sin la capacidad, el campo no viaja. No es un estado nuevo, y `minimum_set` sigue en `active` cuando el mínimo está forzado: la lista explica por qué |
| D9 | **(PO, Q-GRD-45) Fuera del registro y del KPI.** Un aviso de configuración ilegible no es una decisión: no entra en el registro ni cuenta en el KPI. Una denegación del mínimo forzado es una denegación verificada como cualquier otra y cuenta (BR-CONS-004) |
| D10 | **Fallo total del cargador** (la rama principal no se resuelve, error de E/S), coherente con D13 de DS-US-GRD-008. `layers::load` sigue devolviendo `Err`. En ese brazo, `serve_audited` lee **solo** los niveles personales (`layers::personal`, nuevo) y evalúa con: mínimo forzado; permisos = los personales sobre los valores por defecto (`harden` sobre `EffectivePermissions` de solo mínimo); políticas = `Policies::unreadable()` (el movimiento de un agente se deniega con `unverifiable`; la persona pasa, como hoy); bases = la unión de reserva (DS-US-GRD-014 D6, o `GuardEntry.bases` mientras no esté); `configStatus` con `floor` y `worktree` en `unreadable`. En degradado, el mismo fallo da el mínimo solo, sin niveles personales |
| D11 | **(PO, aplicación de Q-GRD-35) Escenario 3 con un agente.** `forbiddenPaths` aplica por defecto a agentes (`appliesTo: agents`). El test usa `raptor-fake-agent`, como dice ya el Gherkin corregido por el PO, y una segunda variante con `appliesTo: everyone` y la persona |
| D12 | **Orden: después de DS-US-GRD-007 PR-A.** Sin permisos aplicados ni `disableSafeMinimum` real (DS-US-GRD-007 D4), los escenarios 1, 4 y 5 pasarían por casualidad: el mínimo nunca se desactiva y nada lee el perfil para un push. Además, D1 necesita la carga única para toda operación (DS-US-GRD-007 D11). No se hace en paralelo con PR-A: las dos tocan `evaluate.rs` (`decision`, `serve_audited`). Si las toma el mismo implementador, pueden ir en una rama con dos commits. Los escenarios 1 y 5 piden un suelo confirmado que desactiva el mínimo, es decir, `raptor guard confirm` (US-GRD-014). Si US-GRD-014 no está en `main`, se verifican en `crates/core` con un `Confirmed` construido, y su versión de extremo a extremo se suma a PR-B de DS-US-GRD-007 (T010) |
| D13 | **Sin cambios en motor-local.** El motor sigue ignorando una fuente ilegible para sus valores (BR-CONS-007 de motor-local; rama base y umbral), que no pueden relajar nada de Guardrails. La dependencia abierta de BR-EDGE-004 queda como está |

## 2. Forma (archivos que se tocan)

| Pieza | Archivo |
|---|---|
| Fuentes que fuerzan el mínimo; suelo roto sin `floor-relax-pending`; regla de D4 | `crates/policy/src/team.rs` (`combine`, `TeamLoader::load`) |
| Lectura solo personal (fallo total), estado y blobs de cada fuente | `crates/core/src/guardrails/layers.rs` (`permissions`, `personal`, `config_status`, `config_ref`) |
| `configStatus` y `configRef` en la decisión; brazo de fallo total | `crates/core/src/guardrails/evaluate.rs` (`decision` recibe el estado; `serve_audited`) |
| Aviso en el hook | `apps/cli/src/guard.rs` (líneas de aviso desde `decision.config_status`); `crates/core/src/guardrails/hook.rs` (el `configStatus` del degradado) |
| `config_issues` en el estado | `crates/api/src/guard.rs` (`ConfigIssue`, campo); `crates/api/src/methods/guard.rs` (capacidad); `crates/core/src/guardrails/install.rs` (`status`); `crates/core/src/channel/conn.rs` (quita el campo sin la capacidad) |
| Mensajes | `apps/cli/i18n/{en,es}/guard.txt` |
| Tests | § 6 |
| Documentación | Este archivo; la historia; enmiendas de ADR-GRD-003/004/005 (§ 9); `backlog.md`; `release-status.md` |

## 3. Matriz de fuentes

| Fuente rota | Qué aplica | ¿Fuerza el mínimo? | `configStatus` | Aviso |
|---|---|---|---|---|
| Suelo `Ignored` (JSON inválido, marcas de conflicto commiteadas en la rama principal) | El confirmado, si es legible; si no, nada del suelo. Más worktree y personales | Sí | `floor: unreadable` | Hook y estado |
| Suelo `Partial` (errata en `permissions` o `policies`) | El confirmado, si es legible, más lo legible del nuevo como endurecedor; sin confirmado, lo legible solo endurece | Sí | `floor: partial` | Hook y estado |
| Worktree `Ignored` (marcas de conflicto commiteadas en `feat-x`) | Suelo y personales; el worktree no aporta | Sí (**cambio**) | `worktree: unreadable` | Hook y estado (por worktree) |
| Worktree `Partial` | Lo legible del worktree, que endurece | Sí (**cambio**) | `worktree: partial` | Hook y estado |
| Perfil o local `Ignored` | Todo lo demás; se pierden sus endurecimientos | No | `profile`/`local: unreadable` | Hook y estado |
| Perfil o local `Partial` | Lo legible, que endurece; se pierde lo que no se pudo leer | No (Q-GRD-43) | `profile`/`local: partial` | Hook y estado |
| Conflicto o edición sin commitear | La última versión commiteada | No | `ok` | Ninguno |
| Fallo total del cargador del equipo | Mínimo + personales; agentes sin mover ramas (`unverifiable`) | Sí | `floor`, `worktree: unreadable` | Hook |

## 4. Configuración

No hay claves nuevas. La documentación del esquema dice qué pasa con un archivo roto (§ 3), y que el arreglo se aplica al commitearlo, sin reinstalar.

## 5. Contrato (`crates/api`)

```rust
/// A configuration source that cannot be read or is read only in part (US-GRD-011).
pub struct ConfigIssue {            // deny_unknown_fields
    pub source: Level,              // floor | worktree | profile | local
    pub status: ConfigStatus,       // unreadable | partial
    pub worktree: Option<Untrusted>,// only with source = worktree: the worktree folder name
}
// GuardStatus: #[serde(default, skip_serializing_if = "Vec::is_empty")] pub config_issues: Vec<ConfigIssue>
```

- Capacidad `guard.config-status` (`CAP_GUARD_CONFIG_STATUS`) en `crates/api/src/methods/guard.rs`.
- `Decision.config_status` y `Decision.config_ref` no cambian de forma (D1, D2).

Mensajes (`guard.txt`, plantillas fijas; los parámetros van etiquetados y saneados; nunca llevan contenido del archivo ni dicen cómo desactivar nada):

| Clave | es |
|---|---|
| `guard.config.floor.unreadable` | GitRaptor: aviso: la configuración del equipo de {repo} en la rama principal no se puede leer. Se aplican el conjunto mínimo y lo que sí se puede leer. |
| `guard.config.floor.partial` | GitRaptor: aviso: la configuración del equipo de {repo} en la rama principal se aplica solo en parte (tiene una clave o una operación que esta versión no conoce). El conjunto mínimo vuelve a aplicarse. |
| `guard.config.worktree.unreadable` | GitRaptor: aviso: la configuración del equipo de {repo} no se puede leer en {worktree}. Se aplican el conjunto mínimo y lo que sí se puede leer. |
| `guard.config.worktree.partial` | GitRaptor: aviso: la configuración del equipo de {repo} se aplica solo en parte en {worktree}. El conjunto mínimo vuelve a aplicarse. |
| `guard.config.profile.unreadable` | GitRaptor: aviso: el perfil no se puede leer; sus endurecimientos no se aplican. |
| `guard.config.profile.partial` | GitRaptor: aviso: el perfil se aplica solo en parte. El conjunto mínimo vuelve a aplicarse. |
| `guard.config.local.unreadable` | GitRaptor: aviso: la configuración local de {repo} no se puede leer; sus endurecimientos no se aplican. |
| `guard.config.local.partial` | GitRaptor: aviso: la configuración local de {repo} se aplica solo en parte. El conjunto mínimo vuelve a aplicarse. |
| `guard.status.config.<fuente>.<estado>` | El mismo texto sin el prefijo "GitRaptor: aviso:"; el de worktree nombra el worktree |

El inglés va en `apps/cli/i18n/en/guard.txt` con las mismas claves.

## 6. Criterios de aceptación verificables

Repos, remotos, perfiles y daemons temporales (NFR-01). Git, `raptor` y `raptor-hook` reales. Suite nueva `apps/cli/tests/guard_us_grd_011.rs` (se niega a correr sin *debug assertions*). Cada test comprueba la decisión **y** el aviso, o su ausencia.

| Escenario / criterio | Test |
|---|---|
| 1 · Suelo confirmado que permite force-push (mínimo desactivado); en `feat-x` se commitea la configuración con marcas de conflicto: el force-push se deniega por el mínimo y el hook avisa "no se puede leer en feat-x" | `a_committed_conflict_in_the_worktree_forces_the_minimum_and_warns` (necesita US-GRD-014, D12) |
| 2 · Conflicto sin commitear en `feat-x` con push denegado commiteado: se deniega por la versión commiteada y no hay aviso | `an_uncommitted_conflict_does_not_count` |
| 3 · Equipo ilegible y `secrets/` prohibida en el local: el commit de un agente que toca `secrets/api.txt` no se ejecuta (y con `everyone`, el de la persona tampoco) | `what_is_readable_in_other_levels_still_applies` |
| 4 · Push permitido por el equipo y perfil ilegible que lo denegaba: el push se ejecuta y el hook avisa del perfil | `an_unreadable_profile_only_loses_its_hardenings` |
| 5 · Suelo confirmado que desactiva el mínimo, con un permiso mal escrito en un commit posterior: el force-push se deniega por el mínimo y el hook avisa de que se aplica solo en parte | `a_typo_in_the_team_config_never_relaxes` (necesita US-GRD-014, D12) |
| 6 · Worktree con `push` en `deny` roto: el push pasa con aviso; al commitear la corrección, se deniega sin reinstalar y el aviso desaparece | `committing_the_fixed_config_brings_its_rules_back` |
| `raptor guard status` lista cada fuente rota, el worktree por su nombre, y nada cuando todo es legible | `status_lists_each_broken_source_and_worktree` |
| Degradado: avisa de la configuración del equipo ilegible y fuerza el mínimo | `degraded_mode_warns_about_an_unreadable_team_config` |

Además (`crates/core/tests/us_grd_011.rs` y `crates/policy`):

| Comportamiento | Test |
|---|---|
| Cuatro entradas de `configStatus` por decisión, con la precedencia de D1 y `pending-confirmation` con `floor-relax-pending` | `us_grd_011::config_status_reports_every_source` |
| `configRef` con `floor:`, `confirmed-floor:` y `worktree:`, sin niveles personales | `us_grd_011::config_ref_names_the_team_blobs` |
| Escenarios 1 y 5 con un `Confirmed` construido (mientras US-GRD-014 no esté en `main`) | `us_grd_011::a_broken_worktree_forces_the_minimum_over_a_confirmed_floor`, `us_grd_011::a_typo_forces_the_minimum_over_a_confirmed_floor` |
| Un perfil `Partial` con el mínimo desactivado por un suelo confirmado: el mínimo **sigue desactivado**, se aplica lo legible del perfil y `configStatus` trae `profile: partial` (el aviso). Lo mismo con el local | `us_grd_011::a_partial_personal_level_keeps_a_confirmed_disabled_minimum_and_warns` |
| Fallo total del cargador: mínimo + personales, agente `unverifiable`, persona pasa, `configStatus` `unreadable` | `us_grd_011::a_total_loader_failure_is_the_minimum_plus_the_personal_levels` |
| Capacidad `guard.config-status` en las dos direcciones | `us_grd_011::config_status_capability_both_ways` |
| Worktree `Ignored` o `Partial` fuerza el mínimo con el suelo confirmado desactivándolo | `crates/policy` · `team_config::a_committed_broken_or_partial_worktree_forces_the_minimum` |
| Suelo roto: sin `floor-relax-pending`, rige el confirmado y el mínimo está forzado | `team_config::a_broken_floor_is_unreadable_not_a_pending_relaxation` |
| Suelo sin configuración frente a uno confirmado: sigue siendo relajación pendiente | `team_config::a_forged_floor_without_settings_is_a_relaxation` (lo que queda del test actual) |
| Con relajación pendiente, el mínimo sigue salvo que los dos suelos lo desactiven | `team_config::under_a_pending_relaxation_the_minimum_stays_unless_both_floors_disable_it` |

## 7. Orden de implementación

Después de DS-US-GRD-007 PR-A (D12). Dentro de la historia: T001 → T002 → (T003, T004) → T005 → T006 → T007.

## 8. Pendientes, límites y fuera de alcance

| Pendiente | Dueño |
|---|---|
| Escenarios 1 y 5 de extremo a extremo con `raptor guard confirm` | US-GRD-014 en `main`; si no está, DS-US-GRD-007 T010 (D12) |
| Trinquete personal: un agente que corrompe el perfil o el local quita sus endurecimientos (Q-GRD-32, R-GRD-4) | Historia del PO pendiente y factor de autenticación fuera de banda (fuera del MVP) |
| Ver `configStatus` en `raptor guard log` | Si el contrato del registro llega a admitirlo |
| Más de 64 worktrees en el estado: el resto no se revisa | Declarado (límite de coste) |
| Linux y Windows reales | **Pendiente: etapa de validación multiplataforma** |

**Residuo declarado**: corromper un nivel personal es la forma de quitarle sus endurecimientos, y un agente puede hacerlo (el mismo residuo que el brief de US-GRD-010/012, § 9). El aviso lo hace visible; el trinquete lo cerraría.

## 9. Validación de las decisiones

Las decisiones técnicas (D1, D2, D4, D6, D8, D10, D12 y D13) son **Decisión del orquestador (2026-10-09), validada por el Arquitecto** (`nassa-architect:architect`).

Las de producto son **Decisión del orquestador (2026-10-09), validada por Arquitecto y PO** (`nassa-aadd:product-owner`), registradas en `context.md` y en BR-EDGE-004 y BR-WF-002:

| Decisión | Q-GRD | Nota |
|---|---|---|
| D3 · Solo los niveles del equipo fuerzan el mínimo; un nivel personal parcial aplica lo legible y avisa | Q-GRD-43 (precisa Q-GRD-26) | Ajustada por el PO: se quitó la variante (b), "un nivel personal parcial fuerza el mínimo". Es decisión del PO, pendiente de que Rene la confirme en el PR |
| D5 · Un suelo roto no es una relajación pendiente | Q-GRD-44 | — |
| D7 y D9 · El aviso lo recibe quien opera y lo muestra el estado; no entra en el registro ni en el KPI | Q-GRD-45 | — |
| D11 · El escenario 3 se prueba con un agente | Aplicación de Q-GRD-35 | El PO corrigió el Gherkin ("un agente") |

**Enmiendas propuestas** (las aplica el orquestador; esta Dev Spec no edita ADRs):

| ADR | Enmienda |
|---|---|
| ADR-GRD-003 § 3 | `config-unreadable` no es una razón `system` (no produce el efecto: lo produce el mínimo). El estado viaja en `configStatus`, con `pending-confirmation` = `floor-relax-pending` o `confirmed-floor-missing`, y el cliente avisa. `configRef` = `<fuente>:<oid>`, solo blobs del equipo |
| ADR-GRD-003 § 4 | Degradado: un fallo total del cargador deja el mínimo solo; el aviso sale igual |
| ADR-GRD-004 § 1 y § 2 | El mínimo lo fuerzan también el worktree `Partial`/`Ignored`, nunca un nivel personal (Q-GRD-43); un suelo roto no es una relajación pendiente; con una relajación pendiente, el mínimo sigue salvo que los dos suelos lo desactiven |
| ADR-GRD-005 § 3 | `config_issues` en el estado de protección, con la capacidad `guard.config-status`; no es un estado nuevo ni un `Diagnostic` |

**Preguntas abiertas**: **para Rene, en el PR**: confirmar Q-GRD-43 (un nivel personal parcial no fuerza el mínimo).

## Anexo de forma (perfil `backend-service`)

### 6.1 Tipos compartidos

`ConfigIssue` y el campo `GuardStatus.config_issues` en `crates/api/src/guard.rs`; la capacidad en `crates/api/src/methods/guard.rs`. Sin cambios: `ConfigStatus`, `ConfigSource`, `Decision`. En `crates/policy`: `SourceStatus`, `Parsed` y `layers::harden` sin cambios.

### 6.2 Ciclos de vida (DI)

_No aplica — no hay contenedor de dependencias: el cargador es el `LOADER` estático de `crates/core/src/guardrails/layers.rs` y el estado se calcula por petición._

### 6.3 Firmas del stack

`team::combine(floor, floor_readable, hardeners, force_minimum: bool) -> EffectivePermissions`; `TeamLoader::load` calcula `force_minimum` (D3, D4, D5). `guardrails::layers::{personal(profile, repo_id) -> (Parsed, Parsed), Layers::config_status() -> Vec<ConfigSource>, Layers::config_ref() -> Vec<String>}`. `evaluate::decision(Evaluation, ConfigSeen) -> Decision`, con `ConfigSeen { status: Vec<ConfigSource>, refs: Vec<String> }` (`ConfigSeen::not_read()` reproduce lo de hoy).

### 7.1 Forma del error

No hay errores nuevos. Una denegación del mínimo forzado es la de siempre (`minimum.force-push`, `minimum.base-branch-delete`); el aviso es texto por stderr y no cambia el código de salida.

### 7.2 Forma de la configuración

Sin cambios (§ 4).

### 7.3 Valores numéricos

Hasta 64 worktrees revisados por `raptor guard status`; los límites del documento son los de ADR-GRD-004 § 1 (64 KiB, profundidad 16, 256 claves o elementos, 1 KiB por cadena).

### 8. Modelo de datos

_No aplica — no se crea almacén ni columna: el estado de cada fuente se calcula en cada evaluación y en cada `guard.status`._

### 9. Estrategia de pruebas

§ 6. E2e `apps/cli/tests/guard_us_grd_011.rs`; `crates/core/tests/us_grd_011.rs` para `configStatus`, `configRef`, el fallo total y la capacidad; `crates/policy/tests/team_config.rs` para las fuentes que fuerzan el mínimo y el suelo roto.

## Gaps y violaciones de la constitución

Ningún hueco bloquea la implementación. Sin constitución en la cascada: se aplican `AGENTS.md` y los ADRs citados.

| Id | Hueco | Severidad | Alcance |
|---|---|---|---|
| G1 | Los escenarios 1 y 5 de extremo a extremo necesitan `raptor guard confirm` (US-GRD-014), que aún no está en `main`; mientras tanto se cubren en `crates/core` | Bloquea liberación | T001, T007 |

## Plan de implementación

> Orden topológico (`Depende:`). Rutas relativas a la raíz del repo. T001 son los tests en rojo. Empieza después de DS-US-GRD-007 PR-A.

| # | Tarea | Depende | Aterriza en |
|---|---|---|---|
| T001 | Suite e2e en rojo | DS-US-GRD-007 PR-A | `apps/cli/tests` |
| T002 | Fuentes que fuerzan el mínimo y suelo roto | — | `crates/policy` |
| T003 | Calcular `configStatus`, `configRef` y el fallo total | T002 | `crates/core/src/guardrails` |
| T004 | Añadir `ConfigIssue` y la capacidad `guard.config-status` | — | `crates/api` |
| T005 | Llenar `config_issues` en el estado | T003, T004 | `crates/core` |
| T006 | Avisos del hook y del estado | T003, T005 | `apps/cli`, `crates/core/src/guardrails/hook.rs` |
| T007 | Verificar y documentar | T001, T006 | `docs` |

### T001 — Suite e2e en rojo

**Objetivo.** `apps/cli/tests/guard_us_grd_011.rs` con la primera tabla del § 6; los escenarios 1 y 5 usan `raptor guard confirm` solo si US-GRD-014 está en `main`.

**Ubicación.**
- `apps/cli/tests/guard_us_grd_011.rs` (**CREATE**)

**Reglas**
- Repos y perfiles temporales (NFR-01), sin esperas fijas; las marcas de conflicto se commitean de verdad (merge real con conflicto y `git commit -a`); cada test comprueba la decisión y el aviso.

- **Depende:** DS-US-GRD-007 PR-A
- **Refs:** US-GRD-011; D6, D7, D11, D12
- **Aceptación:** `cargo test -p gitraptor-cli --test guard_us_grd_011 an_uncommitted_conflict_does_not_count`

### T002 — Fuentes que fuerzan el mínimo y suelo roto

**Objetivo.** `combine` con `force_minimum`; `TeamLoader::load` lo calcula con el suelo, el confirmado en vigor y el worktree (D3, D4), y no emite `floor-relax-pending` con el suelo `Partial`/`Ignored` (D5). Ni `harden` ni un nivel personal tocan el mínimo (Q-GRD-43).

**Ubicación.**
- `crates/policy/src/team.rs` (**MODIFY**)
- `crates/policy/tests/team_config.rs` (**MODIFY**: parte `a_forged_floor_without_settings_or_with_broken_settings_is_a_relaxation` y añade los tests del § 6)

**Reglas**
- Puro, sin E/S nueva; si US-GRD-014 ya movió el criterio de `floor-relax-pending` a `team::confirm`, la guarda de D5 va antes de ese criterio, en el mismo sitio; ningún cambio relaja nada respecto a hoy (los tests existentes siguen verdes salvo el que se parte).

- **Depende:** —
- **Refs:** D3, D4, D5
- **Aceptación:** `cargo test -p gitraptor-policy --test team_config`

### T003 — Calcular `configStatus`, `configRef` y el fallo total

**Objetivo.** `Layers::config_status()` y `config_ref()`; `decision()` los recibe; el brazo `Err` de `serve_audited` evalúa con mínimo + personales y `Policies::unreadable()`.

**Ubicación.**
- `crates/core/src/guardrails/layers.rs`, `crates/core/src/guardrails/evaluate.rs` (**MODIFY**)
- `crates/core/tests/us_grd_011.rs` (**CREATE**)

**Reglas**
- Una sola carga por evaluación (la de DS-US-GRD-007 D11); sin capas cargadas, `ConfigSeen::not_read()`; nada lee el working tree.

- **Depende:** T002
- **Refs:** D1, D2, D10
- **Aceptación:** `cargo test -p gitraptor-core --test us_grd_011`

### T004 — Añadir `ConfigIssue` y la capacidad `guard.config-status`

**Objetivo.** Tipo, campo y capacidad del § 5.

**Ubicación.**
- `crates/api/src/guard.rs`, `crates/api/src/methods/guard.rs` (**MODIFY**)

**Reglas**
- La capacidad solo en `methods/guard.rs`; el campo con `skip_serializing_if = "Vec::is_empty"`; `crates/api/tests/architecture.rs` sigue verde.

- **Depende:** —
- **Refs:** D8
- **Aceptación:** `cargo test -p gitraptor-api`

### T005 — Llenar `config_issues` en el estado

**Objetivo.** `install::status` llena `config_issues` (suelo, perfil y local una vez; cada worktree, hasta 64); `conn.rs` quita el campo sin la capacidad.

**Ubicación.**
- `crates/core/src/guardrails/install.rs`, `crates/core/src/channel/conn.rs` (**MODIFY**)

**Reglas**
- Mismo cargador y caché; solo `unreadable` y `partial`; sin contenido del archivo.

- **Depende:** T003, T004
- **Refs:** D8
- **Aceptación:** `cargo test -p gitraptor-core --test us_grd_011 config_status_capability_both_ways`

### T006 — Avisos del hook y del estado

**Objetivo.** Líneas de aviso desde `decision.config_status` con la regla de D7; líneas de `config_issues` en `raptor guard status`; plantillas en/es del § 5; `configStatus` también en la decisión del degradado.

**Ubicación.**
- `apps/cli/src/guard.rs`, `apps/cli/i18n/en/guard.txt`, `apps/cli/i18n/es/guard.txt` (**MODIFY**)
- `crates/core/src/guardrails/hook.rs` (**MODIFY**)

**Reglas**
- Plantillas fijas, `{repo}` y `{worktree}` saneados; avisan `pre-commit`, `pre-push` y `pre-rebase`, y `reference-transaction` solo con una denegación; el código de salida no cambia.

- **Depende:** T003, T005
- **Refs:** D7, D8
- **Aceptación:** `cargo test -p gitraptor-cli --test guard_us_grd_011`

### T007 — Verificar y documentar

**Objetivo.** Suite en verde, estado de la historia, `backlog.md` y `release-status.md`; el brief de US-GRD-010/012 marca la Mejora 4 como cerrada.

**Ubicación.**
- `docs/requirements/features/guardrails/user-stories/US-GRD-011-configuracion-ilegible.md`, `docs/requirements/backlog.md`, `docs/requirements/release-status.md`, `docs/dev-briefs/layered-config.md` (**MODIFY**)

**Reglas**
- Si US-GRD-014 no está en `main`, los escenarios 1 y 5 quedan "verificados en `crates/core`, extremo a extremo pendiente" y la historia no pasa a `implemented`; `release-status.md` se regenera con `node tools/status/release-status.mjs`.

- **Depende:** T001, T006
- **Refs:** D12
- **Aceptación:** `cargo test --workspace`

## 10. Estado de la implementación

_Pendiente._
