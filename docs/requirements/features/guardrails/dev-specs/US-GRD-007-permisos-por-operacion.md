---
id: DS-US-GRD-007
title: "Dev Spec — US-GRD-007: permisos por operación (permitir, pedir confirmación, denegar)"
type: dev-spec
status: draft
created: 2026-10-09
updated: 2026-10-09
story: US-GRD-007
feature: guardrails
domain: GRP
scope: backend
frontend_surface: false
stack: rust
profile: backend-service
tooling: [cargo]
related:
  context: ../context.md
  story: ../user-stories/US-GRD-007-permisos-por-operacion.md
  stories: [US-GRD-007, US-GRD-010, US-GRD-011, US-GRD-014, US-GRD-016, US-GRD-005, US-GRD-018, US-GRD-008]
  adrs: [ADR-GRD-001, ADR-GRD-002, ADR-GRD-003, ADR-GRD-004, ADR-GRD-005, ADR-GRD-006, ADR-GRD-007, ADR-GRP-007, ADR-GRP-016]
  rules: [BR-VAL-001, BR-VAL-002, BR-CALC-001, BR-CONS-001, BR-EDGE-001, BR-EDGE-003, BR-AUTH-001, BR-WF-002, BR-CONS-004]
  api_spec: null
  design_spec: null
  contracts: []
must_read:
  - ../user-stories/US-GRD-007-permisos-por-operacion.md
  - ../business-rules.md
  - ../../../../architecture/decisions/ADR-GRD-002-operaciones-interceptables.md
  - ../../../../architecture/decisions/ADR-GRD-003-motor-decision-contrato.md
  - ../../../../architecture/decisions/ADR-GRD-004-configuracion-efectiva.md
  - ../../../../architecture/extender-sin-archivos-compartidos.md
  - ../../../../dev-briefs/layered-config.md
  - ./US-GRD-008-ramas-protegidas-rutas-prohibidas.md
  - ./US-GRD-018-autoria-commits-persona-y-agente.md
  - ../../../../../crates/policy/src/guard/mod.rs
  - ../../../../../crates/policy/src/guard/fastpath.rs
  - ../../../../../crates/policy/src/team.rs
  - ../../../../../crates/policy/src/layers.rs
  - ../../../../../crates/policy/src/authorship/subcommand.rs
  - ../../../../../crates/git/src/guard_paths.rs
  - ../../../../../crates/core/src/guardrails/evaluate.rs
  - ../../../../../crates/core/src/guardrails/layers.rs
  - ../../../../../crates/core/src/guardrails/hook.rs
  - ../../../../../crates/core/src/guardrails/second_line.rs
  - ../../../../../crates/core/src/channel/conn.rs
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
tags: [guardrails, permisos, br-val-002, ask-unavailable, disable-safe-minimum, minimo-seguro, capacidades, segunda-linea]
---

# Dev Spec — US-GRD-007: permisos por operación

Plano compacto (AADD ligero) de [US-GRD-007](../user-stories/US-GRD-007-permisos-por-operacion.md): la configuración fija para cada operación del catálogo de [BR-VAL-002](../business-rules.md) un permiso (`allow`, `ask`, `deny`), y la capa de hooks lo aplica con Git directo en las operaciones que [ADR-GRD-002](../../../../architecture/decisions/ADR-GRD-002-operaciones-interceptables.md) declara interceptables. La combinación de niveles ya existe ([brief de US-GRD-010/012](../../../../dev-briefs/layered-config.md), `Layers::permissions()`); esta historia la **aplica** en la decisión, hace real `disableSafeMinimum` y produce `MinimumSetStatus::DisabledByTeam`. Sigue el patrón de [DS-US-GRD-008](./US-GRD-008-ramas-protegidas-rutas-prohibidas.md): regla pura en `crates/policy`, configuración y hechos leídos en el daemon, nada nuevo viaja desde el cliente del hook.

**Invariante**: un permiso solo añade `deny` o `ask` sobre lo que ya decide el resto. La única relajación es `disableSafeMinimum`, y solo desde el suelo confirmado y legible. GitRaptor nunca mueve ni borra nada para "arreglar" un bloqueo.

**Restricciones de arquitectura**: no hay `architecture-constitution.md` en la cascada. Rigen `AGENTS.md` (Rust, NFR-01, NFR-02, tests obligatorios) y los ADRs citados, como en las Dev Specs hermanas.

## 1. Decisiones

Cada fila es una **Decisión del orquestador (2026-10-09), validada por el Arquitecto**. Las marcadas **(PO)** cambian lo que se promete al usuario: las validó también el PO y llevan su Q-GRD (§ 9).

