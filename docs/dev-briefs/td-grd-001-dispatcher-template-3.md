---
title: "Brief de implementación — dispatcher plantilla 3: el pre-push evalúa toda ref empujada (TD-GRD-001)"
type: dev-brief
status: draft
created: 2026-10-09
updated: 2026-10-09
feature: guardrails
stories: [TD-GRD-001]
related:
  adrs: [ADR-GRD-001, ADR-GRD-002, ADR-GRD-003, ADR-GRP-016]
  specs: [DS-US-GRD-008, DS-US-GRD-018, DS-INF-GRD-001]
  nfrs: [NFR-01, NFR-12, NFR-GRD-04, NFR-GRD-05]
contract: docs/dev-briefs/td-grd-001-dispatcher-template-3.contract.json
---

# Brief — dispatcher plantilla 3 (TD-GRD-001)

Autor: `rust-architect`. Ejecuta: `rust-expert`. Rama: `feat/TD-GRD-001-dispatcher-template-3`.
Ficha: [TD-GRD-001](../requirements/features/guardrails/technical-stories/TD-GRD-001-dispatcher-plantilla-3-pre-push-toda-ref.md).

## 1. Contexto y objetivo

Hoy, `git push origin <commit>:refs/tags/x` sube un commit que toca una ruta de `forbiddenPaths` sin evaluarlo. Hay dos motivos. El dispatcher de `pre-push` sale por la vía rápida cuando todas las refs remotas son no gobernadas (`fastpath::skippable_push`). Y el cliente (`hook.rs:push`) descarta esas líneas. El objetivo es que la regla de rutas prohibidas se aplique a **toda** ref empujada (tags, notes y demás refs no gobernadas) y que el residuo salga de `policy-reach`. Hace falta una **plantilla 3** del dispatcher y una actualización en el sitio desde las plantillas 1 y 2 que **nunca deje el repo sin un dispatcher que funcione**, aunque se interrumpa en cualquier paso.

> La ficha habla del "dispatcher `sh`". Desde la Enmienda 2026-10-05 de ADR-GRD-001, el dispatcher es **nativo**: `apps/cli/src/bin/raptor-hook.rs`, que se copia en `<common>/gitraptor/hooks/<hook>` y lee sus constantes de `dispatch.conf`. Este brief diseña sobre el código real. El slice de docs corrige la ficha.

## 2. Qué existe y qué falta

