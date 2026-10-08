---
id: DS-US-GRD-008
title: "Dev Spec — US-GRD-008: ramas protegidas y rutas prohibidas por actor"
type: dev-spec
status: draft
created: 2026-10-08
updated: 2026-10-08
story: US-GRD-008
feature: guardrails
domain: GRP
scope: backend
frontend_surface: false
stack: rust
profile: backend-service
tooling: [cargo]
related:
  context: ../context.md
  story: ../user-stories/US-GRD-008-ramas-protegidas-rutas-prohibidas.md
  adrs: [ADR-GRD-001, ADR-GRD-002, ADR-GRD-003, ADR-GRD-004, ADR-GRD-006, ADR-GRD-007, ADR-GRP-007, ADR-GRP-016]
  rules: [BR-VAL-003, BR-CALC-001, BR-CONS-001, BR-EDGE-004]
  api_spec: null
  design_spec: null
  contracts: []
must_read:
  - ../user-stories/US-GRD-008-ramas-protegidas-rutas-prohibidas.md
  - ../../../../architecture/decisions/ADR-GRD-002-operaciones-interceptables.md
  - ../../../../architecture/decisions/ADR-GRD-003-motor-decision-contrato.md
  - ../../../../architecture/decisions/ADR-GRD-004-configuracion-efectiva.md
  - ../../../../architecture/extender-sin-archivos-compartidos.md
  - ./US-GRD-018-autoria-commits-persona-y-agente.md
  - ../../../../../crates/policy/src/guard/mod.rs
  - ../../../../../crates/policy/src/guard/authorship.rs
  - ../../../../../crates/core/src/guardrails/evaluate.rs
  - ../../../../../crates/core/src/guardrails/authorship.rs
  - ../../../../../crates/git/src/guard_read.rs
  - ../../../../../crates/core/src/channel/conn.rs
  - ../../../../../crates/core/src/guardrails/hook.rs
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
  gaps_release: 0
  ready_to_release: false
tags: [guardrails, ramas-protegidas, rutas-prohibidas, br-val-003, actor, politicas]
---

# Dev Spec — US-GRD-008: ramas protegidas y rutas prohibidas por actor

Plano compacto (AADD ligero) de [US-GRD-008](../user-stories/US-GRD-008-ramas-protegidas-rutas-prohibidas.md): la configuración del equipo declara **ramas protegidas** (patrones) que un agente no puede mover y **rutas prohibidas** (patrones) que un agente no puede commitear. La regla es [BR-VAL-003](../business-rules.md) (filas "Rama protegida" y "Ruta prohibida") con [BR-CALC-001](../business-rules.md) (se nombran todas las reglas incumplidas). Sigue el patrón de [DS-US-GRD-018](./US-GRD-018-autoria-commits-persona-y-agente.md): regla pura en `crates/policy`, la configuración se lee en el daemon, el actor lo resuelve el daemon y nada nuevo viaja del cliente del hook.

**Invariante**: no relaja el mínimo seguro ni ninguna otra regla; solo añade denegaciones. GitRaptor nunca mueve ni borra nada para "arreglar" un bloqueo.

## 1. Decisiones

Cada fila es una **Decisión del orquestador (2026-10-08), validada por Arquitecto y PO** (ver § 9).