| # | Decisión |
|---|---|
| D1 | **Regla por operación** (BR-CALC-001). El contrato gana una regla por operación del catálogo: `permission.commit`, `permission.push`, `permission.force-push`, `permission.reset-hard`, `permission.branch-delete`, `permission.rebase`, `permission.merge`, `permission.worktree-add` y `permission.worktree-remove`. Son nueve, aunque los hooks solo produzcan siete (§ 3): así US-GRD-016 las reutiliza sin tocar el enum. Una regla por operación, y no una sola con parámetro, porque el registro guarda las razones **sin parámetros** (`LoggedReason`), y sin el código no se sabría qué permiso denegó |
| D2 | **Una razón por nivel que produce el máximo**. `OperationRule.sources` ya lista las fuentes del máximo. Cada fuente da una razón con su `Level`: `Floor` (también `ConfirmedFloor`), `Worktree`, `Profile` o `Local`. `RuleSource::SafeMinimum` **no** da razón de permiso: el mínimo ya produce `minimum.force-push` con su causa. Un force-push denegado por el mínimo y por el equipo nombra las dos. El mensaje nombra la operación y el nivel con las plantillas de nivel que ya existen: `floor` → "configuración del equipo", `worktree` → "configuración del equipo en esta rama". Params: `branch` (nombre corto) y `remote` cuando aplican |
| D3 | **`ask` se aplica como `deny` con `ask-unavailable`** (S-GRD-9). `decision()` ya aplica `ask` como `deny`, pero no lo explica. Ahora, cuando `effect = ask`, cada razón de ese efecto lleva `cause: ask-unavailable` y la plantilla dice "requiere confirmación humana y la confirmación aún no está disponible". Va como **causa** de la razón que produce el `ask`, no como razón `system` aparte: el máximo lo produce el permiso. Enmienda de ADR-GRD-003 § 3 (§ 9). **(PO, Q-GRD-36)** En el registro es una entrada `denial` y cuenta en el KPI, porque el efecto aplicado es `deny` (pendiente de que Rene lo confirme en el PR) |
| D4 | **`disableSafeMinimum` real.** `Context` gana `minimum: bool`, que es `EffectivePermissions.safe_minimum_active`, y `ctx.minimum = false` apaga entero el bloque del mínimo: `minimum.force-push` y `minimum.base-branch-delete`, también el alias y el renombrado sobre la base. Lo que fuerza el mínimo ya lo decide `TeamLoader::load`: suelo confirmado y `Readable`. US-GRD-011 añade el resto de fuentes que lo fuerzan. Con `ctx.minimum = false`, el force-push se rige por `permission.force-push`. Valor por defecto: `true`. Siempre `true` en degradado y sin la capacidad de D10 |
| D5 | **Dónde se decide cada operación** (tabla del § 3). Sin hooks nuevos ni plantilla nueva. **Pre-hooks (momento A)**: `pre-commit` decide `commit`; `pre-push` decide `push`, `force-push` y el borrado remoto (`push` + `branch-delete`); `pre-rebase` decide `rebase`, también en `pull --rebase`. **`reference-transaction` `prepared`**: decide `branch-delete` local, `worktree-add` (crear con rama nueva) y la **segunda línea** de `commit`, `rebase` y `merge`, que cubre `--no-verify`. La segunda línea la clasifica el daemon (D6) dentro de la misma evaluación `RefTransaction`, sin una llamada aparte ni hechos nuevos del cliente. `commit-msg` no evalúa permisos (ya lo hizo `pre-commit`) |
| D6 | **Clasificar una transacción** (función pura `guard::permissions::classify`, tabla del § 3.1). Usa tres hechos que lee el daemon: la línea (borrado, creación o actualización de `refs/heads/*`); la **cadena de `git`** (los `git` antecesores consecutivos del cliente, como mucho 4, cada uno verificado por `(pid, inicio)` y con su línea de órdenes leída **solo para clasificar**, nunca guardada ni enviada, como en DS-US-GRD-018 § 5.3); y los **commits nuevos** del movimiento (cuántos y cuántos con 2 o más padres), con el lector de dos pasadas de DS-US-GRD-008 D5. Lo que no se puede probar (línea de órdenes ilegible, identidad cambiada, tope superado) y haría falta para un permiso distinto de `allow` se deniega con causa `unverifiable`. Nunca se permite por no poder clasificar |
| D7 | **(PO, Q-GRD-37) Qué cuenta como cada operación.** `commit`: todo commit nuevo que llega a una rama, sea cual sea el subcomando que mueve la rama. Las únicas excepciones son los commits que crea un `rebase` (tienen su permiso), el commit de fusión que crea un `merge` o un `pull` (permiso `merge`) y los commits que trae un `fetch` o un `clone` (vienen del remoto). Así, `cherry-pick`, `revert`, `am` y `commit-tree` + `update-ref` cuentan como `commit`, y el plumbing no lo salta. También cuenta un commit hecho fuera de una rama (con `HEAD` separado y `--no-verify`) que después llega a una rama por fast-forward, `branch -f`, `reset` o `switch -C` (D9). Un commit que cierra una fusión (2 o más padres hechos con `git commit`) cuenta como `commit` **y** `merge`. `push` incluye el push forzado y el borrado remoto: un push forzado también necesita `force-push` permitido, y un borrado remoto, `branch-delete`. `branch-delete` incluye renombrar una rama (`branch -m/-M` borra el nombre viejo) y `update-ref -d`. `worktree-add` solo se reconoce cuando crea rama nueva |
| D8 | **(PO, Q-GRD-38) Merge: protección de la rama, sin contar como impedido.** Con `merge` en `deny` o `ask`, la segunda línea deniega el movimiento de la rama. Esa rama no cambia, pero Git ya escribió la fusión en el working tree (momento B, ADR-GRD-002 § 1). Merge **sigue publicado como no impedible** (`NotPreventable::Merge`), y la fila "merge" del esquema de la historia se verifica en US-GRD-016, como dice su nota. El motivo dice cómo volver al estado anterior (recuperar no es una vía de excepción, SEC-GRD-06). ⚠️ **ASSUMPTION**: `git merge --abort` con fusión en curso y `git reset --merge` tras un fast-forward denegado devuelven el índice y el working tree a `HEAD` sin tocar los cambios locales no relacionados. Lo comprueba `merge_deny_keeps_the_branch_and_leaves_a_recoverable_tree`. Si no se cumple, la plantilla remite a la Time Machine |
| D9 | **(PO, Q-GRD-39) Lo que no se evalúa con Git directo** (BR-EDGE-003): `reset --hard` (momento C: cuando llega la ref, el working tree ya se perdió, y denegar no recupera nada), borrar worktree (C) y crear worktree sin rama nueva (`no-reconocible`). Esas filas pasan a US-GRD-016 (MCP). Tampoco se evalúan un push **solo** a refs no gobernadas (solo tags: la vía rápida sale sin evaluar; lo cierra [TD-GRD-001](../technical-stories/TD-GRD-001-dispatcher-plantilla-3-pre-push-toda-ref.md)) ni el propio `commit --no-verify` con `HEAD` separado (`HEAD` separado no es gobernado). Se publica como `NotPreventable::PermissionReach`. **El commit separado no llega a una rama sin evaluarse** (precisión del PO): el hueco de "commit en `deny` y merge en `allow`, commit separado y después fast-forward" se **cierra**, no se declara. Todo movimiento de una rama que trae commits **frescos** (los que no alcanza ninguna otra rama ni rama remota conocida; DS-US-GRD-008 D5) evalúa `commit` por ellos, salvo los de las excepciones de D7 (§ 3.1). Así se cierra también la misma vía con `branch -f`, `reset --soft`, `switch -C`, `checkout -B` y `update-ref`, y no solo con merge: declararlo dejaría abierta una familia entera de comandos corrientes. Un fast-forward a una rama local o remota que ya existe no trae commits frescos, así que no da falsos positivos. Coste en D14. Quedan dos vías, declaradas en `permission-reach`: subir el commit separado directamente con `push` (rige `push`, no `commit`) y usarlo como **base** de un `rebase` (sus commits frescos son del rebase) |
| D10 | **Capacidad `guard.permissions`** (ADR-GRP-016; patrón D9 de DS-US-GRD-008). Cubre las reglas `permission.*`, la causa `ask-unavailable`, `NotPreventable::PermissionReach` y el comportamiento: permisos, `ctx.minimum` y carga de capas para `Rebase` y `Commit`. **Daemon nuevo con hook sin la capacidad**: evalúa como hoy (mínimo siempre activo, sin permisos) y en `guard.log` quita las entradas con razones `permission.*` (siguen contando en `summary.blocked`). **Hook nuevo con daemon sin la capacidad**: el cliente aplica él mismo los permisos del equipo solo para endurecer (D12), así un daemon de otra versión nunca deja menos que el modo degradado. Es una puerta de compatibilidad, no de seguridad. Tests en las dos direcciones |
| D11 | **Una carga de capas por evaluación, para toda operación gobernada.** Con la capacidad, `serve_audited` carga `layers::load` también para `Rebase` y `Commit`, no solo para `RefTransaction`/`Push` con `guard.policies`. De esa misma carga salen los permisos, `ctx.minimum`, las políticas y la autoría (que deja su lectura aparte `authorship::policy_for`). El worktree es el del cwd del cliente (`worktree_reader`). **Coordinación con US-GRD-014 (D6)**, que hace la misma carga para las bases: es **un solo cambio**. Quien llegue segundo a `main` reutiliza la carga del otro y no añade una segunda |
| D12 | **Modo degradado** (ADR-GRD-003 § 4: nunca menos que el mínimo más el suelo legible). El cliente abre el worktree de su cwd y aplica `TeamLoader::load(reader, None).permissions`: suelo y worktree **solo endurecen**, sin `allow`, sin `disableSafeMinimum` y sin niveles personales, con el mínimo forzado. Clasifica con la misma función pura y la cadena de `git` de su propia ascendencia. Hoy `pre-commit` en degradado deja pasar; ahora evalúa `commit`. La misma función (`permissions::team_only`) cubre al daemon sin la capacidad (D10) |
| D13 | **La vía rápida no cambia.** `fastpath.rs` solo salta las líneas de refs no gobernadas, los valores `ref:` de `HEAD` y los *prunes* de `pack-refs`. Cualquier línea de `refs/heads/*` va a `raptor hook`, y `pre-commit`, `commit-msg` y `pre-rebase` nunca se saltan. Por eso todo `deny` de `commit`, `push`, `rebase`, `branch-delete`, `merge` o `worktree-add` llega al daemon sin coste nuevo por invocación. El único hueco es el push solo a tags (D9). Si falta el binario `raptor`, rige el *fallback* de ADR-GRD-001 § 3 (un commit pasa con aviso): la protección está inactiva y US-GRD-004 lo detecta (`binary-missing`) |
| D14 | **Coste** (ADR-GRD-002 § 5, < 100 ms p95 por evaluación). La carga de capas ya está en la caché por blob (TS-GRD-001). La cadena de `git` se lee solo en una `RefTransaction` y solo si `commit`, `merge`, `rebase` o `worktree-add` no es `allow`. Los commits nuevos se leen solo si `commit` o `merge` no es `allow`, y la segunda pasada (escondiendo las demás ramas) solo si la primera da un candidato a denegar, como en DS-US-GRD-008 D5. Con todo en `allow`, el único coste nuevo es la carga, que ya pagan las políticas. **Caso caro, declarado**: con `commit` distinto de `allow`, un fast-forward, un `branch -f` o un `reset` que trae commits de otra rama pasa la segunda pasada antes de permitirse. En DS-US-GRD-008 eso costó 0,2 a 0,4 s con 2 000 ramas. Por eso hace falta la segunda pasada: la primera no distingue un commit separado de uno que ya está en otra rama. `git commit`, `cherry-pick`, `revert` y `am` no la pagan, porque crean ellos el commit y la primera pasada basta para denegar. Se mide las dos cosas antes de cerrar (§ 10): el commit corriente contra < 100 ms p95, y el fast-forward caro aparte |
| D15 | **Relajación y confirmación: se reutiliza US-GRD-014, sin comando nuevo.** Confirmar es `raptor guard confirm` (`guard.confirm`, `PendingKind::ConfirmTeam`), que define `DS-US-GRD-014`. Su `ConfirmPlan` (D3 de 014) ya lista `permissions.allow`, `ask`, `deny` y `disableSafeMinimum` en la lista cerrada de relajaciones. Su ventana, el anuncio, la cancelación y el rechazo al agente (`check_reserved`, `RESERVED_REFUSED`, auditado) son los de su D2 y D10. El diagnóstico `floor-relax-pending` y la pista `guard-confirm` del estado son su D8. **US-GRD-007 no define contrato de confirmación**: solo verifica, con sus escenarios 4 y 5, que tras confirmar los permisos y `disableSafeMinimum` rigen |
| D16 | **Orden de implementación y dos PR.** **PR-A** (D1 a D14): escenarios 1, 2, 3 y 6 de la historia. No depende de US-GRD-014: un `deny` del suelo sin confirmar ya endurece (Q-GRD-23), y `disableSafeMinimum` se prueba en `crates/core` con un `Confirmed` construido en el test. **PR-B**: escenarios 4 y 5 de extremo a extremo con `raptor guard confirm`. Depende de US-GRD-014 en `main`. PR-A puede ir en paralelo con US-GRD-014 (comparten solo D11). US-GRD-011 va después de PR-A (DS-US-GRD-011, D10) |
| D17 | **Registro** (US-GRD-005, BR-CONS-004). Toda denegación de permiso entra por `log_decision` con sus reglas `permission.*`, su nivel y su causa. `raptor guard log` las muestra con los textos `guard.log.rule.permission` y `guard.op.*`. No cambia el contrato del registro |
| D18 | **Estado.** `minimum_set.status = disabled-by-team` cuando el suelo en vigor es el confirmado, es legible y desactiva el mínimo. US-GRD-011 añade a esa condición "y ninguna fuente lo fuerza". La variante ya existe en el contrato. El texto (`guard.status.minimum-disabled`) no dice cómo volver a activarlo ni cómo desactivarlo. `not_preventable` gana `permission-reach` (solo con la capacidad) |