| Pieza | Hoy | Falta |
|---|---|---|
| Vía rápida de `pre-push` | `raptor-hook.rs:main` → `fastpath::skippable_push`, sin mirar la plantilla | Que solo se aplique con `template` 1 o 2 en `dispatch.conf` |
| Plantillas aceptadas | `raptor-hook.rs:TEMPLATES = ["1","2"]`; `hook.rs:HookArgs::parse` acepta `1..=TEMPLATE_VERSION` | `"3"` y `TEMPLATE_VERSION = 3` |
| Cliente `pre-push` | `hook.rs:push` descarta las refs no gobernadas antes de conectar | Conservarlas y decidir qué se envía al conocer la capacidad (`push_scope`) |
| Regla de rutas prohibidas | `policy/guard/mod.rs:policies_of` retorna en refs no gobernadas; `core/guardrails/policies.rs:touched` no las lee | Aplicar **solo** `forbidden_paths` a las no gobernadas, y leerlas |
| Actualización en el sitio | `install.rs:upgrade` (PR #141): escribe los hashes nuevos en el diario **antes** de escribir los archivos, sin puntos de corte y sin republicar el `Watched` | Máquina de estados con diario "pendiente", puntos de corte, re-verificación antes del cambio de `dispatch.conf` y republicación |
| Salud | `health.rs:integrity` compara cada archivo con un único hash del diario | Aceptar el hash confirmado **o** el de la actualización pendiente, con una regla de coherencia |
| Temporales de `replace_files` | Un proceso matado entre escribir el temporal y el `rename` deja `<archivo>.gitraptor.tmp-<hex>`; nadie lo borra | `GuardWriter::remove_file_temporaries` |

### Convenciones observadas

- **El comportamiento se decide por el número de plantilla que llega del dispatcher**: `crates/core/src/guardrails/hook.rs:second_line` (`if args.template < 2 { return None; }`). El filtro nuevo sigue ese patrón.
- **El cliente consulta la capacidad del daemon después del `hello`**: `hook.rs:ask_daemon` (`CAP_GUARD_POLICIES`) y `hook.rs:commit` (`CAP_GUARD_AUTHORSHIP`). La capacidad la declara `crates/api/src/methods/guard.rs:CAP_GUARD_POLICIES` y el daemon la usa en `crates/core/src/channel/conn.rs:guard_caller`.
- **La vía rápida solo permite, nunca deniega**, y lo malformado va al parser estricto: es el doc de módulo de `crates/policy/src/guard/fastpath.rs`. El mismo archivo lo compila el dispatcher con `#[path]` (`raptor-hook.rs`, `mod fastpath`).
- **El dispatcher valida la plantilla de sus constantes**: `raptor-hook.rs:Conf::read` (`TEMPLATES.contains(...)`). Sin constantes válidas aplica `fallback`, que deniega `pre-push` y **no encadena el hook previo** (`Msg::PriorUnknown`). Esto convierte en estado roto la combinación "binario viejo + `dispatch.conf` de plantilla 3".
- **Los puntos de corte son `<paso>:<before|after>`**, detrás de la feature `chaos`: `crates/core/src/guardrails/cut.rs:trip`, con nombres como `repair-files` y `repair-key` en `install.rs:repair`. El barrido sigue el patrón de `apps/cli/tests/guard_us_grd_003.rs:repo_intact_an_uninstall_cut_at_any_point_is_complete_or_undone`, con `gitraptor_testkit::cut::{points, ENV_CUT, ENV_TRACE}`.
- **Reemplazo atómico archivo a archivo** (temporal, `fsync`, `rename`) dentro de la carpeta con la identidad del diario: `crates/git/src/guard_write/mod.rs:GuardWriter::replace_files` y sus versiones `unix.rs:replace_files` y `portable.rs:replace_files`.
- **El diario se guarda antes y después de cada paso**, y es la referencia de integridad (H-04): `install.rs:install` (`save`) y `journal.rs:Journal`. El manifiesto no cuenta: `health.rs:integrity` lo excluye.
- **Lecturas seguras de archivos ajenos**: `health.rs:read_regular` (sin seguir enlaces, sin bloquear en un FIFO y con tope).
- **La configuración solo endurece** (PR #217): `crates/core/src/guardrails/evaluate.rs:serve_audited` llama a `super::layers::load(...)` y, si falla, usa `Policies::unreadable()`. Este brief no toca cómo se resuelven las políticas, solo dónde se aplica `forbidden_paths`.
- **Las reglas de política son una pasada aparte del mínimo**: `crates/policy/src/guard/mod.rs:evaluate` → `policies_of`.
- **La rama protegida ya solo mira `refs/heads/*`**: `crates/policy/src/guard/policies.rs:protected_branch` (`strip_prefix("refs/heads/")`).
- **Las lecturas tienen topes y lo no verificable deniega**: `crates/core/src/guardrails/policies.rs:MAX_COMMITS` y `MAX_READS`, y `Touched { unverifiable: true }`.
- **El rendimiento se mide con tests `#[ignore]` en release, nunca con un assert de tiempo en debug**: `crates/core/tests/guard_evaluate.rs:policies_cost` y `apps/cli/tests/guard_us_grd_001.rs:latency_report`.
- **Los e2e usan binarios reales y un agente simulado**: `apps/cli/tests/guard_us_grd_008.rs:Machine` y `fake_agent_entry` (`raptor-fake-agent`). Exigen debug assertions, porque `GITRAPTOR_PROFILE_DIR` solo existe en debug (`Machine::new` hace `panic!` en release).
- **Hay un catálogo i18n por módulo**: `apps/cli/i18n/{en,es}/guard.txt`, con claves `guard.np.policy-reach` y `guard.status.template-outdated` (ADR-GRP-016).
- **Los comentarios citan ADR y §** como el código que los rodea. No se cita `TD-GRD-001` en el código (rust-standards).

### Mejoras detectadas

| # | Hallazgo | Dónde | Tratamiento |
|---|---|---|---|
| M1 | **Defecto latente de la subida 1→2**: `upgrade()` no republica el `Watched` del monitor. Después de actualizar, `daemon/guard.rs:guard_health` compara con los hashes **viejos** de `watched.journal` y da una alerta falsa `dispatcher-altered` | `install.rs:upgrade`, `daemon/guard.rs:guard_health` | **En alcance** (D7): sin arreglarlo, todos los repos que suban a la 3 recibirían una alerta falsa |
| M2 | **Defecto latente**: `upgrade()` guarda los hashes nuevos **antes** de escribir. Una subida interrumpida se lee como `DispatcherAltered` y la protección figura **inactiva**, así que la siguiente `raptor guard install` pasa por la pantalla de reparación | `install.rs:upgrade` | **En alcance** (D4, D5) |
| M3 | El e2e `downgrade_to_template_1` edita `dispatch.conf` sin tocar el diario. Por eso `a_template_1_install_is_upgraded_by_reinstalling` prueba la **reparación**, no la subida. Además deja de funcionar con `TEMPLATE_VERSION = 3`, porque busca `"template\t2\n"` | `apps/cli/tests/guard_us_grd_018.rs` | **En alcance** (slice A): se ajusta a la plantilla actual; la subida real la cubren las pruebas nuevas |
| M4 | Un `replace_files` matado entre escribir el temporal y hacer el `rename` deja `<archivo>.gitraptor.tmp-<16 hex>` en `hooks/`. Ni la subida ni la desinstalación lo borran, así que la carpeta queda tras desinstalar (huella, NFR-01) | `guard_write/unix.rs:replace_files`, `uninstall.rs:remove_rest` | **En alcance** (D6, slice C) |
| M5 | ADR-GRD-001 § 8 dice "la plantilla actual y la anterior", pero `HookArgs::parse` acepta `1..=TEMPLATE_VERSION` | `hook.rs:HookArgs::parse` | Docs: la enmienda alinea el ADR con el código, que acepta 1, 2 y 3 |
| M6 | `MAX_READS = 256` cuenta también las lecturas cuyo commit ya está en el remoto. Un `git push --tags` de más de 256 refs, bajo una regla de rutas que gobierne al actor, se deniega por `unverifiable` | `core/guardrails/policies.rs:touched` | Coste declarado (O-3, § 7) con el criterio TD001-BULK; el mensaje sugiere empujar por tandas. Diferido: deduplicar lecturas por `(viejo, nuevo)` |
| M7 | El `Machine` de los e2e está copiado en `guard_us_grd_008.rs`, `guard_us_grd_018.rs` y `guard_machine/mod.rs` | `apps/cli/tests/*` | No se toca. Esta historia trae su propio módulo de ayuda; un `Machine` común con agente queda diferido |

## 3. Decisiones de arquitectura

| # | Decisión | Por qué |
|---|---|---|
| D1 | **La plantilla 3 es un comportamiento del binario según la plantilla de `dispatch.conf`**: el mismo `raptor-hook` aplica la vía rápida de `pre-push` con `template` 1 o 2, y no la aplica con 3 | El binario nuevo, copiado durante la subida mientras `dispatch.conf` aún dice 2, se comporta exactamente como la plantilla 2. El cambio de comportamiento ocurre en **un solo `rename` atómico**, el de `dispatch.conf` |
| D2 | `TEMPLATE_VERSION = 3`. El cliente acepta 1, 2 y 3 (ya acepta `1..=TEMPLATE_VERSION`). `TEMPLATES = ["1","2","3"]` en el dispatcher | Una instalación sin actualizar sigue funcionando (ficha) |
| D3 | **Cliente**: `push` conserva todas las líneas, analizadas de forma estricta como hoy. La función pura `push_scope(op, template, scope_all)` acota las refs. Hay tres destinos:<br>(1) **al daemon** se envía `push_scope(op, template, granted)`: todas las refs solo si `template >= 3` **y** el daemon concede `guard.policies`;<br>(2) **las reglas `everyone` del suelo que evalúa el propio cliente** cuando el daemon no concede la capacidad reciben `push_scope(op, template, true)`: con la plantilla 3 o mayor, también los tags, las notes y las demás refs;<br>(3) en **modo degradado** y ante las señales de ataque, las refs no gobernadas siguen el § 4.5.1 (fail-closed **solo con una regla aplicable**, condición del coordinador);<br>(4) con `template < 3` se filtra **antes** de conectar, como hoy | El daemon sin la capacidad no recibe nada nuevo (ficha). El modo degradado y el daemon sin la capacidad nunca son menos estrictos entre sí ni con el daemon (ADR-GRD-003 § 4). La función pura prueba la puerta sin un daemon falso, que el cliente rechazaría por `same_executable` (O-1, decisión del § 12) |
| D4 | **El diario lleva la actualización como pendiente**: el campo nuevo `Journal.upgrade: Option<Upgrade>` guarda la plantilla destino y sus hashes. `files` y `template` siguen siendo los **confirmados** hasta el último paso. `listed()` devuelve la unión | Una subida interrumpida se lee como "activa, plantilla antigua" y no como "alterada". La desinstalación sigue borrando todo lo que pudo escribirse |
| D5 | **La salud acepta el hash confirmado o uno pendiente**. Hay una regla de coherencia: si `dispatch.conf` solo coincide con un hash pendiente, **todo** `hooks/*` de la actualización tiene que coincidir con un hash pendiente; si no, `DispatcherAltered`. Mientras hay una actualización pendiente se informa `TemplateOutdated` | Esto hace comprobable el invariante del que depende todo el fail-safe: nunca hay `dispatch.conf` de plantilla 3 con un binario de plantilla ≤ 2 |
| D6 | **Máquina de estados de `upgrade()`** (§ 4.6) con 6 pasos de corte: primero los ejecutables, después la **re-verificación por hash** de cada ejecutable escrito, luego `dispatch.conf` (el cambio), luego el manifiesto y al final el cierre del diario. Antes de escribir se borran los temporales huérfanos | Cada estado intermedio tiene dispatchers que funcionan (§ 4.6). La siguiente `raptor guard install` lo completa por la ruta de **subida**, sin pantalla de reparación |
| D7 | `upgrade()` republica el `Watched` (`registry.protection().watch`) al guardar el pendiente y al cerrar | Corrige M1. El monitor y la salud usan el mismo diario |
| D8 | **Regla por tipo de ref** (§ 4.4): rutas prohibidas en toda ref empujada; rama protegida solo en `refs/heads/*` (ya es así); mínimo y configuración protegida, sin cambios (solo refs gobernadas) | Es el alcance de la ficha. La configuración protegida en un tag no cambia las reglas en vigor (§ 9) |
| D9 | `evaluate::facts` no calcula ascendencia para refs no gobernadas | `push_update` no las mira. Cada lectura evitada es latencia (NFR-GRD-04) |
| D10 | **No se añade ninguna dependencia**. `install::sha256` pasa de `pub(crate)` a `pub` para que los e2e reescriban un diario coherente al simular una instalación de plantilla 1 o 2 | Escalera de simplicidad: ya existe |
| D11 | El arranque del daemon **no** completa por su cuenta una subida interrumpida: la completa la siguiente `raptor guard install` | Lo pide la ficha. Escribir en el repo es un comando reservado (ADR-GRD-007 § 1) |

## 4. Contratos

### 4.1 Dispatcher — `apps/cli/src/bin/raptor-hook.rs` (slice A)

Diff de la plantilla 3 (todo lo demás del archivo queda igual):

```diff
-/// Template versions this dispatcher understands (ADR-GRD-001 § 8).
-const TEMPLATES: &[&str] = &["1", "2"];
+/// Template versions this dispatcher understands (ADR-GRD-001 § 8).
+const TEMPLATES: &[&str] = &["1", "2", "3"];
+
+/// Templates 1 and 2 let a push of refs Guardrails does not govern (tags, notes…) go without
+/// `raptor`. From 3 on every pushed ref is handed over, so the forbidden paths reach them: the
+/// behaviour follows the template of the constants, never the build, so a dispatcher copied
+/// before its `dispatch.conf` during an upgrade behaves as the template still in force.
+fn push_fast_path(conf: &Conf) -> bool {
+    matches!(conf.get("template"), Some("1" | "2"))
+}
 ...
     let skippable = match hook {
         Hook::ReferenceTransaction => {
             fastpath::skippable_ref_transaction(&input, &common, MAX_LINE)
         }
-        Hook::PrePush => fastpath::skippable_push(&input, MAX_LINE),
+        Hook::PrePush => push_fast_path(&conf) && fastpath::skippable_push(&input, MAX_LINE),
         Hook::PreRebase | Hook::PreCommit | Hook::CommitMsg | Hook::Chain(_) => false,
     };
```

- Actualiza el doc de módulo ("Fast path: …") para decir que en `pre-push` solo se aplica con las plantillas 1 y 2.
- `crates/policy/src/guard/fastpath.rs:skippable_push` **no cambia** (slice B solo ajusta su doc: "templates 1 and 2").
- **Sin decisión de `raptor`, con la condición del coordinador (§ 4.5.1, caso A).** `fallback` recibe un indicador nuevo, `ungoverned_only`: `hook == PrePush && !push_fast_path(&conf) && fastpath::skippable_push(&input, MAX_LINE)`. Si es `true` (plantilla ≥ 3 y todas las refs remotas no gobernadas):
  - falta `raptor`: **pasa con el aviso `Msg::Inactive`** y encadena el hook previo;
  - `spawn` falla o el código de salida es inesperado: **pasa con el aviso `Msg::InternalPassed`** y encadena el hook previo.

  Con cualquier ref gobernada, `pre-push` falla cerrado como hoy. Sin `dispatch.conf` válido (`conf` es `None`) y con la carpeta movida (`Msg::Moved`), todo sigue igual que hoy, también con la plantilla 2: esas comprobaciones van antes de la vía rápida.

### 4.2 Constantes y diario — `crates/core/src/guardrails/{constants,journal}.rs` (slice D)

```rust
// constants.rs
/// Version of the dispatcher template: `raptor hook` accepts it and every earlier one.
/// 2 adds the `pre-commit` and `commit-msg` dispatchers; 3 hands every pushed ref of a
/// `pre-push` to `raptor hook` (no fast path for refs that are not governed).
pub const TEMPLATE_VERSION: u32 = 3;

/// The first template whose `pre-push` dispatcher hands over every pushed ref.
pub const EVERY_PUSHED_REF: u32 = 3;
```

```rust
// journal.rs
/// An upgrade of the dispatchers that started and was not confirmed (ADR-GRD-001 § 8): the
/// files of the folder may hold the confirmed hash or one of these until it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upgrade {
    /// The template being installed.
    pub template: u32,
    /// The files of that template and their hashes. A path appears once per build that started
    /// the upgrade (an interrupted upgrade resumed by another `raptor` keeps the earlier hashes).
    pub files: Vec<FileHash>,
}

pub struct Journal {
    // ... existing fields unchanged ...
    /// An upgrade in progress; `None` once it is confirmed (or never started).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade: Option<Upgrade>,
}

impl Journal {
    /// Every file the install may have written: the confirmed ones, then those of a pending
    /// upgrade, each path once. The only ones an uninstall or a rollback ever removes.
    pub fn listed(&self) -> Vec<&str>;
}
```

- `Journal::VERSION` sigue en `1`: el campo es opcional y aditivo, y `Journal` no usa `deny_unknown_fields`.
- Todos los literales `Journal { … }` (`install.rs:install` y `health_tests.rs`) añaden `upgrade: None`.

### 4.3 Salud — `crates/core/src/guardrails/health.rs` (slice D)

Reglas de `integrity(common, journal)`, que sustituyen al bucle actual:

1. Los caminos que se miran son `journal.listed()` sin `MANIFEST`, y se mantiene el rechazo de componentes no `Normal`.
2. Hashes aceptados para un camino = el de `journal.files` (si existe) ∪ los de `journal.upgrade.files` con ese camino.
3. Si falta un archivo que está en `journal.files`, es `DispatcherMissing`. Si falta uno que solo está en `upgrade.files`, no pasa nada (aún no se escribió), **salvo** por la regla 5.
4. Un archivo presente cuyo hash no está entre los aceptados es `DispatcherAltered`. Las comprobaciones de que sea regular, de `read_regular` y del bit de ejecución no cambian.
5. **Coherencia**: si `dispatch.conf` coincide con un hash de `upgrade.files` y no con el de `files`, cada `hooks/*` de `upgrade.files` tiene que existir y coincidir con uno de sus hashes de `upgrade.files`. Si no, `DispatcherAltered`.
6. `check_with` añade `Diagnostic::TemplateOutdated` cuando `journal.template < TEMPLATE_VERSION` **o** `journal.upgrade.is_some()`.
7. `fingerprint` recorre `journal.listed()` en lugar de `journal.files`.

### 4.4 Reglas por tipo de ref (slice B y slice E)

| Ref remota del push | Mínimo (borrado de base, force-push, alias) | Rama protegida | Rutas prohibidas | Configuración protegida |
|---|---|---|---|---|
| `refs/heads/*` | sí (sin cambio) | sí | sí | sí (agente) |
| Otras gobernadas (`refs/for/*`, `refs/custom/*`…) | sí (sin cambio) | no (`protected_branch` exige `refs/heads/`) | sí | sí (agente) |
| **No gobernadas** (`refs/tags/*`, `refs/notes/*`, `refs/remotes/*`, `refs/bisect/*`, `refs/rewritten/*`, `refs/prefetch/*`, `refs/stash`) | no | no | **sí, nuevo** (con plantilla ≥ 3 y `guard.policies`) | no (diferido, § 9) |

`crates/policy/src/guard/mod.rs:policies_of`, el único cambio de la función pura:

```rust
fn policies_of(out, refname, touched, config_touched, ctx, budget) {
    if !refs::is_governed(refname) {
        // A ref Guardrails does not govern (a tag, a note) still uploads commits: only the
        // forbidden paths reach it. The minimum, the protected branches and the protected
        // configuration stay with the governed refs.
        if !ctx.policies.is_empty()
            && let Some(touched) = touched
        {
            policies::forbidden_paths(out, touched, ctx.actor, &ctx.policies, budget);
        }
        return;
    }
    // ... unchanged ...
}
```

`crates/core/src/guardrails/policies.rs:touched`, rama `Operation::Push`: se quita el `if !refs::is_governed(&u.remote_ref) { return None; }`. Toda actualización con `local` = `Oid` se lee con `Hide::RemoteTracking`; un borrado (`Zero`) sigue dando `None`. `config_touched` **no cambia**. `RefTransaction` tampoco: solo lee `refs/heads/*`, y un `git tag` local queda fuera de alcance.

`crates/core/src/guardrails/evaluate.rs:facts`: dentro del `map`, `if !refs::is_governed(&u.remote_ref) { return None; }` antes de `push_ancestry` (D9).

### 4.5 Cliente — `crates/core/src/guardrails/hook.rs` (slice E)

```rust
/// The updates of a `pre-push` one evaluation sees. Every update stays only when the dispatcher
/// is of template [`EVERY_PUSHED_REF`] or later **and** `scope_all` holds (the daemon granted
/// `guard.policies`, or the client itself evaluates the rules for everyone of the floor);
/// otherwise only the governed ones, as templates 1 and 2 have it. `None` when no update is
/// left. Any other operation is returned unchanged.
pub fn push_scope(op: Operation, template: u32, scope_all: bool) -> Option<Operation>;
```

- `push(remote, url, input)`: analiza **todas** las líneas con `input::parse_push_update` (una línea mala sigue siendo `Err(Rule::InputRejected)`) y conserva todas las actualizaciones. Devuelve `Ok(None)` solo si no hay líneas.
- `run`, rama `Hook::PrePush`:
  1. `Ok(Some(op))`: si `args.template < EVERY_PUSHED_REF`, `op = push_scope(op, args.template, false)` y, con `None`, `HookOutcome::allow()` **sin conectar** (es el camino exacto de hoy);
  2. después, como hoy, `same_repo` y `decide`.
- `decide` y `ask_daemon`. `decide` conserva la operación completa (`full`):
  1. tras el `hello`, `granted` = `CAP_GUARD_POLICIES` concedida;
  2. **al daemon** se envía `push_scope(full.clone(), args.template, granted)`. Con `None` no se llama a `guard.evaluate` y la decisión del daemon cuenta como "permitir" (variante nueva `Asked::Nothing(granted)`);
  3. **rama `!policies_granted`** (hoy `hook.rs`, línea ~595): si la decisión del daemon permite, se llama a `floor_rules_for_everyone(args, &push_scope(full, args.template, true)?)`. Con la plantilla 3 o mayor, eso es la **operación completa**, así que un daemon sin la capacidad no queda menos estricto que el modo degradado. Con una plantilla menor que 3, son solo las gobernadas, como hoy;
  4. `Asked::Degraded(cause)`: las actualizaciones gobernadas se evalúan como hoy (`degraded()`, reglas `everyone` del suelo, unión de bases). Con la plantilla 3 o mayor, las no gobernadas siguen el § 4.5.1, caso B. Los dos resultados se combinan con `Evaluation::add` (gana el más restrictivo);
  5. `Asked::NotAuthentic` y `Asked::Failed`: si hay alguna actualización gobernada, deny como hoy. Si **todas** son no gobernadas (plantilla ≥ 3), se aplica el § 4.5.1, caso C;
  6. `run`: si `same_repo` falla y **todas** las actualizaciones son no gobernadas (plantilla ≥ 3), se aplica el § 4.5.1, caso D. Con alguna gobernada, `RepoMismatch` como hoy.
- No cambia nada para `RefTransaction`, `Rebase` ni `Commit`.

#### 4.5.1 Fail-closed condicionado de las refs no gobernadas (condición del coordinador, 2026-10-09)

**Regla.** Con la plantilla 3 o mayor, un push que **solo** lleva refs no gobernadas se deniega por una señal (sin `raptor`, daemon caído, canal no auténtico, error, repo distinto) **solo** si una regla de rutas prohibidas podría gobernar al actor y no se puede verificar. Sin ninguna regla aplicable, el push pasa. **Que una persona sin reglas nunca quede bloqueada tiene prioridad.** Las refs gobernadas no cambian: siguen como hoy.

**Cómo se sabe que no hay regla aplicable.** El cliente lee el **suelo** del repo del dispatcher (`args.common`): el `.gitraptor/settings.json` de la copia de la rama principal, con `layers::LOADER`, como ya hace el modo degradado (`policies::degraded`). Es barato (gix aislado, una lectura) y fiable en la medida en que el suelo lo es (residuo `policy-floor` ya declarado). Los niveles personales (perfil y local) no se leen sin daemon, como hoy.

Funciones nuevas:

```rust
// crates/policy/src/guard/policies.rs (slice B)
impl Policies {
    /// The same rules as if each governed whoever moves the ref (`everyone`): what a client that
    /// cannot tell the actor applies to the refs Guardrails does not govern, so a rule for agents
    /// is not skipped because the agent cannot be told apart. `unreadable` is dropped.
    pub fn every_rule(&self) -> Self;
}

// crates/core/src/guardrails/policies.rs (slice E)
/// The floor alone, every scope: what the client reads without a daemon. `degraded()` becomes
/// `floor(reader).everyone_only()`.
pub fn floor(reader: &RepoReader) -> Policies;

// crates/core/src/guardrails/hook.rs (slice E)
/// Why the client decides an all-ungoverned push alone.
enum Signal { Degraded(Degraded), NotAuthentic, Failed, RepoMismatch }
/// A push of refs Guardrails does not govern only, decided without a usable daemon: it goes
/// ahead unless a forbidden-path rule of the floor could govern the actor and the commits break
/// it or cannot be checked.
fn ungoverned_alone(args: &HookArgs, op: &Operation, signal: Signal) -> HookOutcome;
```

Lógica de `ungoverned_alone`:

1. `evaluate::open(&args.common)` falla → **permite**, con `degraded` puesto si la señal es `Degraded`. Sin poder leer el suelo no se sabe si hay regla; la condición tiene prioridad (residuo R-A).
2. `rules = policies::floor(&reader)`. Si `rules.paths.is_empty()` → **permite** (con `degraded` si aplica). Es el caso de la persona sin reglas.
3. Señal `RepoMismatch`: los commits del push no están de forma fiable en `args.common`. Con una regla → `HookOutcome::deny(Rule::RepoMismatch)`.
4. En los demás casos, `evaluate_commit(&reader, &args.common, op, Vec::new(), CommitContext { policies: rules.every_rule(), ..default })`. Aplica solo `forbidden_paths`, porque `policies_of` no aplica otra cosa a las no gobernadas.
   - Si permite → **permite** (con `degraded` si aplica).
   - Si deniega → la decisión, más el motivo de la señal: `Rule::Degraded` con su `Cause` (como `degraded()`), `Rule::ChannelNotAuthentic` o `Rule::InternalError`, a nivel `System`.

| Caso | Quién decide | ¿Cómo sabe si hay regla? | Sin regla aplicable | Con regla y commits con ruta prohibida o no verificables |
|---|---|---|---|---|
| **A. Falta `raptor`**, o `raptor` falla al arrancar o sale con un código inesperado | dispatcher (`std`) | **No puede saberlo de forma barata y fiable**:<br>· `dispatch.conf` y el diario se escriben al instalar, pero las reglas cambian con cada commit del suelo y con el perfil;<br>· el diario vive en el SQLite del perfil, que el dispatcher no lee;<br>· una marca exportada por el daemon en la instantánea quedaría obsoleta y no vería los cambios del propio repo | **Pasa** con el aviso `Msg::Inactive` o `Msg::InternalPassed` | **Pasa con el mismo aviso**: elegido por la prioridad de la condición. **Residuo R-B**. La salud ya publica `binary-missing` |
| **B. Daemon caído** (degradado: `DaemonUnreachable` o `InstanceMismatch`) | cliente | Lee el suelo (`policies::floor`) | Pasa (con la nota de degradado de siempre) | **Deniega** con la ruta y el motivo de degradado. Una regla `agents` cuenta (`every_rule`), porque el actor no se puede verificar. El mensaje dice cómo seguir: `guard.degraded.retry` |
| **C. Canal no auténtico, o error de RPC** | cliente (no usa la respuesta) | Lee el suelo | Pasa | Deniega con la ruta y `ChannelNotAuthentic` o `InternalError` |
| **D. El repo no coincide** (M-02 del cliente) | cliente | Lee el suelo del repo del dispatcher | Pasa | Deniega con `RepoMismatch` en cuanto existe una regla (los commits no se pueden verificar en ese repo) |

**Residuos declarados** (van a `policy-reach` y a la ficha):

- **R-A**: con el suelo ilegible, un push solo a refs no gobernadas sin daemon pasa.
- **R-B**: sin `raptor` (o si `raptor` falla), un push solo a refs no gobernadas pasa con aviso, sin evaluar.
- **R-C**: sin daemon, las reglas de rutas del perfil y del nivel local no se aplican (como hoy en las ramas).
- **R-D (coste)**: sin daemon, la **persona** que sube a un tag un commit que toca una ruta prohibida a los **agentes** queda bloqueada hasta que arranca el servicio. El mensaje lo dice. En Windows, sin canal todavía, esto ocurre siempre (§ 6).

### 4.6 Actualización en el sitio — `crates/core/src/guardrails/install.rs` (slice D)

Firmas:

```rust
pub fn sha256(bytes: &[u8]) -> String;                       // was pub(crate)
fn outdated(common: &Path, journal: Option<&Journal>) -> bool; // was (common, Option<u32>)
fn upgrade(
    ctx: &GuardCtx<'_>,
    repo_id: &str,
    common: &Path,
    store: &mut RepoStore,
    registry: &GuardRegistry,
    now_ms: i64,
) -> Result<GuardStatus, InstallError>;
/// Points the protection watch at `journal` (the integrity reference of the checks).
fn rewatch(registry: &GuardRegistry, journal: &Journal);
```

- `outdated` es `true` si `journal.upgrade.is_some()`, si `journal.template < TEMPLATE_VERSION` o si se da la condición actual de `dispatch.conf` o de un dispatcher ausente. `plan` le pasa el diario.
- `install` llama a `upgrade(ctx, repo_id, common, store, registry, now_ms)`.
- `repair`:
  1. calcula `leftovers` sobre `journal.listed()`, que incluye los pendientes;
  2. pone `journal.upgrade = None` junto a `journal.template = TEMPLATE_VERSION`;
  3. llama a `writer.remove_file_temporaries(...)` antes de `replace_files`.

**Máquina de estados de `upgrade()`**. `T` es la plantilla confirmada (1 o 2), `H_T` sus hashes y `H_3` los de la plantilla nueva. "Funciona" significa que Git ejecuta un dispatcher que reconoce su `dispatch.conf`, consulta a `raptor` según su plantilla y encadena el hook previo.

| Paso | Punto de corte | Escritura | Disco tras el paso | Diario tras el paso | Salud | Comportamiento de los hooks | Siguiente `raptor guard install` |
|---|---|---|---|---|---|---|---|
| U0 | — | lee el diario `confirmed` y exige `journal.folder`; `Folder::build` (destino) | binarios T + conf T | `files=H_T`, `template=T`, `upgrade=None` | activa, `TemplateOutdated` | plantilla T | subida |
| U1 | `upgrade-journal` | `journal.upgrade = Some{template:3, files: previos ∪ H_3}`; `at_ms`; `save`; `rewatch` | igual | `upgrade` pendiente | activa (todo coincide con `H_T`) | T | subida (idempotente) |
| U2 | — (dentro de U1:after → U3:before) | `writer.remove_file_temporaries(common, folder, &journal.listed())` | sin temporales huérfanos | igual | igual | T | subida |
| U3a | `upgrade-first-dispatcher` | `replace_files` del **primer** ejecutable (orden de `Folder`: `pre-push` primero) | mezcla: un binario 3 y el resto T; conf T | igual | activa (cada archivo es `H_T` o `H_3`) | T: el binario 3 con conf T aplica la vía rápida (D1) | subida |
| U3b | `upgrade-other-dispatchers` | `replace_files` del resto de ejecutables, incluidos `pre-commit` y `commit-msg` si T=1 | binarios 3, conf T | igual | activa | T (los binarios 3 aceptan conf 1 y 2) | subida |
| U4 | — | **re-verificación**: lee cada ejecutable del destino con `health::read_regular` y exige su hash `H_3`; si falla, `InstallError::Failed` y **no** se escribe la conf | igual | igual | activa | T | subida |
| U5 | `upgrade-conf` | `replace_files(&[dispatch.conf])`: **el cambio**, un `rename` atómico | binarios 3, conf 3 | igual | activa (regla 5 satisfecha) | **3**: `pre-push` evalúa toda ref | subida (reescribe lo idéntico y cierra) |
| U6 | `upgrade-manifest` | `replace_files(&[manifest.json])` | todo en 3 | igual | activa | 3 | subida |
| U7 | — | `verify(ctx, common, &hooks_dir)` (solo lectura: cada worktree ve la clave) | igual | igual | activa | 3 | subida |
| U8 | `upgrade-commit` | `files=H_3` (solo el destino de este build), `template=3`, `raptor=ctx.raptor`, `upgrade=None`, `at_ms`; `save`; `rewatch` | igual | confirmado en 3 | activa, sin `TemplateOutdated` | 3 | ya instalado |

Por qué ningún corte deja el repo sin protección:

- **La combinación rota** (un binario T ≤ 2 con conf 3, donde `Conf::read` da `None` y `pre-push` deniega todo y no encadena) **no se puede alcanzar**:
  - la conf solo se escribe en U5, después de U4, que comprueba por hash que cada ejecutable ya es el nuevo;
  - la regla 5 de la salud la detecta si aparece por otra vía.
- **Matar el proceso dentro de un `replace_files`** deja el archivo de destino intacto (el `rename` no ocurrió) y un temporal. U2 lo borra en el siguiente intento y `remove_rest` lo borra al desinstalar.
- **La clave no se toca** en ningún paso. Sin la clave no hay hooks; con ella, siempre hay un dispatcher en cada nombre que el diario confirmado lista.
- **Windows**: un `rename` sobre un ejecutable en uso falla con un error de compartición. `upgrade` devuelve `Failed`, el diario queda pendiente y la salud activa, y se reintenta con la siguiente instalación.

Pasos y puntos que el barrido declara, en orden: `upgrade-journal`, `upgrade-first-dispatcher`, `upgrade-other-dispatchers`, `upgrade-conf`, `upgrade-manifest`, `upgrade-commit` (cada uno con `before` y `after`, 12 puntos). Ningún punto se alcanza dos veces.

### 4.7 Desinstalación — `crates/core/src/guardrails/uninstall.rs` (slice D)

`remove_rest`: con `expected` resuelto, llama a `writer.remove_file_temporaries(common, expected, &listed)` antes de `remove_folder`. `listed` ya es `journal.listed()`, la unión.

### 4.8 Escritura — `crates/git/src/guard_write/{mod,unix,portable}.rs` (slice C)

```rust
impl GuardWriter<'_> {
    /// Removes what `replace_files` leaves when it is killed between writing a temporary and
    /// renaming it: `<file>.gitraptor.tmp-<16 hex>` next to a listed file, regular files only
    /// (never through a link), inside the folder the journal recorded (`expected`). Any other
    /// name is left in place.
    pub fn remove_file_temporaries(
        &self,
        common: &Path,
        expected: FileId,
        listed: &[&str],
    ) -> Result<()>;
}
```

- Valida cada `listed` con `check_relative`.
- El nombre se reconoce con una función privada `is_file_temporary(file_name, candidate)`: `candidate == "<file_name>." + t`, donde `t` cumple el `is_temporary` existente (`gitraptor.tmp-` más 16 hexadecimales).
- **Unix**: `openat` sin seguir enlaces, recorre el directorio padre con `rustix::fs::Dir`, usa `statat(AT_SYMLINK_NOFOLLOW)` para comprobar que es regular, `unlinkat` y `fsync` del directorio. Comprueba la identidad de la carpeta como `replace_files`. **Portable**: `symlink_metadata().is_file()`, `remove_file` y la misma comprobación de identidad.
- Sin carpeta (`NotFound`): `Ok(())`.

### 4.9 Textos — `apps/cli/i18n/{en,es}/guard.txt` y `crates/api/src/guard.rs` (slice F)

- `guard.np.policy-reach` (en): `forbidden paths are checked in the commits that reach a branch or are pushed to any ref: not in a commit nobody moves to a branch or pushes, uncommitted changes, a branch changed on the server, a branch created or renamed with `git branch -c` or `-m` (and with reftable, any rename of a protected branch), or a path the file system spells differently from Git (8.3 short names)`.
- `guard.np.policy-reach` (es): `las rutas prohibidas se comprueban en los commits que llegan a una rama o se suben a cualquier ref: no en un commit que nadie mueve a una rama ni sube, en cambios sin commitear, en una rama cambiada en el servidor, en una rama creada o renombrada con `git branch -c` o `-m` (y con reftable, cualquier renombrado de una rama protegida) ni en una ruta que el sistema de archivos escribe distinto que Git (nombres cortos 8.3)`.
- `guard.status.template-outdated` (en): `note: the hooks come from an older template: pushes to tags and other refs are not checked for forbidden paths until you refresh them with `raptor guard install`.`
- `guard.status.template-outdated` (es): `nota: los hooks vienen de una plantilla antigua: los push a tags y otras refs no se comprueban contra las rutas prohibidas hasta que los refresques con `raptor guard install`.`
- `guard.reason.policy-unverifiable` (O-3): se añade al final (en) `If this push carries many refs at once (for example `git push --tags`), push them in smaller batches.` y (es) `Si este push lleva muchas refs a la vez (por ejemplo `git push --tags`), súbelas por tandas.`
- **Clave nueva** `guard.degraded.retry` (§ 4.5.1, caso B). `apps/cli/src/guard.rs` la imprime después de `guard.degraded` cuando el resultado está degradado **y** deniega.
  - en: `To go on: start GitRaptor (`raptor daemon status` starts it) and push again. With the service running, a person's push is told apart from an agent's.`
  - es: `Para seguir: arranca GitRaptor (`raptor daemon status` lo arranca) y vuelve a hacer el push. Con el servicio en marcha, el push de una persona se distingue del de un agente.`
- `guard.np.policy-reach` (en/es): además del cambio de arriba, nombra los residuos R-A y R-B: `…, nor in a push of only tags or other refs Guardrails does not govern when GitRaptor is not installed at its path or fails, or when the team settings cannot be read` / `…, ni en un push solo a tags u otras refs no gobernadas cuando GitRaptor no está en su ruta o falla, o cuando la configuración del equipo no se puede leer`.
- `crates/api/src/guard.rs`, doc de `NotPreventable::PolicyReach`: quitar "in a push to tags" y decir "pushed to any ref".
- Si cambia algún snapshot `insta` de estos textos, se actualiza dentro de este slice.
- **No** se añade ninguna clave ni variante del API.

### 4.10 CI — `.github/workflows/repo-intact.yml` (slice G)

En el job `guardrails-git-min` (Git 2.38.5), paso "Guardrails suites", se añaden después de las líneas existentes:

```yaml
          # TD-GRD-001: every pushed ref and the template 3 upgrade (cut at each point).
          cargo test -p gitraptor-cli --test guard_td_grd_001_reach --test guard_td_grd_001_upgrade
          cargo test -p gitraptor-core --test td_grd_001_evaluate --test td_grd_001_client --test td_grd_001_upgrade_health
          cargo test -p gitraptor-git --test td_grd_001_file_temporaries
```

No hace falta nada más. Los e2e con prefijo `repo_intact_` ya entran en el gate `repo_intact` de los otros jobs por su nombre. `repo_intact_min` es un mínimo y no se toca.

## 5. Topología: slices disjuntos

Orden: **T** (pruebas rojas y stubs) → (**A** ‖ **B** ‖ **C**) → (**D** ‖ **E**) → (**F** ‖ **G**) → **H**. Ningún archivo está en dos slices.

| Slice | Crate / ámbito | Archivos | Depende de |
|---|---|---|---|
| **T** | pruebas de contrato (rojas) y stubs | NUEVOS: `crates/core/tests/td_grd_001_evaluate.rs`, `crates/core/tests/td_grd_001_client.rs`, `crates/core/tests/td_grd_001_upgrade_health.rs`, `crates/git/tests/td_grd_001_file_temporaries.rs`, `apps/cli/tests/td_grd_001_machine/mod.rs`, `apps/cli/tests/guard_td_grd_001_reach.rs`, `apps/cli/tests/guard_td_grd_001_upgrade.rs`. Los stubs (§ 5.1) los escribe el slice dueño de cada archivo | — |
| **A** | `apps/cli` (dispatcher y e2e existente) | `apps/cli/src/bin/raptor-hook.rs`, `apps/cli/tests/guard_us_grd_018.rs` (M3: `downgrade_to_template_1` y el assert de plantilla usan la plantilla actual) | — |
| **B** | `crates/policy` | `crates/policy/src/guard/mod.rs` (`policies_of`), `crates/policy/src/guard/policies.rs` (`every_rule`), `crates/policy/src/guard/fastpath.rs` (solo el doc de `skippable_push`), `crates/policy/src/guard/tests.rs` (regresión unitaria: tag con `touched` prohibido → deny; `refs/tags/main` sin `ProtectedBranch`) | — |
| **C** | `crates/git` | `crates/git/src/guard_write/mod.rs`, `crates/git/src/guard_write/unix.rs`, `crates/git/src/guard_write/portable.rs` | — |
| **D** | `crates/core` (instalación) | `crates/core/src/guardrails/constants.rs`, `journal.rs`, `install.rs`, `health.rs`, `health_tests.rs`, `uninstall.rs` | C |
| **E** | `crates/core` (cliente y evaluación) | `crates/core/src/guardrails/hook.rs`, `crates/core/src/guardrails/evaluate.rs`, `crates/core/src/guardrails/policies.rs` | B; `EVERY_PUSHED_REF` de D (en el stub) |
| **F** | textos | `apps/cli/i18n/en/guard.txt`, `apps/cli/i18n/es/guard.txt` (`policy-reach`, `template-outdated`, `policy-unverifiable`, nueva `degraded.retry`), `apps/cli/src/guard.rs` (imprime `guard.degraded.retry`; añade la clave a su lista de claves, línea ~1264), `crates/api/src/guard.rs` (solo el doc de `PolicyReach`), snapshots `insta` afectados | — |
| **G** | CI | `.github/workflows/repo-intact.yml` | T |
| **H** | docs | § 10 | A–G |

### 5.1 Stubs para la línea base (reproducen el comportamiento actual, nunca `todo!()`)

- `constants.rs`: `pub const EVERY_PUSHED_REF: u32 = 3;`. **`TEMPLATE_VERSION` sigue en 2** en el stub; sube a 3 en la implementación.
- `journal.rs`: `Upgrade` y el campo `upgrade` (con `serde(default)`). `listed()` no cambia y la salud lo ignora.
- `hook.rs`: `pub fn push_scope(...)` que devuelve solo las gobernadas (lo de hoy), sea cual sea `scope_all`. `decide` no cambia en el stub.
- `install.rs`: `sha256` pasa a `pub`.
- `guard_write/mod.rs`: `remove_file_temporaries` que devuelve `Ok(())` sin hacer nada.

## 6. Matriz de plataformas

| Aspecto | macOS | Linux | Windows |
|---|---|---|---|
| Plantilla 3 en el dispatcher (D1) | Soportado; e2e locales | Soportado; CI ubuntu (última Git y **2.38.5**) | Compila y se comporta igual (código común). **Pendiente: etapa de validación multiplataforma** |
| Rutas prohibidas en tags y notes | Soportado (daemon con `guard.policies`; sin daemon, § 4.5.1) | Soportado; CI | Sin transporte del canal, el cliente está siempre degradado. Con la plantilla 3 aplica a tags y notes **todas** las reglas de rutas del suelo (`every_rule`, § 4.5.1, caso B). Coste R-D: una persona que sube a un tag un commit que toca una ruta prohibida a los agentes queda bloqueada hasta que exista el canal. Sin reglas de rutas, nada cambia. Sin `raptor`, un push solo a tags pasa con aviso. **Pendiente: etapa de validación multiplataforma** (XP) |
| Subida 1/2 → 3 con cortes | Soportado; barrido e2e | Soportado; CI | `portable.rs:replace_files` con la DACL privada; un `rename` sobre un `.exe` en uso falla y queda pendiente, con reintento. Sin e2e (no hay canal). **Pendiente: etapa de validación multiplataforma** (nueva XP en `xplat-pendientes.md`) |
| `remove_file_temporaries` | `unix.rs` | `unix.rs` | `portable.rs`; el test de `crates/git` corre también en Windows (el job no bloquea) |

## 7. NFR

- **NFR-GRD-04, latencia.**
  - Un push solo a tags pasa de vía rápida (< 30 ms p95, sin `raptor`) a **una evaluación gobernada: < 100 ms p95** por evaluación y por comando.
  - Sin una regla de rutas que gobierne al actor, `touched` no lee nada (`needs_paths`) y `facts` se salta las no gobernadas (D9). El coste es un proceso `raptor hook` y una ida y vuelta al daemon, como un push de rama.
  - Los pushes de rama no cambian. Un push mixto solo lee más si hay una regla.
  - Se mide con `latency_report_push_to_a_tag` (`#[ignore]`, release, máquina en reposo): plantilla 2 frente a 3, para un push de 1 y de 5 tags, con y sin regla de rutas. El presupuesto es un delta p95 ≤ 100 ms frente a la plantilla 2. **Nunca hay un assert de tiempo en debug.**
- **NFR-01 y NFR-12, cero pérdida y transaccionalidad.** Se cumplen con la máquina de estados del § 4.6, el barrido de cortes y la huella (§ 8).
- **NFR-GRD-05, fail-safe.** Con la plantilla 3, `pre-push` deniega por señales de ataque (canal no auténtico, repo cruzado, daemon caído, error interno) también en un push que solo lleva refs no gobernadas, **pero solo si una regla de rutas prohibidas del suelo podría gobernar al actor y no se puede verificar** (O-2 con la condición del coordinador, § 4.5.1). Sin regla aplicable, el push pasa. Sin `raptor`, un push solo a refs no gobernadas pasa con aviso (R-B). Las refs gobernadas no cambian.
- **Coste fail-closed declarado (O-3).** Lo no verificable deniega **solo** cuando una regla de rutas gobierna al actor. Con `appliesTo: agents` (por defecto), el `push --tags` de una persona no se ve afectado. Hay dos casos:
  - un push con más de 256 refs que se leen (`MAX_READS`) da `unverifiable` desde la ref 257;
  - un tag que apunta a un árbol o a un blob da `unverifiable`. Un tag anotado se lee como su commit (L-03 de DS-US-GRD-008).

  Con O-1, el mismo coste aplica en modo degradado con las reglas `everyone`. El mensaje `guard.reason.policy-unverifiable` sugiere empujar por tandas (§ 4.9).
- **Observabilidad.** No hay eventos ni campos nuevos. Una denegación en un tag entra en el registro como cualquier denegación de push (`log.rs:entry`); los permitidos no se registran (como hoy). La subida sigue auditada como el comando reservado `guard install`. Con D7, el monitor deja de emitir la alerta falsa tras la subida.
- **Seguridad.** Se toca la ejecución de procesos (dispatcher), el borrado de archivos en la carpeta de Guardrails (`remove_file_temporaries`) y la transacción de instalación: **requiere revisión de `security-expert`** antes del PR (§ 11).

## 8. Plan de pruebas

Reglas comunes:

- solo repos temporales (`Fixture`), `HOME` temporal, `GIT_CONFIG_NOSYSTEM=1`, `PATH` fijo y perfil temporal (NFR-01);
- debug assertions;
- nada de `sleep` fijos: se espera la señal (pid del daemon, salida del proceso);
- los e2e son `#![cfg(unix)]` y el agente es `raptor-fake-agent`, con su `#[test] fn fake_agent_entry()` en **cada** binario de test que lo use;
- la plantilla 1 o 2 se simula con el helper `downgrade_to(template)` del módulo `td_grd_001_machine`. El helper:
  1. para el daemon;
  2. pone `template\t<N>` en `dispatch.conf` y, si N=1, borra `hooks/pre-commit` y `hooks/commit-msg`;
  3. reescribe el diario del perfil con `Journal::from_json` y `to_json`: `template=N`, `files` sin los borrados y el hash de `dispatch.conf` recalculado con `install::sha256`, `upgrade=None`.

  Así la instalación simulada está **activa** y no alterada (no es la reparación de M3).

| Prueba (archivo :: función) | Qué comprueba | Criterio |
|---|---|---|
| `guard_td_grd_001_reach.rs :: an_agent_cannot_push_a_forbidden_path_to_a_tag` | Commit separado del agente con `secrets/new.txt` → `git push origin HEAD:refs/tags/x` denegado; el mensaje nombra la ruta; el tag no existe en el remoto; un commit limpio al mismo tag pasa | TD001-TAG |
| `… :: an_agent_cannot_push_a_forbidden_path_to_notes` | Lo mismo con `HEAD:refs/notes/x` | TD001-NOTES |
| `… :: an_agent_cannot_push_a_forbidden_path_to_another_ungoverned_ref` | Lo mismo con `HEAD:refs/remotes/mirror/smuggled` | TD001-OTHER |
| `… :: without_raptor_only_tags_go_ahead_with_a_warning` | Plantilla 3, regla de rutas para agentes y `raptor` renombrado (caso A):<br>· el push de un commit a un tag **pasa** con el aviso `protection inactive` (en la base no hay aviso: rojo);<br>· el push a una **rama** de trabajo se deniega con el mensaje de binario ausente (las gobernadas no cambian) | TD001-NORAPTOR |
| `… :: with_the_daemon_down_a_tag_push_without_path_rules_goes_ahead` | Plantilla 3, suelo **sin** `forbiddenPaths` (solo `protectedBranches`), daemon parado. La persona sube a `refs/tags/x` un commit que toca `secrets/`: **pasa**, el tag llega al remoto y la salida lleva la nota `guard.degraded` (prueba de que se evaluó y no se saltó; en la base no aparece: rojo) | TD001-DOWN-NORULE |
| `… :: with_the_daemon_down_a_path_forbidden_to_agents_on_a_tag_is_denied_with_how_to_continue` | Plantilla 3, `forbiddenPaths: ["secrets/"]` (`agents`, por defecto), daemon parado. El agente sube a `refs/tags/x` un commit que toca `secrets/new.txt`: **deniega**; la salida nombra la ruta y contiene `guard.degraded.retry` ("To go on: start GitRaptor…"); el tag no llega al remoto. Un commit limpio al mismo tag pasa | TD001-DOWN-AGENT |
| `… :: in_degraded_mode_an_everyone_rule_denies_a_forbidden_path_to_a_tag` | Plantilla 3, regla `forbiddenPaths` con `appliesTo: everyone` en el suelo, daemon parado (el cliente no lo arranca): el push **de la persona** a `refs/tags/x` de un commit con ruta prohibida se deniega (motivo `guard.degraded` además de la ruta); un commit limpio pasa | TD001-DEGRADED |
| `… :: latency_report_push_to_a_tag` (`#[ignore]`) | § 7; solo imprime | — (evidencia del PR) |
| `td_grd_001_evaluate.rs :: forbidden_paths_apply_to_every_pushed_ref` | `serve_as` con `Caller{actor: agente, policies: true}`: push a `refs/tags/x`, `refs/notes/x` y `refs/remotes/mirror/x` de un commit con ruta prohibida → `Deny` con `Rule::ForbiddenPath`; commit limpio → `Allow` | TD001-EVAL |
| `… :: protected_branch_stays_on_branches_and_forbidden_paths_reach_tags_and_notes` | Un push con `refs/heads/main` (limpio), `refs/tags/main` y `refs/notes/main` (con ruta prohibida) y `protectedBranches: ["main"]` → `ProtectedBranch` solo con rama `main` de `refs/heads`, y `ForbiddenPath` en los otros; ninguna `ProtectedBranch` por un tag o una nota | TD001-BRANCH |
| `… :: ungoverned_refs_fail_closed_only_when_a_path_rule_governs_the_actor` | O-3, con `forbiddenPaths` de `appliesTo: agents`: un push de 257 tags nuevos (el mismo commit limpio) → el agente da `Deny` con `Cause::Unverifiable` y la persona da `Allow`; un tag anotado a un commit limpio → `Allow` para el agente; un tag a un árbol → `Deny` `Unverifiable` para el agente | TD001-BULK |
| `… :: a_daemon_without_guard_policies_evaluates_no_ungoverned_ref` | `Caller{policies: false}` con un push a un tag con ruta prohibida → `Allow`: el daemon sin la capacidad no aplica políticas (regresión, verde en la base) | — |
| `td_grd_001_client.rs :: capability_and_template_decide_which_pushed_refs_are_sent` | Sin `guard.policies`, el daemon no recibe las líneas no gobernadas y el suelo `everyone` sí se aplica a ellas. En `push_scope`:<br>· (3, false) es lo que se envía al daemon sin la capacidad: solo las gobernadas y, con solo tags, `None`;<br>· (3, true) conserva tags y notas: es lo que se envía con la capacidad y lo que evalúa el suelo `everyone` sin ella;<br>· (2, true) y (1, true) dejan solo las gobernadas | TD001-NOCAP |
| `… :: the_hook_accepts_templates_1_2_and_3` | `HookArgs::parse` acepta `"1"`, `"2"` y `"3"`, y rechaza `"4"` y `"0"` | TD001-TEMPLATES |
| `… :: an_ungoverned_push_signal_blocks_only_with_an_applicable_rule` | `hook::run`, plantilla 3, push solo a un tag (caso D: `HookEnv.git_dir` de otro repo). Sin `forbiddenPaths` en el suelo → permitido. Con una regla → `RepoMismatch`. Y con el daemon ausente (caso B): sin regla → permitido con `degraded` puesto; con una regla `agents` y la ruta tocada → denegado | TD001-SIGNALS |
| `… :: degraded_mode_scopes_ungoverned_refs_by_template` | `hook::run`, sin daemon (canal vacío), regla `everyone` en el suelo y push solo a un tag con ruta prohibida: con plantilla 3 → denegado y `degraded` es `Some`; con plantilla 2 → permitido (como hoy) | — (regresión de O-1 a nivel de cliente) |
| `td_grd_001_upgrade_health.rs :: a_pending_upgrade_is_active_at_every_mix_and_the_new_conf_needs_new_dispatchers` | Archivos sintéticos: todo `H_T`, mezcla y todo `H_3` con conf T → activa + `TemplateOutdated`; conf `H_3` y todo `H_3` → activa; conf `H_3` y un dispatcher `H_T` → `DispatcherAltered`; dos hashes por camino (dos builds) → ambos aceptados | TD001-HEALTH |
| `… :: listed_covers_the_files_of_a_pending_upgrade` | `listed()` = unión sin duplicados, primero los confirmados | TD001-LISTED |
| `td_grd_001_file_temporaries.rs :: remove_file_temporaries_removes_only_temporaries_of_listed_files` | Borra `hooks/pre-push.gitraptor.tmp-0123456789abcdef`; deja un enlace con ese nombre, `pre-push.gitraptor.tmp-xyz`, `otro.gitraptor.tmp-…` (no listado) y el propio `pre-push`; con otra identidad de carpeta → `Changed` | TD001-TEMP-GIT |
| `guard_td_grd_001_upgrade.rs :: repo_intact_templates_1_and_2_keep_working_until_reinstalled` | Con cada plantilla simulada: el estado es `HooksOnly` con `TemplateOutdated`; el mínimo se cumple (`branch -D main` denegado); el push del agente con ruta prohibida a un tag **pasa** (como hoy) | TD001-KEEP |
| `… :: repo_intact_a_template_1_install_is_upgraded_to_3` | `guard install --yes` termina sin pantalla de reparación; `dispatch.conf` dice 3; están los 5 dispatchers; sin `TemplateOutdated`; el push con ruta prohibida a un tag queda denegado; el mínimo se cumple | TD001-UP1 |
| `… :: repo_intact_a_template_2_install_is_upgraded_to_3` | Lo mismo desde la 2 | TD001-UP2 |
| `… :: repo_intact_an_upgrade_from_2_cut_at_any_point_keeps_a_working_dispatcher` | Pasada sin cortes con traza = los 12 puntos declarados, en orden. Por cada punto: el daemon muere con 86, se rearranca (`daemon status`) y se comprueba: (a) cada dispatcher de la plantilla de partida es un ejecutable regular; (b) `HooksOnly`, sin causa de pérdida; (c) funciona: `branch -D main` denegado, un push **limpio** a una rama de trabajo **pasa** y la salida no contiene `moved or altered` ni `hooks it already had did not run`; (d) la siguiente `guard install --yes` termina sin reparación, en plantilla 3, y el push con ruta prohibida a un tag queda denegado; (e) la carpeta no tiene temporales | TD001-CUT2 |
| `… :: repo_intact_an_upgrade_from_1_cut_at_any_point_keeps_a_working_dispatcher` | Lo mismo desde la 1 (añade `pre-commit` y `commit-msg` a mitad) | TD001-CUT1 |
| `… :: repo_intact_an_uninstall_after_an_interrupted_upgrade_leaves_no_trace` | Desde la 1, corte en `upgrade-other-dispatchers:after`, rearranque y `guard uninstall`: huella = repo prístino con `uninstalled_exceptions` (sin `pre-commit` ni `commit-msg` huérfanos) | TD001-UNINSTALL |
| `… :: repo_intact_a_temporary_of_a_killed_write_is_cleaned_by_the_next_install` | Corte en `upgrade-journal:after`; se planta `hooks/pre-push.gitraptor.tmp-0123456789abcdef` (el estado de un proceso matado entre la escritura y el `rename`); `guard install --yes` termina y el archivo desaparece; `guard uninstall` deja el repo prístino | TD001-TEMP-E2E |
| `… :: repo_intact_an_upgrade_changes_only_the_dispatchers_and_the_journal` | Instantánea antes de proteger; tras simular la 2 y subir a la 3, el diff frente a la prístina solo admite `install_exceptions()` (la clave, `gitraptor/` y el perfil, donde vive el diario) | TD001-FOOTPRINT |
| `guard_us_grd_018.rs` (ajustado, slice A) | `downgrade_to_template_1` busca `"template\t{TEMPLATE_VERSION}\n"`; el assert pasa a `"template\t3\n"` | regresión |
| `crates/policy/src/guard/tests.rs` (slice B) | Unitarias de `policies_of` con `refs/tags/*` | regresión |
| `health_tests.rs` (slice D) | Literales con `upgrade: None`; un caso de coherencia | regresión |
| Monitor (D7, slice D) | Tras `install::install` en modo subida (patrón de llamada directa de `apps/cli/tests/guard_windows.rs`), `registry.protection().get(repo_id)` lleva el diario con `template == 3` y `upgrade == None`. Si el montaje en `crates/core` sale demasiado caro, el experto lo declara en el PR | regresión |

**Comando de aceptación** (local, macOS):

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
node tools/test/nextest-junit.mjs        # misma suite que cargo test --workspace, con informe por archivo
cargo test --doc --workspace
CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true cargo test --release -p gitraptor-cli --test guard_td_grd_001_reach latency_report_push_to_a_tag -- --ignored --nocapture
```

CI: la línea base es `repo-intact.yml` (ubuntu y macOS), más el job `guardrails-git-min` con Git 2.38.5 (§ 4.10).

## 9. No se construye (diferido)

- **Configuración protegida (BR-AUTH-004) en refs no gobernadas**: se añade si un tag pasa a poder cambiar la configuración en vigor (hoy el suelo sale de la rama principal, no de un tag).
- **Rama protegida o mínimo en refs no gobernadas**: se añade si una política pide proteger tags (otra historia y otra regla).
- **Rutas prohibidas en `reference-transaction` de refs no gobernadas** (`git tag` local, `update-ref refs/notes/...`): se añade si el residuo "commit aparcado en una ref local" se prioriza. Hoy lo cubre el push.
- **Denegar a un agente un push a una ref no gobernada con la configuración ilegible** (SEC-GRD-17 está ligado a la rama protegida): se añade si SEC-GRD-17 se extiende a las rutas.
- **Deduplicar lecturas por `(viejo, nuevo)` en `touched`** (M6): se añade cuando un `push --tags` de más de 256 refs se deniegue en la práctica.
- **Completar una subida interrumpida al arrancar el daemon**: se añade si la UX lo pide. Hoy la completa `raptor guard install` (D11).
- **Diagnóstico propio de "subida a medias"** (`Diagnostic::UpgradePending`): se añade si `TemplateOutdated` no basta para explicarlo.
- **Un `Machine` de e2e común con agente** (M7): se añade cuando una cuarta historia lo copie.
- **Otros residuos de `policy-reach`** (objetos sueltos, `stash`, `update-ref` a mano, servidor, `reftable`, `send-pack`): fuera de alcance por la ficha.

## 10. Documentación (slice H)

- ADR-GRD-001: **Enmienda (2026-10-09, TD-GRD-001)**. Recoge:
  - la plantilla 3 (§ 2, § 8);
  - que el binario acepta las plantillas 1, 2 y 3 (alinea M5);
  - la subida en el sitio con diario pendiente, puntos de corte y el invariante "conf 3 solo con binarios 3";
  - la consecuencia en el § 3: el `fallback` de `pre-push` deja pasar con aviso un push que solo lleva refs no gobernadas (R-B), y lo demás falla cerrado como hoy (condición del coordinador).
- ADR-GRD-002: **Enmienda (TD-GRD-001)**. En `pre-push` la vía rápida de refs no gobernadas solo vale para las plantillas 1 y 2. La nota de la línea 237 queda como cerrada.
- ADR-GRD-003: nota en la Enmienda de US-GRD-008. Dice tres cosas:
  - `policy-reach` deja de incluir "un push solo a tags u otras refs no gobernadas";
  - con la plantilla 3, `pre-push` también deniega por señales de ataque un push que solo lleva refs no gobernadas, **condicionado a que exista una regla de rutas prohibidas aplicable** que no se pueda verificar (O-2 con la condición del coordinador; § 4.5.1). Sin regla aplicable, pasa;
  - sin daemon, a esas refs se les aplican todas las reglas de rutas del suelo, porque el actor no se puede verificar (O-1 y § 4.5.1, caso B). Residuos R-A a R-D.
- `docs/architecture/non-functional-guardrails.md`, línea 24 (NFR-GRD-05): una nota que dice tres cosas:
  - con la plantilla 3, las señales de ataque dan deny en `pre-push` también para las refs no gobernadas, **pero solo si existe una regla de rutas prohibidas aplicable** que no se puede verificar; una persona sin reglas nunca queda bloqueada;
  - sin `raptor`, un push solo a refs no gobernadas pasa con aviso;
  - se declara el coste fail-closed de O-3.
- DS-US-GRD-008: el párrafo `policy-reach` (§ límites) y la fila "Push solo a tags… → TD-GRD-001" pasan a cerrados. US-GRD-008 (línea 113): igual.
- DS-US-GRD-018 § 11, S8: nota que remite a este brief (el diario guarda la subida como pendiente en lugar de listar primero los hashes nuevos; puntos de corte).
- Ficha TD-GRD-001:
  - corrige "dispatcher `sh`" por "dispatcher nativo";
  - **añade a su Alcance Técnico** la corrección de los dos defectos de la subida 1→2 que esta historia arregla: republicar el `Watched` del monitor (M1) y la subida interrumpida que se veía como protección inactiva (M2);
  - añade la limpieza de temporales de escrituras matadas (M4);
  - enlaza este brief;
  - el `status` lo pone el PR (memoria de seguimiento). Se actualizan `technical-stories.md` y `backlog.md` (línea 38).
- `docs/architecture/xplat-pendientes.md`: una XP nueva para la subida 1/2→3 en Windows (rename sobre un `.exe` en uso, DACL) y el push a tags sin canal.
- **No se tocan**: `docs/architecture/decisions/ADR-GRD-008-factor-autenticacion-fuera-de-banda.md` ni `docs/ARTIFACTS.md`.

## 11. Seguridad (para `security-expert`)

- **El dispatcher arranca `raptor`** en más casos (un push solo a tags), siempre con el mismo argv fijo y el entorno de la allowlist. No hay ejecución nueva.
- **`remove_file_temporaries` borra archivos** en una carpeta que otro proceso del usuario puede tocar:
  - solo nombres con el patrón exacto, junto a un archivo listado;
  - solo archivos regulares, sin seguir enlaces;
  - con la identidad de la carpeta del diario.

  Revisar las condiciones de carrera (TOCTOU) entre `statat` y `unlinkat`. **Alcance acotado (M4)**: solo los temporales de **archivos listados en el diario** y solo dentro de la carpeta de Guardrails (`<common>/gitraptor/`). Nunca un archivo de fuera de esa carpeta, ni un nombre que no sea `<archivo listado>.gitraptor.tmp-<16 hex>`, ni un borrado recursivo.
- **Fail-closed condicionado (§ 4.5.1)**: para un push que solo lleva refs no gobernadas, la señal de ataque ya no basta para denegar. Hay que revisar que:
  - `ungoverned_alone` nunca use la respuesta de un canal no auténtico;
  - lea el suelo de `args.common` (las constantes) y no el `GIT_DIR` de la transacción;
  - los residuos R-A y R-B (suelo ilegible, `raptor` ausente) no se puedan provocar de forma barata por un agente. Borrar `raptor` lo publica la salud como `binary-missing`.
- **La salud acepta dos hashes por camino** mientras hay una subida pendiente. Los dos los escribe el propio daemon. La regla 5 impide que se acepte la combinación rota.
- **Superficie nueva hacia el daemon**: las refs no gobernadas, ya validadas por `evaluate.rs:valid` (`RefName::new`).

## 12. Preguntas abiertas y supuestos

**Decisión del orquestador (2026-10-09), validada por Arquitecto**, sobre O-1, O-2 y O-3:

- **O-1 (modo degradado): opción ESTRICTA.**
  - En `Asked::Degraded`, las refs se acotan por la plantilla y no por la capacidad: `push_scope(op, args.template, true)`. Con la plantilla 3 o mayor, el modo degradado aplica a tags, notes y demás refs las `forbidden_paths` de las reglas `everyone` del suelo, por el camino existente (`degraded()` → `evaluate_commit` → `policies_of`). Con una plantilla menor que 3, se filtran como hoy.
  - **Ajuste 1**: en la rama `!policies_granted` de `decide`, con la plantilla 3 o mayor, `floor_rules_for_everyone` recibe la operación completa, para que un daemon sin la capacidad no quede menos estricto que el modo degradado.
  - El daemon sin la capacidad sigue sin recibir las líneas no gobernadas.
  - **Ajuste 2**: D3 actualizada; el diferido del § 9 queda retirado; criterio TD001-DEGRADED.
- **O-2 (fail-closed nuevo): aceptado, con la condición del coordinador de abajo.** Con la plantilla 3, `pre-push` también deniega por señales de ataque en refs no gobernadas, **solo con una regla aplicable**. Se mantiene la consecuencia planeada en ADR-GRD-001 § 3 y se añaden notas en NFR-GRD-05 y en ADR-GRD-003 (§ 10).
- **O-3 (coste fail-closed): aceptado como coste declarado.**
  - Solo deniega si una regla de rutas gobierna al actor. Con `agents` por defecto, el `push --tags` de una persona no se ve afectado.
  - Un tag anotado se lee como su commit. Solo se deniega un tag que apunta a un árbol o a un blob.
  - Con O-1, el coste también aplica en modo degradado con las reglas `everyone`.
  - El mensaje `guard.reason.policy-unverifiable` sugiere empujar por tandas (§ 4.9).
  - Criterio TD001-BULK.
- **Condición del coordinador al aprobar el plan (2026-10-09), sobre O-2.**
  - Con la plantilla 3, el fail-closed de un push que solo lleva refs no gobernadas aplica **únicamente** cuando una regla gobierna al actor y no se puede verificar (`forbidden_paths` del actor o del suelo `everyone`).
  - Sin ninguna regla aplicable, el push de tags pasa: una persona que empuja un tag en un repo sin rutas prohibidas no puede quedar bloqueada, ni siquiera con el daemon caído.
  - Aplicación: § 4.5.1, casos A a D, con los residuos R-A a R-D. Las refs gobernadas no cambian.
  - Criterios: TD001-NORAPTOR (sustituye a TD001-FAILCLOSED), TD001-DOWN-NORULE, TD001-DOWN-AGENT y TD001-SIGNALS.
- **Exceso de alcance declarado**: M1, M2 y M4 se quedan. El slice H los nombra en el Alcance de la ficha. M4 se limita a los temporales de archivos listados en la carpeta de Guardrails (§ 11).
- ⚠️ **ASSUMPTION**: una subida pendiente se presenta como `HooksOnly` con la nota `TemplateOutdated`, sin un diagnóstico nuevo.
- ⚠️ **ASSUMPTION**: el refspec `HEAD:refs/remotes/mirror/smuggled` representa "otra ref no gobernada" en las pruebas; `refs/bisect/*` y `refs/rewritten/*` siguen el mismo camino de código (`is_governed`).

## Traspaso

`rust-expert`: implementa este brief con el contrato `docs/dev-briefs/td-grd-001-dispatcher-template-3.contract.json`. El orden es:

1. Slice **T**: las pruebas rojas y los stubs del § 5.1, que deben compilar y estar en rojo.
2. La línea base.
3. Los slices **A** a **H** en el orden del § 5.

Antes del PR pasa por `security-expert` (§ 11). Si una decisión de este brief choca con el código real, para y pregunta; no la decidas de nuevo.