| # | Decisión |
|---|---|
| D1 | **Claves** `policies.protectedBranches` y `policies.forbiddenPaths`, las dos con la misma forma: `{ "patterns": ["…"], "appliesTo": "agents" \| "everyone" }`. `appliesTo` por defecto `agents`. Niveles admitidos (`x-gitraptor-levels`): equipo (suelo y worktree), perfil y local, como `commitAuthorship`. Dejan de producir `policy-not-supported`. Límites: 64 patrones por clave, 256 bytes por patrón. Un patrón vacío, con bytes de control, que empieza por `!` o `#` (negación y comentario no existen) o que empieza por `refs/heads/` es **inválido**: se descarta **solo ese patrón**, los demás se aplican y la fuente queda `parcial` (diagnóstico `policy-invalid`), lo que ya fuerza el mínimo seguro (D12 de ADR-GRD-004). Una clave con el tipo equivocado no se aplica, con el mismo diagnóstico. `[` y `\` son literales. El estado `parcial` viaja en `configStatus` de cada decisión, porque la vista en `status` queda diferida |
| D2 | **Combinación** (BR-CONS-001): solo endurecen. El valor efectivo es la **unión** de las reglas de todas las fuentes legibles (suelo, worktree, perfil); cada regla conserva el nivel que la declara y su `appliesTo`. Ninguna fuente puede quitar el patrón de otra. No existe forma de relajar. Si dos niveles declaran el mismo patrón con distinto `appliesTo`, se aplican las dos reglas y gana la más estricta (`everyone`) |
| D3 | **El actor entra en la condición** (enmienda de ADR-GRD-003 § 1, igual que US-GRD-018 D4). Con `appliesTo: agents`, la regla deniega solo si el actor es un agente (detectado o registrado, resuelto por el daemon, D5 de US-GRD-018); con "sin atribuir" (la persona en su terminal) pasa. Con `appliesTo: everyone` deniega a todos. Así "la persona sí, salvo que la política diga otra cosa". Un agente que el daemon no detecta ni tiene registrado cuenta como persona: residuo declarado (§ 8, `policy-actor`). La regla de negocio que lo permite (excepción consciente al "toda operación, sea cual sea el actor" de Q-GRD-1, como hizo BR-AUTH-005 con la autoría) la registra el PO como Q-GRD-35 en `context.md` y en BR-VAL-003 (§ 9) |
| D4 | **Rama protegida = cualquier movimiento de `refs/heads/<patrón>`**, por un agente: crear, actualizar (commit, merge, `branch -f`, `update-ref`, `reset`) y borrar. Se decide en `reference-transaction prepared` (cubre `commit` con y sin `--no-verify`, `merge`, `branch -d/-f`, `update-ref`) y en `pre-push` (la ref remota `refs/heads/<patrón>`: push, force-push y borrado remoto). No hay dispatcher nuevo ni etapa nueva: son las dos operaciones que la capa ya evalúa. La regla de rama protegida se evalúa en **una pasada propia** sobre todas las líneas (no cuelga de los `return` tempranos del mínimo). Los nombres se comparan **siempre** con NFC y minúsculas, aunque el sistema de archivos local distinga: el del remoto es desconocido y un alias solo da falsos positivos entre ramas que difieren en mayúsculas |
| D5 | **Ruta prohibida = un commit nuevo que modifica, crea o borra la ruta**. El **daemon** lee los commits nuevos del movimiento con el lector aislado (el cliente no envía rutas: al daemon no llega nada nuevo): los commits alcanzables desde `new` que ni `old` ni otra rama o rama remota alcanzan, hasta 256, y de cada uno su diferencia de árbol contra su primer padre (contra todos los padres en una fusión: una ruta cuenta solo si difiere de **todos**, así traer lo que ya existía no cuenta, pero una resolución de conflicto sí). Se evalúa en `reference-transaction prepared` (rango `old..new`, cubre commits, `--amend`, `--no-verify`, `commit-tree` + `update-ref` y varios commits) y en `pre-push` (rango: el commit local menos el remoto y las ramas remotas conocidas, cierra un commit hecho con `HEAD` separado). Solo se lee cuando hay una regla de rutas aplicable al actor. Si Git envía `old` en ceros (`update-ref <ref> <nuevo>` sin valor esperado, igual que en una creación), el daemon toma el valor actual de la ref, que en `prepared` todavía es el anterior. **Topes de trabajo**: 256 commits nuevos y 100 000 commits visitados por línea, 2 000 puntas de ramas, y un agregado de 4 096 commits nuevos y 100 000 rutas por evaluación. Pasar un tope, un árbol ilegible, objetos ausentes (clon parcial) o un repo superficial que no permite probar el rango no se pueden verificar: se deniega con causa `unverifiable` (fail-closed) **solo cuando hay una regla aplicable**. La asimetría es deliberada: en `pre-push` no se esconde nada por ramas locales (lo que sale se revisa entero) y solo las ramas remotas conocidas cuentan como "ya existía" |
| D6 | **Patrones** (un solo matcher, lineal en el tamaño del patrón por el de la ruta, con topes): `/` separa segmentos; `*` y `?` no cruzan `/`; `**` como segmento completo cruza segmentos. **Ramas**: el patrón completo contra el nombre corto (`release/*` no cubre `release/1.0/x`; `release/**` sí). **Rutas** (semántica de `.gitignore` reducida, sin negaciones): un patrón sin `/` salvo el final se busca a cualquier profundidad (`*.pem`, `secrets/`); con `/` en medio o al principio está anclado a la raíz (`config/prod.yml`, `/secrets`); `/` final = solo directorio; un patrón que cubre un directorio cubre todo lo que hay debajo. Se compara siempre normalizado (NFC y minúsculas): el contenido es portable entre sistemas de archivos. Los nombres se comparan como bytes y las rutas no UTF-8 se convierten con pérdida solo para el mensaje (saneado). `.github/workflows/` queda anclado a la raíz y no cubre `a/.github/workflows/` (use `**/.github/workflows/`) |
| D7 | **Todas las reglas incumplidas, juntas** (BR-CALC-001): `Evaluation::add` ya acumula las razones del efecto máximo. Una transacción que mueve una rama protegida con un commit que toca una ruta prohibida devuelve las dos razones en una sola decisión. Por patrón incumplido hay una razón, con la **primera** ruta que lo cumple (el máximo son los patrones declarados, 64 por clave y fuente: no hay truncado que ocultar) |
| D8 | **Razones y causas** (contrato, `crates/api/src/guard.rs`): reglas `policy.protected-branch` y `policy.forbidden-path`; causa `unverifiable` (D5); parámetros nuevos `pattern` y `path` (no confiables, como los demás). Los parámetros llevan la rama (`branch`) o la ruta y el patrón que se incumplió, para que el mensaje nombre la regla. Ningún código de error nuevo |
| D9 | **Capacidad `guard.policies`** (ADR-GRP-016): el daemon aplica estas reglas y resuelve el actor de `RefTransaction` y `Push` solo si la conexión la pidió. La puerta es de **compatibilidad**, no de seguridad: tras una actualización puede seguir vivo un daemon anterior, o haber otra instalación. Un cliente nuevo ante un daemon sin la capacidad no cambia nada, y un daemon nuevo ante un hook sin ella tampoco (tests en las dos direcciones). El cliente actual la pide en `connection.accept` |
| D10 | **Registro** (US-GRD-005, BR-CONS-004): toda denegación ya entra en el registro con sus reglas por `log_decision`; las reglas nuevas añaden sus textos `guard.log.rule.*` y las ramas implicadas viajan como `branch`. `raptor guard log` las muestra sin cambios de contrato |
| D11 | **Modo degradado** (ADR-GRD-003 § 4: nunca menos restrictivo que el mínimo más el suelo legible): el cliente lee el suelo con el mismo cargador y evalúa con el mismo código **las reglas `everyone` del suelo**; sin niveles personales ni actor, las reglas `agents` no se aplican (el actor es "sin atribuir"). Residuo declarado (§ 8, `policy-actor`): un agente puede forzar el modo degradado (borrar el perfil: `instance-mismatch`, o impedir que arranque el daemon). Aplicar las reglas `agents` a "sin atribuir" en degradado sería más seguro pero bloquearía a la persona mientras el daemon esté caído: queda como pregunta para Rene (PR). Se enmienda la garantía de ADR-GRD-003 § 4, que hoy solo lista como pérdida las reglas de autoría |
| D12 | **Coste**: el actor y la configuración se resuelven solo para un movimiento de `refs/heads/*` o un push, y la lectura de commits solo si hay una regla de rutas aplicable; sin reglas en la configuración el único coste es cargar la configuración, que ya está en la caché acotada del cargador (TS-GRD-001). Antes de cerrar la historia se mide una evaluación con 2 000 ramas y un rango de 256 commits, contra el presupuesto de ADR-GRD-002 § 5 (< 100 ms p95); el resultado va en § 10 |

## 2. Forma (archivos que se tocan)

| Pieza | Archivo |
|---|---|
| Claves y límites (`policies.protectedBranches`, `forbiddenPaths`), esquema | `crates/policy/src/settings/{model,document,diagnostic}.rs`, `crates/policy/schema/settings.schema.json` |
| Matcher de patrones (puro) | `crates/policy/src/guard/glob.rs` (nuevo) |
| Reglas y combinación (puro) | `crates/policy/src/guard/policies.rs` (nuevo); `guard/mod.rs` gana `Context.policies`, `Facts.touched` y dos llamadas |
| Lectura de los commits nuevos y sus rutas | `crates/git/src/guard_read.rs` (`new_commit_paths`) |
| Contrato: reglas, causa, parámetros, `NotPreventable`, capacidad | `crates/api/src/guard.rs`, `crates/api/src/methods/guard.rs` |
| Configuración efectiva de las políticas, actor para `RefTransaction`/`Push`, hechos | `crates/core/src/guardrails/policies.rs` (nuevo), `crates/core/src/guardrails/evaluate.rs`, `crates/core/src/channel/conn.rs` |
| Mensajes y lista "no se puede impedir" | `apps/cli/src/guard.rs`, `apps/cli/i18n/{en,es}/guard.txt` |
| Tests | § 6 |
| Documentación | Este archivo; ADR-GRD-003 (enmienda); ADR-GRD-002 (fila Commit/Push); la historia; backlog; `release-status.md` |

## 3. Decisión por operación

| Operación (Git) | Dónde se decide | Qué se evalúa |
|---|---|---|
| `git commit`, `--amend`, `--no-verify`, `merge` con commit, `commit-tree` + `update-ref` | `reference-transaction prepared` | rama protegida (`refs/heads/x`); rutas del rango nuevo |
| `git branch -d/-D/-f/-m`, `update-ref`, `reset` (mueven una rama) | `reference-transaction prepared` | rama protegida |
| `git push` (también `--force`, `--delete`) | `pre-push` | rama protegida remota; rutas del rango que sube |
| `git rebase`, `cherry-pick`, `revert`, `am` | `reference-transaction prepared` | sin clasificación especial: cada paso es un commit nuevo y se evalúa (una rama del agente que lleva un cambio de la persona en una ruta prohibida no se puede rebasar por el agente: fallo cerrado, § 8) |

NFR-01 frente a una denegación en `prepared` (como DS-US-GRD-018 S9): Git ya escribió el commit y, con `-a`, el índice nuevo; al fallar, el índice, el árbol de trabajo y la rama quedan como antes, el mensaje sigue en `COMMIT_EDITMSG` y el commit queda como objeto inalcanzable. Los cambios del working tree no se pierden.

## 4. Configuración

```json
{
  "policies": {
    "protectedBranches": { "patterns": ["main", "release/*"], "appliesTo": "agents" },
    "forbiddenPaths": { "patterns": ["secrets/", "*.pem", ".github/workflows/"] }
  }
}
```

Con `appliesTo: everyone` la persona tampoco puede mover la rama ni commitear la ruta con Git directo, y no hay excepción consciente en el MVP (la acción reservada de ADR-GRD-007 no está): el cambio de la propia política en la rama protegida también queda bloqueado, y **congela la rama local**: `git pull`, `git fetch origin main:main` y cualquier fast-forward son un movimiento de la rama. No se pueden eximir, porque las ramas remotas conocidas (`refs/remotes/*`) las puede escribir un agente. Lo declara la documentación del esquema. Con `agents`, que es lo habitual, la persona sigue flujos normales.

Casos que la documentación del esquema muestra: `release/*` no cubre `release/1.0/x` (sí `release/**`); `secrets/` aplica a cualquier profundidad; `/secrets` y `config/prod.yml` están anclados a la raíz; no hay negaciones.

## 5. Contrato (`crates/api/src/guard.rs`)

- `Rule::ProtectedBranch` = `policy.protected-branch`; `Rule::ForbiddenPath` = `policy.forbidden-path`.
- `Cause::Unverifiable` = `unverifiable`.
- `ParamKind::Pattern`, `ParamKind::Path`.
- `NotPreventable::PolicyActor` y `NotPreventable::PolicyReach` (§ 8): textos en `guard.np.*`.
- Capacidad `guard.policies` en `crates/api/src/methods/guard.rs`.

Mensajes (`guard.txt`, plantillas fijas, parámetros etiquetados y saneados, sin mencionar cómo desactivar la política): `guard.reason.protected-branch` ("la rama «{branch}» está protegida (patrón «{pattern}», {level}); un agente no la mueve"), `guard.reason.forbidden-path` ("la ruta «{path}» está prohibida (patrón «{pattern}», {level}); un agente no la toca"), `guard.reason.forbidden-path-unverifiable`.

## 6. Criterios de aceptación verificables

Repos, remotos, perfiles y daemons temporales (NFR-01); Git, `raptor` y `raptor-hook` reales; el agente es `raptor-fake-agent` como Claude Code (patrón de `apps/cli/tests/guard_us_grd_018.rs`). Suite nueva: `apps/cli/tests/guard_us_grd_008.rs` (se niega a correr sin *debug assertions*). Pruebas unitarias en `crates/policy` (matcher, combinación, reglas, configuración) y `crates/git/tests/new_commit_paths.rs`.

| Criterio | Test |
|---|---|
| Un agente no hace commit, push ni borra `main` protegida; el motivo nombra la rama; con `--no-verify` también | `apps/cli` `guard_us_grd_008::protected_branch_blocks_an_agent` |
| La persona sí (misma operación sin agente) y una rama no protegida es libre | `the_person_and_other_branches_are_free` |
| `appliesTo: everyone` bloquea también a la persona | `everyone_applies_to_the_person_too` |
| Un agente no commitea una ruta prohibida (modificar, crear, borrar); los cambios siguen en el working tree; con `--no-verify` y con `commit-tree` + `update-ref`; el motivo nombra la ruta | `forbidden_path_blocks_an_agent_commit` |
| Una ruta prohibida no sale por `push` aunque el commit se hiciera con `HEAD` separado | `forbidden_path_does_not_leave_by_push` |
| Dos reglas incumplidas se nombran juntas | `two_broken_rules_are_named_together` |
| Cada denegación aparece en `raptor guard log` con su regla | `denials_reach_the_decision_log` |
| Un nivel personal solo endurece; el suelo no se relaja; el mínimo sigue | `levels_only_harden` |
| Sin daemon (degradado) no se aplican y el mínimo sigue; un daemon sin `guard.policies` no cambia | `degraded_mode_applies_no_policy_rule` |
| Matcher: ramas, rutas, normalización, topes | `crates/policy` `guard::glob::tests::*`, `guard::policies::tests::*` |
| Configuración: formas, límites, `policy-invalid` | `crates/policy` `settings::document::tests::*` |
| Commits nuevos y rutas: commit, fusión (diferencia con todos los padres), rango, tope | `crates/git/tests/new_commit_paths.rs` |

## 7. Orden de implementación

1. Pruebas en rojo (e2e `guard_us_grd_008.rs`).
2. `crates/policy` (configuración, matcher, reglas) → `crates/git` (commits nuevos) → `crates/api` (contrato, capacidad) → `crates/core` (configuración efectiva, actor, hechos, evaluación) → `apps/cli` (mensajes y lista).
3. Documentación, estado de la historia, `backlog.md`, `release-status.md`.

## 8. Pendientes, límites y fuera de alcance

**Lo que no se puede impedir** (se publica en `NotPreventable`, texto en `guard.np.*`; la protección cubre **agentes detectados o registrados**, no "cualquier proceso"):

- **`policy-actor`**: las reglas con `appliesTo: agents` reconocen al agente por su proceso. No lo reconoce un agente que el daemon no detecta ni tiene registrado: un proceso huérfano o reparentado (`setsid`, `nohup`), lanzado por `launchd`/`at`/`ssh` o desde un contenedor cuenta como persona. Sin daemon (modo degradado) solo se aplican las reglas `everyone` del suelo; un agente puede forzar ese modo borrando el perfil o impidiendo que arranque el daemon.
- **`policy-reach`**: una ruta prohibida se verifica en los commits que llegan a una rama o se suben a una rama con `push`. No ve: un commit que nadie mueve a una rama ni sube (objeto suelto); `git stash` y el working tree sin commitear; un push **solo a tags u otras refs no gobernadas** (el dispatcher sale sin evaluar; cerrarlo exige una plantilla 3, ADR-GRD-001); un commit escondido bajo una rama remota que el agente escribió a mano (`update-ref refs/remotes/…`: cuenta como "ya existía", el mismo residuo de DS-US-GRD-018 § 11); un agente que cambia la rama **en el servidor** (otra herramienta, la API del remoto); en repos con refs `reftable`, `branch -m/-M` sobre cualquier rama protegida (no ejecuta el hook, como ya figura para la base: `rename-base-reftable`, que se amplía a las ramas protegidas). `reset --hard` y `checkout` sobre una rama protegida tocan el working tree antes de la ref (ya figura `reset-hard`); `send-pack` directo no pasa por `pre-push` (ya figura).

**Otros residuos declarados**: más de 256 commits nuevos o 100 000 rutas se deniegan por no poder verificarse (`unverifiable`); un rebase de una rama que lleva un cambio de la persona en una ruta prohibida se deniega al agente; el nivel **local** no tiene lector todavía (US-GRP-013). **Traspaso a US-GRD-016 / TS-CKP-003**: hoy el gate del Cockpit rechaza lo gobernado (`NoGuardrails`), sin salto posible, pero el hook bajo el ejecutor devuelve "la decisión ya tomada": cuando exista el motor del gate, esa decisión debe incluir estas reglas (criterio de aceptación de US-GRD-016) o `safe_commit` las saltará.

| Pendiente | Dueño |
|---|---|
| La misma decisión por MCP (`safe_commit`, etc.) | US-GRD-016 (BR-CONS-002) |
| Excepción de la persona para una regla `everyone` | ADR-GRD-007 (acciones reservadas, fuera del MVP) |
| Mostrar las políticas efectivas en `raptor guard status` | US-GRD-004/ historia de estado por escribir |
| Agente registrado (US-GRP-009) sin proceso detectado como actor | US-GRP-009 / US-GRD-005 (ya pendiente en DS-US-GRD-018) |
| Windows (no hay canal) y Linux | **Pendiente: etapa de validación multiplataforma**; Linux lo cubre el CI de ubuntu |
| Límite de tamaño de diff y formato de commit (otras filas de BR-VAL-003) | US-GRD-009 y siguientes |

## 9. Validación de las decisiones

**Decisión del orquestador (2026-10-08), validada por el Arquitecto (`nassa-architect:architect`) y el PO (`nassa-aadd:product-owner`)**, una consulta cada uno sobre la primera versión de esta Dev Spec. Ajustes incorporados:

| Origen | Ajuste | Dónde |
|---|---|---|
| Arquitecto B2 | En degradado se aplican las reglas `everyone` del suelo; se enmienda la garantía de ADR-GRD-003 § 4 | D11 |
| Arquitecto B4 | `old` en ceros se resuelve con el valor actual de la ref | D5 |
| Arquitecto D1, PO 2 | Solo se descarta el patrón inválido; `!`, `#` y `refs/heads/` se rechazan; `configStatus` `parcial` viaja en cada decisión | D1 |
| Arquitecto D4 | Pasada propia para la rama protegida; plegado siempre de mayúsculas y NFC; `reftable`; congelación de la rama con `everyone` | D4, § 4, § 8 |
| Arquitecto D5 | Topes de trabajo (commits visitados, puntas, agregado); `unverifiable` por repo superficial o clon parcial; asimetría de `pre-push` | D5 |
| Arquitecto D9 | Tests de la capacidad en las dos direcciones | D9, § 6 |
| Arquitecto D12 | Medición con 2 000 ramas antes de cerrar | D12 |
| PO 1 / Arquitecto B3 | Decisión de negocio nueva en `context.md` y BR-VAL-003 (el actor condiciona estas dos reglas), enmienda del Gherkin de la historia, y confirmación pendiente de Rene en el PR | D3, ver nota |
| PO 3, 4 | Se nombran todas las reglas; tests de force-push, borrado remoto y crear una rama que casa con un patrón protegido | D7, § 6 |
| Arquitecto D5, D6, § 8 | Residuos: remote-tracking falsificado, `reftable`, servidor, ejecutor; `policy-actor` con los casos concretos | § 8 |

**Desacuerdo resuelto por el orquestador**: el Arquitecto marcó como bloqueante (B1) cerrar un push solo a tags. El dispatcher sale sin evaluar cuando todas las refs remotas son no gobernadas (vía rápida de `sh`), así que cubrirlo exige una plantilla 3 del dispatcher y su ruta de actualización (ADR-GRD-001, S8 de DS-US-GRD-018): cambio de instalación fuera del alcance de esta historia y de su rama. Se **declara** en `policy-reach` y se anota como pendiente con dueño en el PR; el Arquitecto lo acepta como residuo declarado si figura en la lista pública.

**Preguntas para Rene (van en el PR, "Para Rene")**: (1) confirmar el valor por defecto `agents` y que "sin atribuir" pase (refina Q-GRD-1); (2) si en degradado las reglas `agents` deben aplicarse a "sin atribuir"; (3) si el push solo a tags justifica una plantilla 3.


## Anexo de forma (perfil `backend-service`)

### 6.1 Tipos compartidos

Los de § 5: `Rule::{ProtectedBranch, ForbiddenPath}`, `Cause::Unverifiable`, `ParamKind::{Pattern, Path}`, `NotPreventable::{PolicyActor, PolicyReach}` y la capacidad `guard.policies`, todos en `crates/api/src/guard.rs` y `crates/api/src/methods/guard.rs`. Del lado de `crates/policy`: `settings::model::{Policies, PatternPolicy, AppliesTo}`, `guard::policies::{Rules, Scope, Touched}` y `guard::glob::{Pattern, Kind}`. De `crates/git`: `RepoReader::new_commit_paths` y `NewCommits`.

### 6.2 Ciclos de vida (DI)

_No aplica — no hay contenedor de dependencias ni servicios con ciclo de vida: la regla es una función pura y la configuración se carga con el `TeamLoader` estático de `crates/core/src/guardrails/authorship.rs` (caché acotada), igual que `commitAuthorship`._

### 6.3 Firmas del stack

`policy::guard::evaluate(&Operation, &Facts, &Context) -> Evaluation` (sin cambiar la firma: `Context` gana `policies` y `Facts` gana `touched`); `evaluate::evaluate_commit(reader, common, op, bases, CommitContext)` (el `CommitContext` gana `policies`); `RepoReader::new_commit_paths(&self, hidden: &[&str], new: &str, limits: Limits) -> Result<Touched, ReadError>`.

### 7.1 Forma del error

Una denegación es un `Decision` con `reasons[]` (`policy.protected-branch`, `policy.forbidden-path`, causa opcional `unverifiable`); sin códigos de error JSON-RPC nuevos. El hook sale con 1 y escribe la plantilla fija por stderr (M-05).

### 7.2 Forma de la configuración

§ 4: `policies.protectedBranches` y `policies.forbiddenPaths`, `{ patterns: [string], appliesTo: "agents" | "everyone" }`, generadas en `crates/policy/schema/settings.schema.json` desde los tipos.

### 7.3 Valores numéricos

64 patrones por clave; 256 bytes por patrón; 256 commits nuevos y 100 000 commits visitados por línea; 2 000 puntas de ramas; 4 096 commits nuevos y 100 000 rutas por evaluación; parámetros truncados a 120 caracteres al mostrarse (M-05).

### 8. Modelo de datos

_No aplica — no se crea ningún almacén ni columna: el registro de decisiones (US-GRD-005) guarda las razones sin parámetros, así que no guarda rutas ni patrones._

### 9. Estrategia de pruebas

§ 6: suite e2e `apps/cli/tests/guard_us_grd_008.rs` con Git, `raptor` y `raptor-hook` reales sobre repos y perfiles temporales; unitarias del matcher, la combinación y las reglas en `crates/policy`; `crates/git/tests/new_commit_paths.rs` para la lectura de commits nuevos (commit, fusión, rango, superficial, clon parcial, rutas no UTF-8 y NFD, topes); un test de `crates/core` por cada sentido de la capacidad `guard.policies`.


## Gaps y violaciones de la constitución

_No gaps. Ready to implement._ Lo diferido tiene dueño en § 8. Las decisiones que necesitan la confirmación de Rene (el valor por defecto `agents`, el modo degradado, la plantilla 3) están en § 9 y van en el PR; ninguna impide implementar, porque el comportamiento por defecto es el más restrictivo que no bloquea a la persona.

## Plan de implementación

> Orden topológico (`Depende:`). Rutas relativas a la raíz del repo. T001 son los tests en rojo.

| # | Tarea | Depende | Aterriza en |
|---|---|---|---|
| T001 | Escribir la suite e2e en rojo | — | `apps/cli/tests` |
| T002 | Claves, límites y esquema de `policies` | — | `crates/policy/src/settings`, `crates/policy/schema` |
| T003 | Matcher de patrones y reglas puras | T002 | `crates/policy/src/guard` |
| T004 | Leer los commits nuevos y sus rutas | — | `crates/git` |
| T005 | Contrato y capacidad `guard.policies` | T003 | `crates/api` |
| T006 | Configuración efectiva, actor, hechos, degradado | T003, T004, T005 | `crates/core` |
| T007 | Mensajes, registro y lista "no se puede impedir" | T005, T006 | `apps/cli` |
| T008 | Verificar, medir y documentar | T001, T007 | `docs`, `apps/cli/tests` |

### T001 — Escribir la suite e2e en rojo

**Objetivo.** `apps/cli/tests/guard_us_grd_008.rs` con los escenarios de la historia y § 6; todos fallan antes de implementar (el comportamiento no existe) y ninguno depende de una API que aún no está.

**Ubicación.**
- `apps/cli/tests/guard_us_grd_008.rs` (**CREATE**)

**Reglas**
- Repos, remotos y perfiles temporales, nunca este repo (NFR-01); sin esperas fijas; el agente es `raptor-fake-agent`; todos los mensajes de commit llevan el trailer de `agents-commit` para que solo las políticas nuevas puedan denegar.

- **Depende:** —
- **Refs:** US-GRD-008; DS-US-GRD-018 § 7
- **Aceptación:** `cargo test -p gitraptor-cli --test guard_us_grd_008 protected_branch_blocks_an_agent`

### T002 — Claves, límites y esquema de `policies`

**Objetivo.** `policies.protectedBranches` y `policies.forbiddenPaths` tipadas, con sus límites, el diagnóstico `policy-invalid` y el esquema regenerado.

**Ubicación.**
- `crates/policy/src/settings/model.rs`, `document.rs`, `diagnostic.rs` (**MODIFY**)
- `crates/policy/schema/settings.schema.json` (**MODIFY**, regenerado)

**Reglas**
- Un patrón inválido se descarta solo él y deja la fuente `parcial`; una clave de tipo equivocado no se aplica; los niveles admitidos salen de `x-gitraptor-levels`; el esquema se regenera con `GITRAPTOR_WRITE_SCHEMA=1 cargo test -p gitraptor-policy schema_`.

- **Depende:** —
- **Refs:** D1, D2; ADR-GRP-007
- **Aceptación:** `cargo test -p gitraptor-policy settings::`

### T003 — Matcher de patrones y reglas puras

**Objetivo.** `guard/glob.rs`, `guard/policies.rs` y las llamadas desde `evaluate`: rama protegida en una pasada propia, rutas prohibidas sobre `Facts.touched`, todas las razones juntas.

**Ubicación.**
- `crates/policy/src/guard/glob.rs`, `policies.rs` (**CREATE**)
- `crates/policy/src/guard/mod.rs` (**MODIFY**)

**Reglas**
- Sin E/S ni reloj (ADR-GRD-003 § 1); el matcher recorre segmentos con topes de longitud; solo añade denegaciones; la rama protegida no cuelga de los `return` tempranos del mínimo.

- **Depende:** T002
- **Refs:** D3, D4, D6, D7
- **Aceptación:** `cargo test -p gitraptor-policy guard::`

### T004 — Leer los commits nuevos y sus rutas

**Objetivo.** `RepoReader::new_commit_paths`: commits nuevos de un movimiento (ocultando `old` y las demás ramas), diferencia de árbol contra todos los padres, topes y `unverifiable`.

**Ubicación.**
- `crates/git/src/guard_read.rs` (**MODIFY**)
- `crates/git/tests/new_commit_paths.rs` (**CREATE**)

**Reglas**
- Lector aislado, sin objetos de reemplazo ni commit-graph; ocultar `old` y las ramas locales y remotas salvo las que se actualizan; `old` en ceros se resuelve fuera, en el llamador; fail-closed ante cualquier límite.

- **Depende:** —
- **Refs:** D5
- **Aceptación:** `cargo test -p gitraptor-git --test new_commit_paths`

### T005 — Contrato y capacidad `guard.policies`

**Objetivo.** Reglas, causa, parámetros, `NotPreventable` y la capacidad.

**Ubicación.**
- `crates/api/src/guard.rs`, `crates/api/src/methods/guard.rs` (**MODIFY**)

**Reglas**
- Un archivo propio por pieza (ADR-GRP-016); la capacidad se declara solo en `methods/guard.rs`; `crates/api/tests/architecture.rs` debe seguir en verde.

- **Depende:** T003
- **Refs:** D8, D9
- **Aceptación:** `cargo test -p gitraptor-api`

### T006 — Configuración efectiva, actor, hechos, degradado

**Objetivo.** Cargar las políticas de las fuentes, resolver el actor de `RefTransaction` y `Push`, leer los hechos con el lector aislado, aplicar las reglas `everyone` del suelo en degradado.

**Ubicación.**
- `crates/core/src/guardrails/policies.rs` (**CREATE**)
- `crates/core/src/guardrails/evaluate.rs`, `hook.rs`, `mod.rs` (**MODIFY**)
- `crates/core/src/channel/conn.rs` (**MODIFY**)

**Reglas**
- La configuración se lee en el daemon, nunca del cliente; el actor lo resuelve el daemon; la lectura de commits solo ocurre con una regla aplicable; sin la capacidad nada cambia.

- **Depende:** T003, T004, T005
- **Refs:** D3, D5, D9, D11, D12
- **Aceptación:** `cargo test -p gitraptor-core guardrails:: --test guard_evaluate`

### T007 — Mensajes, registro y lista "no se puede impedir"

**Objetivo.** Plantillas en/es, textos del registro y los dos `NotPreventable`.

**Ubicación.**
- `apps/cli/src/guard.rs` (**MODIFY**)
- `apps/cli/i18n/en/guard.txt`, `apps/cli/i18n/es/guard.txt` (**MODIFY**)
- `crates/policy/src/guard/mod.rs` (**MODIFY**, `not_preventable`)

**Reglas**
- Plantillas fijas con parámetros etiquetados y saneados; nunca mencionan cómo desactivar la política; los `match` exhaustivos de `apps/cli/src/guard.rs` cubren las reglas nuevas.

- **Depende:** T005, T006
- **Refs:** D8, D10, § 8
- **Aceptación:** `cargo test -p gitraptor-cli --test guard_us_grd_008`

### T008 — Verificar, medir y documentar

**Objetivo.** Suite completa en verde, medición de D12, enmienda de ADR-GRD-003 y ADR-GRD-002, estado de la historia, `backlog.md` y `release-status.md`.

**Ubicación.**
- `docs/architecture/decisions/ADR-GRD-003-motor-decision-contrato.md`, `ADR-GRD-002-operaciones-interceptables.md` (**MODIFY**)
- `docs/requirements/features/guardrails/user-stories/US-GRD-008-ramas-protegidas-rutas-prohibidas.md` (**MODIFY**)
- `docs/requirements/backlog.md`, `docs/requirements/release-status.md` (**MODIFY**)

**Reglas**
- No se declara hecho lo que no se verificó: Windows y Linux se marcan pendientes; `release-status.md` se regenera con `node tools/status/release-status.mjs`, no a mano.

- **Depende:** T001, T007
- **Refs:** D12
- **Aceptación:** `cargo test --workspace`