## 2. Forma (archivos que se tocan)

Cada pieza nueva va en un archivo propio (ADR-GRP-016, [extender sin archivos compartidos](../../../../architecture/extender-sin-archivos-compartidos.md)); los archivos centrales ganan una línea o un brazo.

| Pieza | Archivo | PR |
|---|---|---|
| Clasificador de la línea de órdenes de `git` (refactor: lo usa también la segunda línea de autoría, sin cambiar su comportamiento) | `crates/policy/src/guard/git_command.rs` (nuevo); `crates/policy/src/authorship/subcommand.rs` (llama al nuevo) | A |
| Clasificar y aplicar permisos (puro) | `crates/policy/src/guard/permissions.rs` (nuevo); `guard/mod.rs`: `pub mod`, `Context.{minimum, permissions}`, `Facts.classified` y una llamada por operación; el bloque del mínimo bajo `ctx.minimum` | A |
| Commits nuevos de un movimiento (cuenta y fusiones, sin rutas) | `crates/git/src/guard_paths.rs` (`fresh_commits`, comparte el recorrido de `fresh_commit_paths`) | A |
| Contrato: reglas, causa, `NotPreventable`, capacidad | `crates/api/src/guard.rs`; `crates/api/src/methods/guard.rs` | A |
| Cadena de `git`, hechos de clasificación, permisos solo de equipo | `crates/core/src/guardrails/permissions.rs` (nuevo); `guardrails/mod.rs` (+1 línea) | A |
| Carga única, `ctx.minimum`, `ask-unavailable` | `crates/core/src/guardrails/evaluate.rs` (`serve_audited`, `CommitContext`, `decision`) | A |
| Caller con la capacidad, cwd y cadena; filtro de `guard.log` | `crates/core/src/channel/conn.rs` (`guard_caller`, brazo de `guard.log`) | A |
| Cliente del hook: `pre-commit` con la capacidad, degradado y daemon sin capacidad | `crates/core/src/guardrails/hook.rs` (`commit`, `decide`, `degraded`) | A |
| Estado: `disabled-by-team`, `permission-reach` | `crates/core/src/guardrails/install.rs` (`status`); `crates/policy/src/guard/mod.rs` (`not_preventable`) | A |
| Mensajes | `apps/cli/src/guard.rs` (`reason_text`, `not_preventable_text`, texto del mínimo); `apps/cli/i18n/{en,es}/guard.txt` | A |
| Tests | § 6 | A y B |
| Documentación | Este archivo; la historia; enmiendas de ADR-GRD-002/003/004 (§ 9); `backlog.md`; `release-status.md` | A y B |

## 3. Decisión por operación

| Operación (BR-VAL-002) | Dónde se decide (Git directo) | Momento | Publicación |
|---|---|---|---|
| `commit` | `pre-commit` → `Commit{PreCommit}`; segunda línea en `reference-transaction` (`--no-verify`, `commit-tree` + `update-ref`, `cherry-pick`…) | A / A | Impedible |
| `push` | `pre-push`, toda línea con ref remota gobernada | A | Impedible (salvo solo tags: `permission-reach`) |
| `force-push` | `pre-push`, línea forzada (mismo hecho `FastForward` que el mínimo) | A | Impedible |
| `branch-delete` | Local: `reference-transaction` (valor nuevo cero en `refs/heads/*`, salvo *prune*). Remoto: `pre-push` (local cero), junto con `push` | A | Impedible |
| `rebase` | `pre-rebase` (también `pull --rebase`); segunda línea en `reference-transaction` | A / B | Impedible |
| `merge` | Segunda línea en `reference-transaction` (cadena con `merge`/`pull`, o commit nuevo con 2 o más padres) | B | **No impedible** (D8): protege la rama |
| `worktree-add` con rama nueva | `reference-transaction` de la creación de la rama, cadena con `worktree add` | A | Impedible |
| `worktree-add` sin rama nueva, `worktree-remove`, `reset-hard` | No se evalúan (D9) | C | No impedible → US-GRD-016 |

NFR-01 ante una denegación en `prepared` (como DS-US-GRD-008 § 3): el commit queda como objeto inalcanzable, `COMMIT_EDITMSG` conserva el mensaje, y la rama y los cambios del working tree no se pierden.

### 3.1 Tabla del clasificador (`guard::permissions::classify`)

Entrada: la línea (`Delete`, `Create`, `Update` de `refs/heads/*`), la cadena de `git` (`Certain(subcomando, acción)` o `Unknown`) y los commits nuevos (`commits`, `merges`, `unverifiable`, solo si se pidieron). Salida: las operaciones del catálogo y, si hacía falta un hecho que no se pudo probar, `unverifiable`.

| Línea | Cadena (del más cercano hacia fuera) | Commits nuevos | Operaciones |
|---|---|---|---|
| `Delete` (no *prune*) | cualquiera | — | `branch-delete` |
| `Create` | alguno es `worktree add` | — | `worktree-add`, y además la fila que toque por sus commits frescos |
| `Create` / `Update` | alguno es `rebase` | — | `rebase` (sus commits frescos son del rebase) |
| `Create` / `Update` | alguno es `merge` o `pull` | ≥ 1 | `merge`; y `commit` si hay commits frescos **además** del commit de fusión que crea el propio merge (la punta con 2 o más padres) |
| `Create` / `Update` | alguno es `fetch` o `clone` | — | ninguna (los commits vienen del remoto) |
| `Create` / `Update` | cualquier otra, también `branch`, `reset`, `switch`, `checkout`, `update-ref` y `Unknown` | ≥ 1 | `commit`, y `merge` si `merges ≥ 1` |
| `Create` | `Unknown` y `worktree-add` no es `allow` | — | `unverifiable` |
| `Create` / `Update` | `Unknown` y `rebase` o `merge` no es `allow`, con commits frescos | — | `unverifiable` (no se sabe si los crea un rebase o un merge) |

"Commits frescos" = los que no alcanza ni el valor viejo de la ref ni otra rama local o remota conocida (DS-US-GRD-008 D5), con los topes de `PathLimits`. Antes de **denegar** por `commit`, se confirman con la segunda pasada (`Hide::OtherBranches`), salvo que la cadena sea `commit`, `cherry-pick`, `revert` o `am`: esos crean el commit, y la primera pasada basta. Precedencia en la cadena: `worktree add` > `rebase` > `merge`/`pull` > `fetch`/`clone` > el resto. ⚠️ **ASSUMPTION** (se verifica en T001 con Git 2.38 a 2.56): `worktree add -b` crea la rama con un `git branch` hijo, y `pull` lanza un `git merge` o un `git rebase` hijo; la cadena de hasta 4 `git` los cubre. **Se deniega de más**, declarado: con `commit` en `deny`, llevar a una rama un commit que solo alcanza un tag (los tags no esconden nada, DS-US-GRD-008 D5).

## 4. Configuración

No hay claves nuevas: `permissions.{allow, ask, deny}` y `disableSafeMinimum` ya existen (`crates/policy/src/settings/model.rs`, ADR-GRP-007) con sus niveles. Ejemplo del esquema de la historia:

```json
{ "permissions": { "deny": ["commit", "push", "rebase"], "ask": ["branch-delete"] } }
```

La documentación del esquema añade qué cuenta como cada operación (D7), que `merge` solo protege la rama (D8) y qué no se evalúa con Git directo (D9).

## 5. Contrato (`crates/api`)

- `Rule::{PermissionCommit, PermissionPush, PermissionForcePush, PermissionResetHard, PermissionBranchDelete, PermissionRebase, PermissionMerge, PermissionWorktreeAdd, PermissionWorktreeRemove}` = `permission.<operación>`, con los nombres de `settings::model::Operation`.
- `Cause::AskUnavailable` = `ask-unavailable`. Se reutiliza `Cause::Unverifiable`.
- `NotPreventable::PermissionReach` = `permission-reach`.
- Capacidad `guard.permissions` en `crates/api/src/methods/guard.rs` (`CAP_GUARD_PERMISSIONS`, en `GROUP.capabilities`).
- `Operation::Commit`: el comentario pasa a "se envía cuando el daemon concede `guard.authorship` o `guard.permissions`". Sin campos nuevos.

Mensajes (`guard.txt`, plantillas fijas; los parámetros van etiquetados y saneados; nunca dicen cómo cambiar el permiso):

| Clave | es |
|---|---|
| `guard.reason.permission-deny` | GitRaptor: en este repo {operation} está denegado ({level}). La operación no se ejecuta. |
| `guard.reason.permission-ask` | GitRaptor: en este repo {operation} requiere confirmación humana ({level}) y la confirmación aún no está disponible. La operación no se ejecuta. |
| `guard.reason.permission-merge` | GitRaptor: en este repo merge está denegado ({level}): la rama {branch} no se movió. Si el working tree quedó con la fusión, `git merge --abort` (o `git reset --merge` tras un fast-forward) lo devuelve a su estado. |
| `guard.reason.permission-unverifiable` | GitRaptor: en este repo {operation} no está permitido ({level}) y no se pudo comprobar qué operación es esta. No se ejecuta. |
| `guard.op.<operación>` | commit · push · force-push · reset --hard · borrar una rama · rebase · merge · crear un worktree · borrar un worktree |
| `guard.log.rule.permission` | permiso de {operation} |
| `guard.np.permission-reach` | los permisos por operación no se aplican con Git directo a reset --hard, a borrar un worktree, a crear un worktree sin rama nueva ni a un push solo a tags; merge solo se frena en la rama. Un commit con --no-verify en HEAD separado no se evalúa al hacerlo, sino cuando llega a una rama, salvo si se sube directamente con push o se usa como base de un rebase |
| `guard.status.minimum-disabled` | mínimo seguro: desactivado por la configuración del equipo que confirmaste en esta máquina. |

Con `ask` en lugar de `deny` se usa `permission-ask` (o `permission-merge`, para merge). El inglés va en `apps/cli/i18n/en/guard.txt` con las mismas claves.

## 6. Criterios de aceptación verificables

Repos, remotos, perfiles y daemons temporales (NFR-01). Git, `raptor` y `raptor-hook` reales. El agente es `raptor-fake-agent` (patrón de `apps/cli/tests/guard_us_grd_008.rs`), y los permisos aplican igual a la persona (Q-GRD-1), así que cada fila se prueba con los dos. Suite nueva `apps/cli/tests/guard_us_grd_007.rs` (se niega a correr sin *debug assertions*). Los commits del agente llevan el trailer de `agents-commit`, para que solo los permisos puedan denegar.

| Escenario / criterio | Test | PR |
|---|---|---|
| 1 · `commit` en `deny`: no se ejecuta, también con `--no-verify` y con `commit-tree` + `update-ref`; el motivo nombra el permiso y "configuración del equipo" | `deny_commit_blocks_commit_and_its_second_line` | A |
| 1 · `push` en `deny` | `deny_push_blocks_push` | A |
| 1 · `force-push` en `deny`: el motivo nombra el permiso (y el mínimo, si está activo) | `deny_force_push_names_the_permission_and_the_team` | A |
| 1 · `branch-delete` en `deny`: local (`branch -D`, `update-ref -d`, `branch -m`) y remoto | `deny_branch_delete_blocks_local_and_remote` | A |
| 1 · `rebase` en `deny`: `rebase`, `pull --rebase` y `rebase --no-verify` (la rama no se mueve) | `deny_rebase_blocks_rebase_and_pull_rebase` | A |
| 1 · `worktree-add` en `deny`: `worktree add -b` no crea nada; sin rama nueva, pasa (declarado) | `deny_worktree_add_blocks_a_worktree_with_a_new_branch` | A |
| 1 · `merge` en `deny`: la rama no se mueve y el estado es recuperable (D8); la fila del esquema va a US-GRD-016 | `merge_deny_keeps_the_branch_and_leaves_a_recoverable_tree` | A |
| D9 · `commit` en `deny` y `merge` en `allow`: un commit `--no-verify` con `HEAD` separado no llega a `feat-x` por `merge --ff-only`, `branch -f`, `reset --soft` ni `switch -C` (motivo `permission.commit`) | `a_detached_commit_cannot_reach_a_branch_unevaluated` | A |
| D9 · Con `commit` en `deny`, un fast-forward a una rama local que ya existe y un `pull` de commits del remoto pasan (sin falsos positivos) | `a_fast_forward_of_existing_commits_is_not_a_commit` | A |
| 2 · `push` en `allow` sin políticas: se ejecuta | `allow_lets_push_through` | A |
| 3 · `branch-delete` en `ask`: no se ejecuta y el motivo dice "requiere confirmación humana" | `ask_is_denied_as_requiring_human_confirmation` | A |
| 6 · Endurecer en `feat-x` sin commitear no cambia nada; tras commitear, el push se deniega sin reinstalar | `a_hardening_applies_when_committed_not_when_edited` | A |
| Cada denegación (también `ask`) está en `raptor guard log` con `permission.*`, su nivel y su causa, y cuenta en el KPI | `permission_denials_reach_the_decision_log` | A |
| Degradado: el `deny` del suelo y del worktree aplica; `allow` y `disableSafeMinimum` del suelo no; el mínimo sigue | `degraded_mode_applies_the_team_permissions_only_to_harden` | A |
| 4 · Instalar y confirmar después una configuración que desactiva el mínimo y permite force-push: anuncio, ventana, y al cerrarse el force-push de `feat-x` se ejecuta; el estado dice `disabled-by-team` | `confirming_a_floor_that_disables_the_minimum_lets_force_push_through` | B |
| 5 · Llega a la rama principal un cambio que permite force-push: sigue denegado, el estado muestra `floor-relax-pending` con la acción, un agente no confirma (rechazado y registrado) y tras confirmar el force-push se ejecuta | `a_team_relaxation_waits_for_the_developer` | B |

Además (`crates/core/tests/us_grd_007.rs` y unitarias):

| Comportamiento | Test |
|---|---|
| `disableSafeMinimum` solo desde el suelo confirmado y legible; sin confirmar, en el worktree o en un nivel personal, no apaga nada | `us_grd_007::disable_safe_minimum_only_from_the_confirmed_readable_floor` |
| `minimum_set` es `disabled-by-team` con un `Confirmed` que lo desactiva; `active` en otro caso | `us_grd_007::minimum_set_status_follows_the_confirmed_floor` |
| Una razón por nivel del máximo (suelo + perfil; `SafeMinimum` sin razón de permiso) | `us_grd_007::reasons_name_every_level_of_the_maximum` |
| Capacidad en las dos direcciones; sin ella, `guard.log` quita las entradas `permission.*` y las sigue contando | `us_grd_007::permissions_capability_both_ways` |
| Clasificación no demostrable → `unverifiable` solo si el permiso no es `allow` | `us_grd_007::an_unclassifiable_transaction_is_denied_only_when_it_matters` |
| Coste de D14 (2 000 ramas, `merge` en `deny`, commit corriente) | `us_grd_007::permissions_cost` (release, `#[ignore]` fuera de la medición) |
| Tabla del § 3.1, precedencia y lista de exclusión | `crates/policy` · `guard::permissions::tests::*` |
| Línea de órdenes: subcomando y acción seguros, opciones globales, alias, no UTF-8 | `crates/policy` · `guard::git_command::tests::*`; los de `authorship::subcommand` siguen verdes |
| Permisos en la función pura: cada operación × `allow`/`ask`/`deny`, `ctx.minimum` apagado, todas las razones juntas | `crates/policy` · `guard::tests::permissions_in_the_function::*` |
| Commits nuevos: commit, fusión, amend, fast-forward a un commit existente, creación, topes | `crates/git/tests/fresh_commits.rs` |

## 7. Orden de implementación

Ver § Plan de implementación: PR-A = T001 a T009; PR-B = T010 y T011, después de US-GRD-014 en `main`.

## 8. Pendientes, límites y fuera de alcance

**Lo que no se puede impedir** (lista publicada, `NotPreventable`, textos `guard.np.*`): `reset-hard`, `merge` (solo se protege la rama, D8), `remove-worktree`, `create-worktree` sin rama nueva y, nuevo, `permission-reach` (D9). Los saltos voluntarios no cambian (`voluntary-skips`): `-c core.hooksPath`, `GIT_CONFIG_*`, `send-pack` y editar refs a mano saltan también los permisos.

**Otros residuos declarados**: sin el binario `raptor` rige el *fallback* (D13); un commit que solo alcanza un tag cuenta como fresco (§ 3.1); un commit separado puede salir con `push` (rige `push`) o entrar como base de un `rebase` (D9); con `commit` distinto de `allow`, un fast-forward de commits de otra rama paga la segunda pasada (D14); en degradado no hay niveles personales; los commits que trae un `fetch` a una ref local (`fetch origin main:main`) no cuentan como `commit`.

| Pendiente | Dueño |
|---|---|
| Las mismas decisiones por MCP: `reset --hard`, merge, borrar worktree, crear worktree sin rama | US-GRD-016 (BR-CONS-002) |
| Cola de confirmación real (`ask` que espera, BR-WF-001) | US-GRD-015 |
| Comando de confirmación, ventana, diagnóstico de relajación pendiente | US-GRD-014 (D15) |
| `configStatus` real y aviso de configuración ilegible | US-GRD-011 |
| Push solo a tags | [TD-GRD-001](../technical-stories/TD-GRD-001-dispatcher-plantilla-3-pre-push-toda-ref.md) |
| Linux y Windows reales | **Pendiente: etapa de validación multiplataforma** (Linux lo cubre el CI de ubuntu) |

## 9. Validación de las decisiones

Las decisiones técnicas (D1, D2, D4 a D6 y D10 a D18) son **Decisión del orquestador (2026-10-09), validada por el Arquitecto** (`nassa-architect:architect`).

Las de producto son **Decisión del orquestador (2026-10-09), validada por Arquitecto y PO** (`nassa-aadd:product-owner`), registradas en `context.md` y en BR-VAL-002 y BR-EDGE-003:

| Decisión | Q-GRD | Nota |
|---|---|---|
| D3 · `ask` cuenta en el KPI como denegación verificada | Q-GRD-36 | Pendiente de que Rene lo confirme en el PR |
| D7 · Qué cuenta como cada operación | Q-GRD-37 | — |
| D8 · Merge con Git directo protege solo la rama; la fila "merge" pasa a US-GRD-016 | Q-GRD-38 | — |
| D9 · Alcance de los permisos con Git directo (`permission-reach`) | Q-GRD-39 | Precisión del PO: el commit separado que llega a una rama por fast-forward, `branch -f`, `reset` o `switch -C` se **cierra** (commits frescos → `commit`). Quedan declarados `push` directo y base de `rebase` |

**Enmiendas propuestas** (las aplica el orquestador; esta Dev Spec no edita ADRs):

| ADR | Enmienda |
|---|---|
| ADR-GRD-003 § 3 | `ask-unavailable` viaja como **causa** de las razones que producen `ask`, no como razón `system` (D3). Reglas `permission.<operación>` (D1) |
| ADR-GRD-003 § 1 | La **identidad de la operación** (cadena de `git`) clasifica la operación del catálogo en `reference-transaction`; el actor sigue sin entrar en los permisos (D6) |
| ADR-GRD-003 § 4 | Degradado: aplica los permisos de suelo y worktree solo para endurecer, con el clasificador sobre la ascendencia del propio cliente (D12). La garantía no cambia |
| ADR-GRD-002 § 1 | Fila Merge: la segunda línea deniega el movimiento de la rama con `merge` en `deny`/`ask` (B, sigue no impedible). Fila Rebase: la segunda línea evalúa `rebase`. Fila Crear worktree con rama nueva: se reconoce por la cadena (`worktree add`). Fila Push: un push solo a refs no gobernadas no evalúa `push` (D9) |

**Preguntas abiertas**: **para Rene, en el PR**: confirmar Q-GRD-36 (`ask` cuenta en el KPI mientras no exista la cola).

## Anexo de forma (perfil `backend-service`)

### 6.1 Tipos compartidos

Los del § 5 en `crates/api`. En `crates/policy`: `guard::permissions::{Classified, LineKind, Chain, Fresh, classify, apply}` y `guard::git_command::{GitCommand, parse}`; `team::{EffectivePermissions, OperationRule, RuleSource}` sin cambios. En `crates/git`: `RepoReader::fresh_commits` y `NewCommits { commits, merges, unverifiable }`. En `crates/core`: `guardrails::permissions::{git_chain, classified, team_only}`.

### 6.2 Ciclos de vida (DI)

_No aplica — no hay contenedor de dependencias: el clasificador y la regla son funciones puras, la configuración sale del `LOADER` estático de `crates/core/src/guardrails/layers.rs` (caché acotada) y la cadena de `git` se lee por evaluación._

### 6.3 Firmas del stack

`policy::guard::evaluate(&Operation, &Facts, &Context) -> Evaluation` sin cambiar la firma: `Context` gana `minimum: bool` y `permissions: Option<EffectivePermissions>` (`None` = sin la capacidad), y `Facts` gana `classified: Vec<Classified>`, alineado con las líneas. `permissions::classify(line: LineKind, chain: &Chain, fresh: Option<&Fresh>, needs: &EffectivePermissions) -> Classified`. `RepoReader::fresh_commits(&self, old: Option<&str>, new: &str, updated: &[&str], hide: Hide, limits: &PathLimits) -> Result<NewCommits, ReadError>`. `guardrails::permissions::git_chain(pid: u32, checks: &Checks) -> Chain`.

### 7.1 Forma del error

Una denegación es un `Decision` con `reasons[]` (`permission.*`, causa opcional `ask-unavailable` o `unverifiable`). No hay códigos de error JSON-RPC nuevos. El hook sale con 1 y escribe la plantilla fija por stderr (M-05).

### 7.2 Forma de la configuración

§ 4: `permissions.{allow, ask, deny}: [Operation]` y `permissions.disableSafeMinimum: bool` (solo equipo), ya generadas en `crates/policy/schema/settings.schema.json`. Solo cambia la documentación del esquema.

### 7.3 Valores numéricos

Cadena de `git`: hasta 4 antecesores consecutivos y 16 procesos recorridos (`second_line::MAX_DEPTH`). Commits nuevos: los topes de `PathLimits` de DS-US-GRD-008 (256 por línea, 100 000 visitados, 4 096 puntas). Presupuesto: < 100 ms p95 por evaluación gobernada.

### 8. Modelo de datos

_No aplica — no se crea almacén ni columna: el registro guarda las razones sin parámetros, y la confirmación (almacén por repo) es de US-GRD-014._

### 9. Estrategia de pruebas

§ 6. E2e `apps/cli/tests/guard_us_grd_007.rs` con Git, `raptor` y `raptor-hook` reales; `crates/core/tests/us_grd_007.rs` para el mínimo desactivado, la capacidad y el coste; unitarias del clasificador y de la regla en `crates/policy`; `crates/git/tests/fresh_commits.rs`.

## Gaps y violaciones de la constitución

Ningún hueco bloquea la implementación de PR-A. Sin constitución en la cascada: se aplican `AGENTS.md` y los ADRs citados. Las decisiones de producto pendientes del PO (§ 9) no impiden implementar.

| Id | Hueco | Severidad | Alcance |
|---|---|---|---|
| G1 | Los escenarios 4 y 5 necesitan `raptor guard confirm` de US-GRD-014, que aún no está en `main` | Bloquea liberación | T010, T011 |

## Plan de implementación

> Orden topológico (`Depende:`). Rutas relativas a la raíz del repo. T001 son los tests en rojo.

| # | Tarea | Depende | Aterriza en |
|---|---|---|---|
| T001 | Suite e2e de PR-A en rojo | — | `apps/cli/tests` |
| T002 | Clasificador de la línea de órdenes de `git` | — | `crates/policy/src/guard`, `crates/policy/src/authorship` |
| T003 | Clasificar y aplicar permisos; `ctx.minimum` | T002 | `crates/policy/src/guard` |
| T004 | Commits nuevos de un movimiento | — | `crates/git` |
| T005 | Contrato y capacidad `guard.permissions` | — | `crates/api` |
| T006 | Carga única, cadena, hechos, `ask-unavailable` | T003, T004, T005 | `crates/core/src/guardrails`, `crates/core/src/channel` |
| T007 | Cliente del hook: `pre-commit`, degradado, daemon sin capacidad | T006 | `crates/core/src/guardrails/hook.rs` |
| T008 | Estado, mensajes y lista "no se puede impedir" | T005, T006 | `crates/core/src/guardrails/install.rs`, `apps/cli` |
| T009 | Verificar PR-A, medir y documentar | T001, T007, T008 | `docs`, `crates/core/tests` |
| T010 | Suite e2e de PR-B en rojo | T009, US-GRD-014 | `apps/cli/tests` |
| T011 | Cerrar PR-B | T010 | `docs` |

### T001 — Suite e2e de PR-A en rojo

**Objetivo.** `apps/cli/tests/guard_us_grd_007.rs` con las filas de PR-A del § 6; fallan antes de implementar y no dependen de una API que aún no existe.

**Ubicación.**
- `apps/cli/tests/guard_us_grd_007.rs` (**CREATE**)

**Reglas**
- Repos y perfiles temporales (NFR-01), sin esperas fijas; cada fila con el agente y con la persona; la configuración del equipo se commitea en la rama principal sin confirmar (un `deny` ya endurece, Q-GRD-23).
- Verificar el ⚠️ ASSUMPTION del § 3.1 (cadena de `worktree add -b` y `pull`) en la versión de Git del runner y anotarlo.

- **Depende:** —
- **Refs:** US-GRD-007; DS-US-GRD-008 § 6
- **Aceptación:** `cargo test -p gitraptor-cli --test guard_us_grd_007 deny_push_blocks_push`

### T002 — Clasificador de la línea de órdenes de `git`

**Objetivo.** `guard/git_command.rs`: subcomando y acción **seguros** o `Unknown`; `authorship::subcommand::second_line_evaluates` pasa a usarlo sin cambiar su resultado.

**Ubicación.**
- `crates/policy/src/guard/git_command.rs` (**CREATE**)
- `crates/policy/src/authorship/subcommand.rs`, `crates/policy/src/guard/mod.rs` (**MODIFY**)

**Reglas**
- Puro, sin E/S; un alias o una opción global desconocida es `Unknown`; los tests de `authorship::subcommand` no se tocan y siguen verdes.

- **Depende:** —
- **Refs:** D6; DS-US-GRD-018 § 5.3
- **Aceptación:** `cargo test -p gitraptor-policy git_command:: subcommand::`

### T003 — Clasificar y aplicar permisos; `ctx.minimum`

**Objetivo.** `guard/permissions.rs` con la tabla del § 3.1 y la regla de D1 a D4; `evaluate` aplica permisos en `Push`, `RefTransaction`, `Rebase` y `Commit{PreCommit}`, y el bloque del mínimo cuelga de `ctx.minimum`.

**Ubicación.**
- `crates/policy/src/guard/permissions.rs` (**CREATE**)
- `crates/policy/src/guard/mod.rs`, `crates/policy/src/guard/tests.rs` (**MODIFY**)

**Reglas**
- Sin E/S ni reloj (ADR-GRD-003 § 1); `permissions = None` no cambia nada respecto a hoy; una razón por nivel del máximo; `SafeMinimum` sin razón de permiso; `Commit{CommitMsg}` y `Commit{SecondLine}` no evalúan permisos.

- **Depende:** T002
- **Refs:** D1–D7
- **Aceptación:** `cargo test -p gitraptor-policy guard::`

### T004 — Commits nuevos de un movimiento

**Objetivo.** `RepoReader::fresh_commits`: cuenta de commits nuevos y de fusiones con las dos pasadas y los topes de `fresh_commit_paths`, sin leer árboles.

**Ubicación.**
- `crates/git/src/guard_paths.rs` (**MODIFY**)
- `crates/git/tests/fresh_commits.rs` (**CREATE**)

**Reglas**
- Comparte el recorrido con `fresh_commit_paths` (sin duplicarlo); fail-closed ante cualquier tope; sin objetos de reemplazo ni commit-graph.

- **Depende:** —
- **Refs:** D6, D14; DS-US-GRD-008 D5
- **Aceptación:** `cargo test -p gitraptor-git --test fresh_commits`

### T005 — Contrato y capacidad `guard.permissions`

**Objetivo.** Reglas, causa, `NotPreventable::PermissionReach` y la capacidad (§ 5).

**Ubicación.**
- `crates/api/src/guard.rs`, `crates/api/src/methods/guard.rs` (**MODIFY**)

**Reglas**
- La capacidad se declara solo en `methods/guard.rs`; `crates/api/tests/architecture.rs` sigue verde; los códigos en kebab-case con un test de ida y vuelta.

- **Depende:** —
- **Refs:** D1, D3, D9, D10
- **Aceptación:** `cargo test -p gitraptor-api`

### T006 — Carga única, cadena, hechos, `ask-unavailable`

**Objetivo.** Con la capacidad, `serve_audited` carga las capas para toda operación gobernada, resuelve permisos y `ctx.minimum`, lee la cadena y los commits nuevos solo cuando hacen falta, y `decision()` pone `ask-unavailable`. `guard_caller` resuelve cwd y cadena; `guard.log` filtra para conexiones sin la capacidad.

**Ubicación.**
- `crates/core/src/guardrails/permissions.rs` (**CREATE**)
- `crates/core/src/guardrails/evaluate.rs`, `crates/core/src/guardrails/mod.rs` (**MODIFY**)
- `crates/core/src/channel/conn.rs` (**MODIFY**)
- `crates/core/tests/us_grd_007.rs` (**CREATE**)

**Reglas**
- Una sola `layers::load` por evaluación, compartida con políticas y autoría (D11; si US-GRD-014 ya la hizo, se reutiliza); la línea de órdenes nunca se guarda ni se registra; sin la capacidad, la decisión es idéntica a la de hoy.

- **Depende:** T003, T004, T005
- **Refs:** D3, D4, D6, D10, D11, D14
- **Aceptación:** `cargo test -p gitraptor-core --test us_grd_007`

### T007 — Cliente del hook: `pre-commit`, degradado, daemon sin capacidad

**Objetivo.** `commit()` envía `Commit{PreCommit}` si el daemon concede `guard.authorship` o `guard.permissions`; en degradado o sin la capacidad aplica `permissions::team_only` sobre el worktree del cwd con la cadena de su propia ascendencia.

**Ubicación.**
- `crates/core/src/guardrails/hook.rs` (**MODIFY**)

**Reglas**
- Nunca menos que el degradado (D10, D12); en degradado el mínimo está forzado y no hay niveles personales; la segunda línea de autoría no cambia.

- **Depende:** T006
- **Refs:** D5, D10, D12
- **Aceptación:** `cargo test -p gitraptor-cli --test guard_us_grd_007 degraded_mode_applies_the_team_permissions_only_to_harden`

### T008 — Estado, mensajes y lista "no se puede impedir"

**Objetivo.** `minimum_set` `disabled-by-team`, `permission-reach`, plantillas en/es del § 5 y brazos de `reason_text`.

**Ubicación.**
- `crates/core/src/guardrails/install.rs`, `crates/policy/src/guard/mod.rs` (**MODIFY**)
- `apps/cli/src/guard.rs`, `apps/cli/i18n/en/guard.txt`, `apps/cli/i18n/es/guard.txt` (**MODIFY**)

**Reglas**
- Plantillas fijas, parámetros etiquetados y saneados; ningún texto dice cómo cambiar un permiso ni cómo desactivar el mínimo; los `match` exhaustivos de `apps/cli/src/guard.rs` cubren las reglas nuevas.

- **Depende:** T005, T006
- **Refs:** D2, D3, D8, D9, D17, D18
- **Aceptación:** `cargo test -p gitraptor-cli --test guard_us_grd_007`

### T009 — Verificar PR-A, medir y documentar

**Objetivo.** Suite completa en verde, medición de D14 en § 10, estado de la historia, `backlog.md` y `release-status.md`.

**Ubicación.**
- `docs/requirements/features/guardrails/user-stories/US-GRD-007-permisos-por-operacion.md`, `docs/requirements/backlog.md`, `docs/requirements/release-status.md` (**MODIFY**)
- Este archivo (**MODIFY**, § 10)

**Reglas**
- No se declara hecho lo que no se verificó: escenarios 4 y 5 quedan pendientes de PR-B; Windows y Linux, pendientes; `release-status.md` se regenera con `node tools/status/release-status.mjs`.

- **Depende:** T001, T007, T008
- **Refs:** D14, D16
- **Aceptación:** `cargo test --workspace`

### T010 — Suite e2e de PR-B en rojo

**Objetivo.** Escenarios 4 y 5 con `raptor guard confirm` de US-GRD-014: anuncio, ventana (acortada con `GITRAPTOR_TEST_GUARD_WINDOW_MS`), rechazo al agente y force-push tras confirmar.

**Ubicación.**
- `apps/cli/tests/guard_us_grd_007.rs` (**MODIFY**)

**Reglas**
- Solo con US-GRD-014 en `main`; no redefinir nada de su contrato (D15); el rechazo al agente se comprueba en el registro de auditoría de 014.

- **Depende:** T009, US-GRD-014
- **Refs:** D15, D16; `DS-US-GRD-014` D1–D3, D8, D10
- **Aceptación:** `cargo test -p gitraptor-cli --test guard_us_grd_007 a_team_relaxation_waits_for_the_developer`

### T011 — Cerrar PR-B

**Objetivo.** Escenarios 4 y 5 en verde, historia completa, enmiendas de § 9 aplicadas por el orquestador.

**Ubicación.**
- `docs/requirements/features/guardrails/user-stories/US-GRD-007-permisos-por-operacion.md`, `docs/requirements/release-status.md` (**MODIFY**)

**Reglas**
- La historia pasa a `implemented` solo con los seis escenarios verificados (la fila de merge del esquema la cierra US-GRD-016).

- **Depende:** T010
- **Refs:** D16
- **Aceptación:** `cargo test -p gitraptor-cli --test guard_us_grd_007`

## 10. Estado de la implementación

_Pendiente: se rellena al cerrar PR-A (medición de D14) y PR-B._
