---
id: DS-US-GRD-009
title: "Dev Spec — US-GRD-009: tamaño del diff y formato de commit"
type: dev-spec
status: draft
created: 2026-10-21
updated: 2026-10-21
story: US-GRD-009
feature: guardrails
domain: GRP
scope: backend
frontend_surface: false
stack: rust
profile: backend-service
tooling: [cargo]
related:
  context: ../context.md
  story: ../user-stories/US-GRD-009-tamano-diff-formato-commit.md
  stories: [US-GRD-009, US-GRD-008, US-GRD-007, US-GRD-018, US-GRD-010, US-GRD-011, US-GRD-006, US-GRD-016, US-GRD-005]
  adrs: [ADR-GRD-001, ADR-GRD-002, ADR-GRD-003, ADR-GRD-004, ADR-GRD-006, ADR-GRD-007, ADR-GRP-007, ADR-GRP-016]
  rules: [BR-VAL-003, BR-VAL-001, BR-CALC-001, BR-CONS-001, BR-CONS-002, BR-CONS-004, BR-EDGE-004]
  api_spec: null
  design_spec: null
  contracts: []
must_read:
  - ../user-stories/US-GRD-009-tamano-diff-formato-commit.md
  - ../business-rules.md
  - ../../../../architecture/decisions/ADR-GRD-002-operaciones-interceptables.md
  - ../../../../architecture/decisions/ADR-GRD-003-motor-decision-contrato.md
  - ../../../../architecture/decisions/ADR-GRD-004-configuracion-efectiva.md
  - ../../../../architecture/extender-sin-archivos-compartidos.md
  - ../../../../dev-briefs/layered-config.md
  - ./US-GRD-008-ramas-protegidas-rutas-prohibidas.md
  - ./US-GRD-007-permisos-por-operacion.md
  - ./US-GRD-018-autoria-commits-persona-y-agente.md
  - ../../../../../crates/policy/src/settings/model.rs
  - ../../../../../crates/policy/src/guard/mod.rs
  - ../../../../../crates/policy/src/guard/policies.rs
  - ../../../../../crates/policy/src/guard/glob.rs
  - ../../../../../crates/policy/src/layers.rs
  - ../../../../../crates/git/src/guard_paths.rs
  - ../../../../../crates/core/src/guardrails/evaluate.rs
  - ../../../../../crates/core/src/guardrails/layers.rs
  - ../../../../../crates/core/src/guardrails/policies.rs
  - ../../../../../crates/core/src/guardrails/hook.rs
  - ../../../../../crates/core/src/guardrails/plan_gate.rs
  - ../../../../../crates/api/src/guard.rs
  - ../../../../../crates/api/src/methods/guard.rs
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
  gaps_release: 0
  ready_to_release: true
tags: [guardrails, tamano-diff, formato-commit, conventional-commits, br-val-003, politicas, segunda-linea, capacidades]
---

# Dev Spec — US-GRD-009: tamaño del diff y formato de commit

Plano compacto (AADD ligero) de [US-GRD-009](../user-stories/US-GRD-009-tamano-diff-formato-commit.md): la configuración fija un **límite de líneas cambiadas por commit** y un **formato de mensaje** (Conventional Commits), y la capa de hooks deniega el commit que los incumple, con un motivo que da el tamaño y el límite, o el formato y un ejemplo válido. La regla es [BR-VAL-003](../business-rules.md) (filas "Límite de tamaño de diff" y "Formato de commit"), con S-GRD-7 (líneas añadidas más eliminadas), S-GRD-2 (el formato del equipo no lo cambia un nivel personal) y [BR-CALC-001](../business-rules.md) (se nombran todas las reglas incumplidas). Es el segundo bloque de políticas de Q-GRD-9 y sigue el patrón de [DS-US-GRD-008](./US-GRD-008-ramas-protegidas-rutas-prohibidas.md): regla pura en `crates/policy`, configuración y hechos leídos en el daemon en `reference-transaction` `prepared`, y nada nuevo viaja desde el cliente del hook.

**Invariante**: solo añade `deny` o `ask`; nunca relaja otra regla. GitRaptor nunca reescribe, divide ni corrige un commit o su mensaje: decide, explica y registra.

**Restricciones de arquitectura**: no hay `architecture-constitution.md` en la cascada. Rigen `AGENTS.md` (Rust, NFR-01, NFR-02, tests obligatorios) y los ADRs citados, como en las Dev Specs hermanas.

## 1. Decisiones

Las que fija un ADR o una regla lo citan. El resto son **Decisión del orquestador (2026-10-21), validada por Arquitecto** (`nassa-architect:architect`); las marcadas **(PO)** cambian lo que se promete al usuario y llevan su Q-GRD (§ 9): son **Decisión del orquestador (2026-10-21), validada por Arquitecto y PO**.

| # | Decisión |
|---|---|
| D1 | **Claves** `policies.diffSize` y `policies.commitFormat` (§ 4), niveles (`x-gitraptor-levels`) equipo, perfil y local, como `protectedBranches`. Dejan de producir `policy-not-supported`. `diffSize = { maxLines, exclude?, appliesTo?, effect? }`: `maxLines` entero de 1 a 100 000, obligatorio (sin él la regla no existe y la fuente queda `parcial` con `policy-invalid`); `exclude`, hasta 64 patrones de ruta de 256 bytes con el matcher de DS-US-GRD-008 D6 (`glob.rs`, `Kind::Path`). `commitFormat = { style?, types?, appliesTo?, effect? }`: `style` solo `conventional` (por defecto); `types`, de 1 a 32 tipos `[a-z][a-z0-9-]{0,31}`; sin `types` rigen los once de `@commitlint/config-conventional` (`build`, `chore`, `ci`, `docs`, `feat`, `fix`, `perf`, `refactor`, `revert`, `style`, `test`). `"commitFormat": {}` ya exige Conventional Commits. **Errores**: un patrón o un tipo inválido se descarta solo y la fuente queda `parcial` (`policy-invalid`), como en DS-US-GRD-008 D1. Un `style`, `appliesTo` o `effect` que esta versión no conoce se lee como el valor más estricto (`conventional`, `everyone`, `deny`) con `policy-invalid` (patrón M-03 de DS-US-GRD-008). Un `types: []`, un `maxLines` fuera de rango o un tipo JSON equivocado es un valor fuera de límites (PQ-8): la fuente entera se ignora con su diagnóstico (`out-of-range`, `wrong-type`). Si se descartan todos los tipos, la regla no acepta ningún tipo: una errata nunca relaja (Q-GRD-26) |
| D2 | **Combinación** (BR-CONS-001, brief de US-GRD-010/012 D2, ADR-GRD-004 § 2). **Límite**: cada fuente que declara `diffSize` aporta **su propia regla** (límite, exclusiones, `appliesTo` y efecto). Rigen las reglas del suelo, del suelo confirmado (así subir el límite o añadir una exclusión en el suelo espera la confirmación, como en `Layers::policies`, `crates/core/src/guardrails/layers.rs:94`), del worktree y la **personal** = la del local si la declara y, si no, la del perfil. El commit debe cumplir todas, así que el efectivo es el menor límite (filas 5 y 6 de US-GRD-010). **Las exclusiones solo valen para la regla de su fuente**: un nivel personal o el worktree no puede eximir de la regla del suelo. **Formato** (S-GRD-2, BR-CONS-001; **(PO, Q-GRD-51)**): rige **un solo formato, el del primer nivel que lo declare** en este orden: el suelo (o el suelo confirmado), después el worktree y, si ningún nivel del equipo lo fija, el personal (local ?? perfil). Si un nivel del equipo lo fija, ni el worktree (cuando lo fija el suelo) ni los personales cuentan: el formato es una convención del equipo y un personal no lo cambia, ni para quitar tipos. Se refina "el más específico que lo exija" de ADR-GRD-004 § 2 (§ 9). Lo que no cuenta se anota como relajación ignorada (`IgnoredRelaxation`, `crates/policy/src/layers.rs:88`) con dos claves nuevas: `RelaxKey::DiffSize` (un límite personal mayor que el del equipo) y `RelaxKey::CommitFormat` (un formato personal cuando un nivel del equipo fija uno) |
| D3 | **(PO, Q-GRD-48) A quién se aplica**: `appliesTo` por defecto `agents`, como las ramas protegidas y las rutas prohibidas (Q-GRD-35). Con `agents`, la regla solo alcanza a un agente detectado o registrado (actor que resuelve el daemon, DS-US-GRD-018 D5); "sin atribuir" (la persona en su terminal) pasa. Con `everyone`, también a la persona. El actor entra en la condición: enmienda de ADR-GRD-003 § 1 en el patrón de US-GRD-008 (§ 9). Un agente que el daemon no detecta cuenta como persona: residuo `policy-actor` (§ 8). Los escenarios de la historia dicen "un agente detectado", y la persona tiene su propio esquema (con el valor por defecto pasa; con `everyone`, no). Validada por el PO: el valor de la historia es revisar el trabajo de los agentes, y bloquear por defecto a la persona sin excepción consciente empuja a `--no-verify` (Q-GRD-46, R-GRD-2) |
| D4 | **Efecto `deny` o `ask`** (BR-VAL-003: "denegar, salvo que la configuración la marque como pedir confirmación"). `effect` por defecto `deny`. Varias reglas incumplidas dan el máximo (`deny > ask`). Un `ask` sigue DS-US-GRD-007 D19 sin cambios (`evaluate::decision_for`, `crates/core/src/guardrails/evaluate.rs:212`): el agente queda denegado con causa `ask-unavailable`; la persona (solo con `everyone`) pasa con el aviso `guard.notice.policy-ask-developer`, y el actor no demostrado (degradado, daemon sin la capacidad) queda denegado. Por MCP, el ejecutor ya distingue `ask` de `deny` (`view_of`, `crates/core/src/guardrails/plan_gate.rs:150`) y `raptor-mcp` lo devuelve como `confirmation-required` (US-MCP-013): la misma decisión y las mismas razones. Ver D17 sobre el ejecutor |
| D5 | **Dónde se evalúa: solo en `reference-transaction` `prepared`**, por el daemon, dentro de la evaluación `RefTransaction` que ya existe. Ni `pre-commit`, ni `commit-msg`, ni `pre-push`. **Motivos**: (a) un solo punto cubre `commit`, `--amend`, `--no-verify`, `commit-tree` + `update-ref`, `cherry-pick`, `revert`, `am`, `rebase` y el commit con `HEAD` separado que después llega a una rama; (b) el mensaje es el definitivo, el del objeto, después de `--cleanup`, `core.commentChar` y las plantillas; (c) el tamaño, el formato, las rutas prohibidas, la rama protegida y los permisos salen en **una** decisión, con todas sus razones (BR-CALC-001: "ruta prohibida y límite de diff → las dos"); (d) no hay hechos nuevos del cliente. El coste: la persona que escribe el mensaje en el editor recibe la denegación después de escribirlo, pero Git lo conserva en `.git/COMMIT_EDITMSG` (NFR-01, como en DS-US-GRD-008 § 3), y para un agente con `-m` no hay diferencia. Se propone la enmienda de la fila Commit de ADR-GRD-002 § 1, que hoy dice "`commit-msg` (formato)" (§ 9), con el precedente de la Nota (2026-10-08, US-GRD-008) para las rutas. `pre-push` no evalúa: un commit es lo que es al llegar a una rama local; lo que sale sin haber estado nunca en una rama se declara (`commit-policy-reach`, § 8) |
| D6 | **Qué commits se evalúan.** Por cada línea de `refs/heads/*` que crea o actualiza una rama, los **commits nuevos** del movimiento, con el recorrido de dos pasadas de DS-US-GRD-008 D5 (`RepoReader::walk_fresh`, `crates/git/src/guard_paths.rs`): primero solo `old` escondido; si nada incumple, la respuesta es final; ante un candidato a denegar, se confirma escondiendo las demás ramas y ramas remotas (`Hide::OtherBranches`). Cuentan **todos** los commits nuevos, sea cual sea el subcomando: los de un `rebase` (`-i`, `squash`, `reword` crean commits nuevos con mensaje y diff nuevos), `cherry-pick`, `revert`, `am` y el plumbing. Es la regla de DS-US-GRD-008 § 3 y no la lista inversa de DS-US-GRD-018 D9: la autoría se conserva al copiar un commit, pero el tamaño y el mensaje son del commit que llega. **Excepción**: si la cadena de `git` (DS-US-GRD-007 D6) es con certeza `fetch` o `clone`, la línea no se evalúa (los commits vienen del remoto). Una cadena `Unknown` se evalúa. **Fusiones** (2 o más padres): se evalúa el formato (con la exención de D9) y **no** el tamaño: lo que trae una fusión son commits que ya se evaluaron, y la resolución de conflictos queda declarada (§ 8). Un borrado no evalúa nada. El rango se acota con `PathLimits` (256 commits nuevos por línea, 4 096 por evaluación) |
| D7 | **Cómo se miden las líneas** (S-GRD-7). Por commit no fusión, diferencia de su árbol contra el del primer padre (contra el árbol vacío en un commit raíz): **líneas añadidas más eliminadas**, con el diff de línea de gix (`gix::diff::blob`, algoritmo Myers, el de Git por defecto). La feature `blob-diff` de gix ya está activa por `status` (`Cargo.toml:27`): no cambia ninguna dependencia. **Renombrados**: detección como el `diff.renames` por defecto de Git (similitud del 50 %, sin copias, hasta 1 000 candidatos); un renombrado exacto cuenta 0 y uno con cambios cuenta su diff. **Binarios**: un lado con un byte NUL en sus primeros 8 000 bytes (la heurística de Git) cuenta 0 líneas. **Submódulos**: 1 línea por lado, como `git diff --numstat`. **Cambio de modo** solo: 0. Los saltos de línea `\r\n` y los cambios de espacios cuentan. No se leen `.gitattributes` ni la configuración `diff.*` del repo: el resultado es determinista y no lo cambia un agente con `git config`. La ruta que se compara con `exclude` es la nueva (la vieja en un borrado). **Oráculo**: `git diff --numstat -M <padre> <commit>` sobre un corpus (T004); la diferencia de conteo que gix y xdiff puedan tener en entradas patológicas se declara (G2, § Notas del autor) |
| D8 | **Gramática de Conventional Commits** (especificación 1.0.0), sobre la primera línea del mensaje leída como bytes, con un tope de 4 096 bytes: `tipo` = `[A-Za-z][A-Za-z0-9-]*`, comparado sin distinguir mayúsculas con la lista (la especificación lo pide, salvo `BREAKING CHANGE`); `(ámbito)` opcional, de 1 a 64 bytes, sin `(`, `)` ni saltos de línea; `!` opcional; después `": "` exactamente; y una descripción con al menos un carácter que no sea espacio. Si hay más líneas, la segunda debe estar vacía (cuerpo separado por una línea en blanco). Los pies y `BREAKING CHANGE:` no se validan. Una primera línea vacía (mensaje en blanco o `--cleanup=verbatim` con líneas en blanco al principio), sin `": "` o de más de 4 096 bytes no cumple. Parser escrito a mano, una pasada, sin expresiones regulares: cumple el "tiempo lineal con tope de tamaño" de ADR-GRD-003 § 1. Un formato propio (`style: "pattern"` con una expresión) queda fuera del MVP (§ 8) |
| D9 | **(PO, Q-GRD-49) Mensajes que escribe Git.** No incumplen el formato: (a) en un commit de **fusión**, una primera línea que empieza por `Merge ` (`Merge branch 'x'`, `Merge remote-tracking branch`, `Merge tag`, `Merge pull request #n`); (b) `Revert "…"` o `Reapply "…"` (Git 2.43 y siguientes) cuando el cuerpo lleva la línea `This reverts commit <oid>.` que escribe `git revert`; (c) `fixup! `, `squash! ` o `amend! ` seguidos de una primera línea que cumple (se quitan hasta 8 prefijos). En un commit que no es fusión, `Merge …` no cumple. Un agente puede imitar (a) en una fusión y (b) a mano: el formato es una regla de legibilidad, no de seguridad, y se declara (§ 8). Validada por el PO: son los mensajes estándar que el equipo ya lee en su historial; exigir el formato obligaría a `-m` en cada fusión y revert y rompería el flujo de `--fixup` + `--autosquash`. Es la misma lista que commitlint ignora por defecto (`@commitlint/is-ignored`, consultado el 2026-10-21: fusiones, `Revert`/`Reapply`, `fixup!`/`squash!`/`amend!`), y D9 es más estricta: pide la fusión real, la línea de `git revert` y una primera línea que cumpla tras el prefijo |
| D10 | **Contrato** (`crates/api/src/guard.rs`): reglas `policy.diff-size` y `policy.commit-format`; causas `type-not-allowed` (el tipo no está en la lista) y `body-not-separated` (falta la línea en blanco); se reutilizan `unverifiable` y `ask-unavailable`; parámetros nuevos `lines`, `limit`, `commits` y `types`, y se reutiliza `oid` (12 caracteres). `NotPreventable::CommitPolicyReach` (`commit-policy-reach`). Capacidad **`guard.commit-policies`** (`crates/api/src/methods/guard.rs`): con ella el daemon aplica las dos claves y envía sus reglas, causas y parámetros; sin ella, no cambia nada. Es una puerta de **compatibilidad**: un cliente viejo no sabría leer las reglas nuevas. Con un daemon sin la capacidad, el cliente aplica él mismo las reglas `everyone` del suelo (D14), como DS-US-GRD-008 D9 (M-05). Tests en los dos sentidos. Ningún código de error nuevo |
| D11 | **Razones y mensajes** (M-05: plantillas fijas, parámetros etiquetados, saneados y acotados a 120 caracteres). Una razón **por regla incumplida**: la del tamaño nombra el **primer** commit que la incumple (el más antiguo del rango), sus líneas y el límite, y `commits` si son varios; la del formato nombra el primer commit, la lista de tipos y un **ejemplo válido**. El ejemplo lo arma el cliente de la CLI a partir de la lista: `feat` si está permitido, si no el primer tipo, con una descripción fija por idioma («feat: describe el cambio en una línea» / «feat: describe the change in one line»). **El mensaje nunca repite el texto del commit** (ADR-GRD-003 § 3: "lo que nunca lleva: mensajes de commit"), ni siquiera el tipo que se usó. Ningún texto dice cómo subir el límite ni cómo quitar la política. Claves en § 5 |
| D12 | **El mensaje no sale del daemon.** El daemon lee del objeto del commit solo la primera línea (como mucho 4 096 bytes) y si la segunda está vacía, los pasa a la función pura que da los hechos de formato (`Header`), y suelta los bytes en la misma función, como `plan_gate::commit_facts` (`crates/core/src/guardrails/plan_gate.rs:407`). En `Facts` solo viajan el veredicto, el tipo en minúsculas (para compararlo con la lista) y la forma. Nada del mensaje va a `Decision`, al registro, al canal ni a los logs. ADR-GRD-003 M-06 ("al daemon nunca llega el mensaje") se mantiene: el cliente no envía nada nuevo. Las líneas por archivo se guardan con su ruta solo durante la evaluación, para las exclusiones |
| D13 | **Sin configuración y configuración ilegible.** Sin `diffSize` ni `commitFormat` en ninguna fuente no se lee nada y no hay coste (escenario 4: 5 000 líneas con «wip» pasan). Un nivel del equipo ilegible o `parcial` ya fuerza el mínimo (ADR-GRD-004 D12, US-GRD-011). Si el cargador del equipo falla del todo, el movimiento de una rama por un agente ya se deniega con `unverifiable` (DS-US-GRD-008 D13): no hay regla nueva que añadir. Un `maxLines` mal escrito deja la fuente ignorada o `parcial`, nunca "sin límite" en silencio: el aviso de US-GRD-011 lo cuenta |
| D14 | **Modo degradado y daemon sin la capacidad** (ADR-GRD-003 § 4: nunca menos que el mínimo más el suelo legible). El cliente evalúa con el mismo código las reglas `everyone` del suelo (`hook::floor_rules_for_everyone`, `crates/core/src/guardrails/hook.rs:797`, y `degraded`): lee él los commits nuevos con el mismo lector. Las reglas `agents` no se aplican (el actor es "sin atribuir"). Un `ask` en ese camino deniega a todos (DS-US-GRD-007 D19, no demostrado) |
| D15 | **Coste y topes** (ADR-GRD-002 § 5: < 100 ms p95 por evaluación gobernada). Los commits se leen solo si alguna regla de las dos claves alcanza al actor (`needs_commit_facts(actor)`), y las líneas solo si hay una regla de tamaño; con solo formato no se lee ningún árbol. **Topes por evaluación**: 16 MiB por blob, 64 MiB de blobs leídos, 100 000 archivos cambiados (`PathLimits.paths`), y los de commits de D6. Pasado un tope: si lo contado ya supera todos los límites aplicables, se deniega con el conteo como mínimo ("al menos N líneas"); si no, se deniega con `unverifiable` (fail-closed) **solo cuando hay una regla aplicable**. ⚠️ **ASSUMPTION**: un commit corriente (≤ 1 200 líneas en ≤ 50 archivos) añade ≤ 15 ms p95 a la evaluación. Se mide antes de cerrar (T009): commit corriente, commit de 1 200 líneas y rebase de 256 commits de 50 líneas, con 2 000 ramas |
| D16 | **Registro** (US-GRD-005, BR-CONS-004). Toda denegación entra por `log_decision` con sus reglas, nivel y causa; `raptor guard log` la muestra con `guard.log.rule.diff-size` y `guard.log.rule.commit-format`. El registro guarda las razones sin parámetros (`LoggedReason`): ni rutas, ni líneas, ni nada del mensaje. El `ask` de un agente cuenta en el KPI; el paso de la persona, no (Q-GRD-46) |
| D17 | **Excepción consciente y MCP**, solo como referencia. La excepción de US-GRD-006 salta `policy.diff-size` y `policy.commit-format`, también con `everyone`, pero solo en las operaciones que admite (push, borrar una rama local y rebase no interactivo; Q-GRD-47). En la práctica cubre el rebase de la persona que reescribe commits fuera de límite o de formato; un commit directo no tiene excepción en el MVP (DS-US-GRD-006 D5). Aquí no hay nada propio que hacer, y con `appliesTo: agents` la persona no la necesita. **Ejecutor y MCP** (BR-CONS-002): hoy `raptor-mcp` no expone ninguna escritura y el ejecutor decide un commit por su plan (`PlanShape::Commit`, `crates/policy/src/guard/plan.rs`), cuyo hook devuelve la decisión ya tomada. **Traspaso a US-GRD-016**: cuando exista un commit por el ejecutor, su plan tiene que llevar los hechos de estas reglas (formato del mensaje del plan; tamaño del índice frente a `HEAD`), o el commit las saltará. Lo dice su criterio de aceptación (§ 8). Validada por el PO: hoy no hay commit por MCP que saltar, y BR-CONS-002, que cubre US-GRD-016, ya exige que cada escenario de BR-VAL-003 dé la misma decisión por las dos capas |
| D18 | **Dos claves, un PR.** Comparten el punto de evaluación, el recorrido, la capacidad y los mensajes, así que van juntas. Depende solo de lo que ya está en `main` (US-GRD-008, US-GRD-007 PR-A con la cadena de `git` y `decision_for`, US-GRD-018). Cierra las filas 5 y 6 de US-GRD-010, que su brief dejó para esta historia |

## 2. Forma (archivos que se tocan)

Cada pieza nueva va en un archivo propio (ADR-GRP-016, [extender sin archivos compartidos](../../../../architecture/extender-sin-archivos-compartidos.md)); los archivos centrales ganan una línea o un brazo.

| Pieza | Archivo |
|---|---|
| Claves, límites, diagnósticos y esquema | `crates/policy/src/settings/{model,document}.rs`, `crates/policy/schema/settings.schema.json` (regenerado) |
| Gramática de Conventional Commits y mensajes de Git (puro) | `crates/policy/src/guard/commit_format.rs` (nuevo) |
| Reglas y combinación de las dos claves (puro) | `crates/policy/src/guard/commit_policies.rs` (nuevo); `guard/mod.rs`: `pub mod`, `Context.commit_policies`, `Facts.commits` y una llamada por línea de `RefTransaction` |
| Relajaciones ignoradas | `crates/policy/src/layers.rs` (`RelaxKey::{DiffSize, CommitFormat}`, `ignored_relaxations`) |
| Commits nuevos con sus líneas por archivo y su cabecera | `crates/git/src/guard_changes.rs` (nuevo; reutiliza `walk_fresh` de `guard_paths.rs`, que pasa a `pub(crate)`); `crates/git/src/lib.rs` (+1 línea) |
| Contrato: reglas, causas, parámetros, `NotPreventable`, capacidad | `crates/api/src/guard.rs`, `crates/api/src/methods/guard.rs` |
| Configuración efectiva, lectura de hechos en dos pasadas, salto de `fetch`/`clone` | `crates/core/src/guardrails/commit_policies.rs` (nuevo); `guardrails/{mod,layers,evaluate}.rs` |
| Capacidad en el canal y en el cliente; degradado | `crates/core/src/channel/conn.rs` (`guard_caller`), `crates/core/src/guardrails/hook.rs` |
| Mensajes, ejemplo y lista "no se puede impedir" | `apps/cli/src/guard.rs`, `apps/cli/i18n/{en,es}/guard.txt`; `crates/policy/src/guard/mod.rs` (`not_preventable`) |
| Tests | § 6 |
| Documentación | Este archivo; la historia; enmiendas de ADR-GRD-002/003/004 (§ 9); `backlog.md`; `release-status.md` |

## 3. Decisión por operación

| Operación (Git) | Dónde se decide | Qué se evalúa |
|---|---|---|
| `git commit`, `--amend`, `--no-verify`, `commit-tree` + `update-ref` | `reference-transaction` `prepared` | tamaño y formato del commit nuevo (el amend cuenta el commit entero, contra el padre del original) |
| `git merge` con commit de fusión, `pull` que fusiona | `prepared` | formato de la fusión (exención D9 a); tamaño no; los commits nuevos que no son la fusión, como cualquier commit |
| `git merge --squash` + `commit` | `prepared` | como un commit: el tamaño entero |
| `git rebase` (también `-i`, `--autosquash`), `cherry-pick`, `revert`, `am` | `prepared` | cada commit nuevo que llega a la rama (D6) |
| Fast-forward, `branch -f`, `reset`, `switch -C` a commits nuevos (p. ej. hechos con `HEAD` separado) | `prepared` | los commits nuevos del movimiento; a commits que ya tiene otra rama, nada |
| `fetch`/`clone` a una rama local | — | nada (cadena certera, D6) |
| `git push` | — | nada (D5; residuo en § 8) |

NFR-01 ante una denegación en `prepared` (como DS-US-GRD-008 § 3): la rama, el índice y el working tree quedan como antes, el mensaje sigue en `.git/COMMIT_EDITMSG` y el commit queda como objeto inalcanzable. En un rebase denegado, Git deja la rama sin mover y el rebase se puede abortar.

## 4. Configuración

```json
{
  "policies": {
    "diffSize": { "maxLines": 400, "exclude": ["Cargo.lock", "pnpm-lock.yaml"] },
    "commitFormat": { "style": "conventional", "types": ["feat", "fix", "docs", "chore", "test", "refactor"] }
  }
}
```

| Campo | Tipo | Por defecto | Validación |
|---|---|---|---|
| `policies.diffSize.maxLines` | entero | — (obligatorio) | 1 a 100 000; fuera de rango → `out-of-range` |
| `policies.diffSize.exclude` | `[string]` | `[]` | ≤ 64 patrones de ≤ 256 bytes, reglas de patrón de DS-US-GRD-008 D1 |
| `policies.commitFormat.style` | `enum` `conventional` | `conventional` | Desconocido → `conventional` + `policy-invalid` |
| `policies.commitFormat.types` | `[string]` | los 11 de `config-conventional` | 1 a 32; `[a-z][a-z0-9-]{0,31}`; uno inválido se descarta solo |
| `…appliesTo` (las dos) | `enum` `agents`, `everyone` | `agents` | Desconocido → `everyone` + `policy-invalid` |
| `…effect` (las dos) | `enum` `deny`, `ask` | `deny` | Desconocido → `deny` + `policy-invalid` |

La documentación del esquema explica: qué cuenta como línea (D7), que las exclusiones solo valen para la regla de su nivel (D2), los mensajes de Git que no incumplen (D9) y que con `everyone` la persona tampoco puede hacer un commit grande ni sin formato. Un commit directo no tiene excepción en el MVP; un rebase que reescribe esos commits sí (DS-US-GRD-006 D5, Q-GRD-47).

## 5. Contrato (`crates/api`)

- `Rule::DiffSize` = `policy.diff-size`; `Rule::CommitFormat` = `policy.commit-format`.
- `Cause::TypeNotAllowed` = `type-not-allowed`; `Cause::BodyNotSeparated` = `body-not-separated`. Se reutilizan `Cause::Unverifiable` y `Cause::AskUnavailable`.
- `ParamKind::{Lines, Limit, Commits, Types}`; se reutiliza `ParamKind::Oid`. `lines` es un número o `≥N` cuando un tope cortó el conteo (D15).
- `NotPreventable::CommitPolicyReach` = `commit-policy-reach`.
- Capacidad `guard.commit-policies` (`CAP_GUARD_COMMIT_POLICIES`, en `GROUP.capabilities`).

Mensajes (`apps/cli/i18n/{es,en}/guard.txt`):

| Clave | es |
|---|---|
| `guard.reason.diff-size` | GitRaptor: el commit {oid} cambia {lines} líneas y el límite del repo es {limit} ({level}). Divídelo en commits más pequeños. |
| `guard.reason.diff-size-commits` | GitRaptor: {commits} commits de este movimiento superan el límite de {limit} líneas ({level}); el primero, {oid}, cambia {lines}. |
| `guard.reason.diff-size-unverifiable` | GitRaptor: no se pudo contar el tamaño del commit {oid} y el repo tiene un límite de {limit} líneas ({level}). No se ejecuta. |
| `guard.reason.commit-format` | GitRaptor: el mensaje del commit {oid} no sigue Conventional Commits ({level}): «tipo(ámbito opcional): descripción», con un tipo de: {types}. Ejemplo: «{example}». |
| `guard.reason.commit-format-type` | igual, con «el tipo no está en la lista del repo» |
| `guard.reason.commit-format-body` | GitRaptor: el mensaje del commit {oid} necesita una línea en blanco entre la primera línea y el cuerpo (Conventional Commits, {level}). |
| `guard.example.commit-format` | {type}: describe el cambio en una línea — en: «{type}: describe the change in one line» |
| `guard.notice.policy-ask-developer` | GitRaptor: en este repo {rule} pide confirmación humana; se ejecuta porque lo haces tú. |
| `guard.log.rule.diff-size` / `guard.log.rule.commit-format` | límite de diff / formato de commit |
| `guard.np.commit-policy-reach` | el tamaño del diff y el formato se comprueban en los commits que llegan a una rama local: no en un commit que se sube sin haber estado en una rama, ni en el servidor, ni en lo que resuelve una fusión; los mensajes de Git (fusión, revert, fixup) no se exigen |

Con `ask` en lugar de `deny`, el agente recibe la misma plantilla con la causa `ask-unavailable` ("requiere confirmación humana y la confirmación aún no está disponible", como `guard.reason.permission-ask`). `guard.np.policy-actor` amplía su texto a estas dos políticas. El inglés va con las mismas claves.

## 6. Criterios de aceptación verificables

Repos, remotos, perfiles y daemons temporales (NFR-01); Git, `raptor` y `raptor-hook` reales; el agente es `raptor-fake-agent` (patrón de `apps/cli/tests/guard_us_grd_008.rs`), y sus commits llevan el trailer de `agents-commit`, para que solo estas políticas puedan denegar. La configuración se commitea en la rama principal. Suite nueva `apps/cli/tests/guard_us_grd_009.rs` (se niega a correr sin *debug assertions*).

| Escenario / criterio | Test |
|---|---|
| Esquema 1 · límite 400: un commit de 400 líneas del agente se ejecuta; uno de 1 200 no, con «1200» y «400» en el motivo; también con `--no-verify` y con `commit-tree` + `update-ref`; la rama, el índice y el working tree quedan igual y el mensaje sigue en `COMMIT_EDITMSG` | `diff_limit_decides_the_commit` (2 filas) |
| 2 · «arreglos varios» con Conventional Commits: no se ejecuta; el motivo nombra Conventional Commits, la lista de tipos y un ejemplo que cumple la gramática (el test lo pasa por el mismo parser); no repite el mensaje | `an_unformatted_message_is_denied_with_a_valid_example` |
| 3 · «fix: handle empty branch name» se ejecuta | `a_formatted_message_is_accepted` |
| 4 · sin límite ni formato en ningún nivel: 5 000 líneas con «wip» se ejecutan | `nothing_is_required_without_configuration` |
| 5 (D3) · la persona pasa con el valor por defecto; con `appliesTo: everyone`, no | `the_person_passes_unless_everyone` |
| D4 · `effect: ask`: el agente no pasa (`ask-unavailable`, cuenta en el KPI); la persona con `everyone` pasa con el aviso y no cuenta | `ask_denies_the_agent_and_warns_the_person` |
| D6 · `rebase -i` que aplasta commits pequeños en uno grande, `reword` a un mensaje sin formato y `cherry-pick` de un commit grande: denegados; un fast-forward a una rama que ya existe y un `pull` del remoto, no | `every_new_commit_is_evaluated_whatever_the_subcommand` |
| 6 (D6, D9) · fusión con «Merge branch 'x'»: pasa sin contar el tamaño; un commit normal con «Merge stuff», no | `merges_are_judged_by_their_message_only` |
| 6 (D9) · `git revert`, `commit --fixup` y `--squash` de un commit con formato pasan | `git_written_messages_pass` |
| D7 · renombrar un archivo de 2 000 líneas cuenta 0; un binario cuenta 0; `exclude: ["Cargo.lock"]` no cuenta el lockfile | `renames_binaries_and_exclusions_are_not_lines` |
| BR-CALC-001 · tamaño + formato + ruta prohibida en un commit: las tres razones en una decisión | `every_broken_rule_is_named_together` |
| US-GRD-010 filas 5 y 6 · equipo 400 y local 200 → 200; equipo 400 y local 1 000 → 400 (y se anota la relajación ignorada) | `a_personal_level_only_lowers_the_limit` |
| S-GRD-2, Q-GRD-51 · con formato en el suelo, un formato personal con más o con menos tipos no cuenta; sin formato en el suelo y con formato en el worktree, rige el del worktree y el personal no cuenta; sin formato en ningún nivel del equipo, el local lo exige | `the_team_format_is_not_changed_by_a_personal_level` |
| D16 · cada denegación está en `raptor guard log` con su regla; el registro no lleva nada del mensaje | `commit_policy_denials_reach_the_decision_log` |
| D14 · degradado: las reglas `everyone` del suelo aplican, las `agents` no, el mínimo sigue | `degraded_mode_applies_the_floor_rules_for_everyone` |

Además (`crates/core/tests/us_grd_009.rs`, `crates/policy/tests/us_grd_009_commit_policies.rs`, `crates/git/tests/fresh_commit_changes.rs`):

| Comportamiento | Test |
|---|---|
| Capacidad en los dos sentidos: sin ella el daemon no aplica nada nuevo; el cliente ante un daemon sin ella aplica las reglas `everyone` del suelo | `us_grd_009::commit_policies_capability_both_ways` |
| Subir `maxLines` o añadir `exclude` en el suelo no relaja hasta confirmar (suelo confirmado construido en el test); bajarlo endurece en el acto | `us_grd_009::a_floor_relaxation_waits_for_its_confirmation` |
| Topes (blob, total, archivos, commits): conteo "al menos" cuando ya supera, `unverifiable` cuando no; nunca se permite por un tope | `us_grd_009::bounds_never_allow` |
| Con solo `commitFormat` no se lee ningún árbol; sin reglas aplicables no se lee nada | `us_grd_009::nothing_is_read_that_no_rule_needs` |
| Coste de D15 (2 000 ramas; commit corriente, 1 200 líneas, rebase de 256) | `us_grd_009::commit_policies_cost` (release, `#[ignore]` fuera de la medición) |
| Gramática: tipos, mayúsculas, ámbito, `!`, `": "`, descripción vacía, segunda línea, 4 096 bytes, bytes no UTF-8; mensajes de Git (fusión, revert/reapply con y sin la línea, prefijos anidados); prueba de propiedades: nunca entra en pánico y es lineal | `us_grd_009_commit_policies::format_*` |
| Combinación y reglas: tabla de D2, exclusiones por regla, `appliesTo` × actor, `effect`, todas las razones juntas, una por regla | `us_grd_009_commit_policies::rules_*` |
| Configuración: formas, límites, valores desconocidos al más estricto, `policy-not-supported` ya no sale | `crates/policy` `settings::document::tests::commit_policies_*` |
| Líneas por archivo frente a `git diff --numstat -M` (corpus: añadir, borrar, modificar, CRLF, renombrado exacto y parcial, binario, submódulo, modo, commit raíz, amend) y topes | `fresh_commit_changes::*` |

## 7. Orden de implementación

Ver § Plan de implementación: un solo PR, T001 a T009.

## 8. Pendientes, límites y fuera de alcance

**Lo que no se puede impedir** (lista publicada, textos `guard.np.*`):

- **`commit-policy-reach`** (nuevo): no se comprueba un commit que se sube con `push` sin haber llegado nunca a una rama local (`commit-tree` y `push origin <oid>:refs/heads/x`), un cambio hecho en el servidor, ni lo que resuelve una fusión (el tamaño de una fusión no cuenta: un cambio escondido en la resolución de conflictos pasa). Los mensajes de Git (D9) se pueden imitar a mano.
- **`policy-actor`** (se amplía): las reglas `agents` reconocen al agente por su proceso; un agente no detectado cuenta como persona, y sin daemon solo aplican las reglas `everyone` del suelo.
- Los saltos voluntarios no cambian (`voluntary-skips`).

**Otros residuos declarados**: se deniega **de más** al agente que rebasa o hace `cherry-pick` de commits que no cumplen (hechos por la persona con `agents`, o anteriores a la política), y al que crea un commit raíz grande. Estos dos casos se publican en el apartado "lo que se deniega de más" (Q-GRD-31), como pidió el PO; más de 256 commits nuevos en una línea, o pasar un tope sin haber superado el límite, se deniega como `unverifiable`; no se leen `.gitattributes` (un `-diff` sobre un texto cuenta sus líneas: se cuenta de más, nunca de menos); un archivo de más de 16 MiB se deniega como no verificable; el nivel local no tiene lector en el modo degradado.

| Pendiente | Dueño |
|---|---|
| La misma decisión por MCP: los hechos de estas reglas en el plan de un commit del ejecutor (`PlanShape::Commit`) | US-GRD-016 (BR-CONS-002) |
| `effect: ask` en `protectedBranches` y `forbiddenPaths` (BR-VAL-003 lo admite para toda política; hoy solo estas dos claves lo tienen) | Seguimiento de US-GRD-008 |
| Formato propio con una expresión (`style: "pattern"`, motor lineal de ADR-GRD-003 § 1) y otras reglas del encabezado (largo máximo) | Historia por escribir (PO) |
| Excepción consciente para un commit directo con una regla `everyone` (el rebase ya la tiene, DS-US-GRD-006 D5) | Se reabre con un caso real (Q-GRD-47) |
| Mostrar el límite y el formato efectivos en `raptor guard status` | Historia de estado por escribir |
| Linux y Windows reales | **Pendiente: etapa de validación multiplataforma** (Linux lo cubre el CI de ubuntu; Windows, máquina real) |

## 9. Validación de las decisiones

Las decisiones técnicas (D1, D2, D4 a D8 y D10 a D18) son **Decisión del orquestador (2026-10-21), validada por Arquitecto** (`nassa-architect:architect`). Las de producto llevan su Q-GRD, registrada en `context.md` y en BR-VAL-003 / BR-CONS-001. Por la autonomía que Rene Bonilla delegó (2026-10-04), las decide el orquestador y las valida el PO; Rene puede vetarlas en el PR:

| Decisión | Q-GRD | Nota |
|---|---|---|
| D3 · `appliesTo` por defecto `agents` para el tamaño y el formato; la persona pasa salvo `everyone`. Refina Q-GRD-1 para estas dos políticas, como Q-GRD-35 para las otras dos | Q-GRD-48 | **Decisión del orquestador (2026-10-21), validada por Arquitecto y PO.** La historia dice "un agente detectado" en sus escenarios y añade el esquema de la persona |
| D9 · los mensajes que escribe Git (fusión, revert/reapply, fixup/squash/amend) no incumplen el formato | Q-GRD-49 | **Decisión del orquestador (2026-10-21), validada por Arquitecto y PO.** Se descarta exigir `-m` en fusiones y reverts. La historia gana un escenario |
| D2 (formato) · rige el formato del primer nivel que lo declare (suelo, worktree, personal), sin juntar el del equipo con el personal | Q-GRD-51 | **Decisión del orquestador (2026-10-21), validada por el PO.** El borrador aplicaba a la vez el del worktree y el personal cuando el suelo no fija formato, y en cambio ignoraba al personal cuando lo fija el suelo: el mismo personal contaba o no según el nivel del equipo. Se vuelve a la letra de BR-CONS-001 y S-GRD-2 |

**Revisión del PO de las demás decisiones con impacto de producto** (2026-10-21, sin cambios):

- **D1**: sin límite por defecto (`maxLines` obligatorio) y sin formato si nadie lo pide. Es lo que dicen BR-VAL-001 y el escenario 4. Los once tipos de `config-conventional` son el conjunto que conoce un equipo que ya usa Conventional Commits, y `fix:` del escenario 3 está dentro. Un valor desconocido se lee como el más estricto: una errata nunca relaja (Q-GRD-26).
- **D4**: `ask` sigue a Q-GRD-46. El agente queda denegado (`ask-unavailable` con Git directo, `confirmation-required` por MCP) y cuenta en el KPI; la persona con `everyone` pasa con aviso y no cuenta.
- **D6**: se evalúan todos los commits nuevos que llegan a una rama, también los de un rebase, porque `rebase -i` puede juntar commits pequeños en uno grande. El precio es que se deniega de más al agente que rebasa commits grandes de la persona. Se acepta hacia el lado seguro, como Q-GRD-31, y va en el apartado "lo que se deniega de más" de la lista publicada. El motivo nombra el commit, y la persona puede hacer el rebase ella misma.
- **D7**: líneas añadidas más eliminadas (S-GRD-7). Las exclusiones por ruta cubren los lockfiles, que es el caso que más estorba en un equipo real. Que un binario cuente 0 se acepta, porque el límite sirve para revisar y no para controlar seguridad (G2).
- **D17**: se acepta el traspaso a US-GRD-016.

**Enmiendas propuestas** (las aplica el orquestador; esta Dev Spec no edita ADRs):

| ADR | Enmienda |
|---|---|
| ADR-GRD-002 § 1 | Fila Commit: el formato y el tamaño se evalúan en `reference-transaction` `prepared`, desde el objeto del commit, no en `commit-msg` ni en `pre-commit` (D5); `pre-push` no los evalúa |
| ADR-GRD-003 § 1 | El actor entra en la condición de `policy.diff-size` y `policy.commit-format` (`appliesTo`), que solo añaden denegaciones (D3). El formato no usa expresiones en el MVP: parser lineal escrito a mano (D8). La cabecera del mensaje se lee en el daemon y no sale de la función que la clasifica; el cliente no envía nada nuevo (D12) |
| ADR-GRD-003 § 4 | Degradado: aplica también las reglas `everyone` de estas dos claves del suelo (D14) |
| ADR-GRD-004 § 2 | Formato: el del primer nivel que lo declare, empezando por el más general (suelo, worktree y después personal), y no "el más específico" (D2, Q-GRD-51). Límite: una regla por fuente con sus exclusiones; el efectivo es el menor |

**Preguntas abiertas**: ninguna. Q-GRD-48, Q-GRD-49 y Q-GRD-51 quedaron decididas el 2026-10-21. Si Rene veta alguna, cambia un valor por defecto, una lista cerrada o el orden de los niveles, y el contrato sigue igual.

## Anexo de forma (perfil `backend-service`)

### 6.1 Tipos compartidos

Los del § 5 en `crates/api`. En `crates/policy`: `settings::model::{DiffSize, CommitFormat, FormatStyle, PolicyEffect}` (`AppliesTo` ya existe); `guard::commit_format::{Header, Verdict, parse, git_written}`; `guard::commit_policies::{CommitPolicies, DiffRule, FormatRule, NewCommits, CommitFacts, FileLines, combine, apply}`. En `crates/git`: `RepoReader::fresh_commit_changes` y `NewCommitChanges { commits: Vec<CommitChange>, unverifiable }`, `CommitChange { oid, parents, head: Vec<u8>, body_separated, files: Option<Vec<(String, u32)>>, partial }`. En `crates/core`: `guardrails::commit_policies::{changes, floor_everyone}` y `Layers::commit_policies()`.

### 6.2 Ciclos de vida (DI)

_No aplica — no hay contenedor de dependencias: las reglas y el parser son funciones puras, la configuración sale del `LOADER` estático de `crates/core/src/guardrails/layers.rs` (caché acotada) y los commits se leen por evaluación._

### 6.3 Firmas del stack

`policy::guard::evaluate(&Operation, &Facts, &Context) -> Evaluation` sin cambiar la firma (`crates/policy/src/guard/mod.rs:130`): `Context` (`:68`) gana `commit_policies: CommitPolicies` y `Facts` (`:45`) gana `commits: Vec<Option<NewCommits>>`, alineado con las líneas. `commit_format::parse(head: &[u8], body_separated: bool, parents: usize) -> Header`. `commit_policies::apply(out: &mut Evaluation, commits: &NewCommits, actor: Option<AgentKind>, rules: &CommitPolicies, budget: &mut glob::Budget)`. `RepoReader::fresh_commit_changes(&self, old: Option<&str>, new: &str, updated: &[&str], hide: Hide, limits: &PathLimits, lines: Option<&LineLimits>) -> Result<NewCommitChanges, ReadError>` (`lines = None`: solo cabeceras). En `core`: `commit_policies::changes(reader, op, rules, actor, chain) -> Vec<Option<NewCommits>>`, con la misma forma que `policies::touched` (`crates/core/src/guardrails/policies.rs:163`), llamada desde `evaluate_preset` (`crates/core/src/guardrails/evaluate.rs:128`).

### 7.1 Forma del error

Una denegación es un `Decision` con `reasons[]` (`policy.diff-size`, `policy.commit-format`; causa opcional `type-not-allowed`, `body-not-separated`, `unverifiable` o `ask-unavailable`). Sin códigos de error JSON-RPC nuevos. El hook sale con 1 y escribe la plantilla fija por stderr (M-05).

### 7.2 Forma de la configuración

§ 4, generada en `crates/policy/schema/settings.schema.json` desde los tipos (`GITRAPTOR_WRITE_SCHEMA=1 cargo test -p gitraptor-policy schema_`).

### 7.3 Valores numéricos

`maxLines` 1 a 100 000; `exclude` ≤ 64 × 256 bytes; `types` 1 a 32 × 32 bytes; ámbito ≤ 64 bytes; cabecera ≤ 4 096 bytes; hasta 8 prefijos `fixup!`/`squash!`/`amend!`; renombrados al 50 % con ≤ 1 000 candidatos; binario = NUL en los primeros 8 000 bytes; 16 MiB por blob y 64 MiB por evaluación; commits y archivos con los topes de `PathLimits` (256 por línea, 4 096 por evaluación, 100 000 archivos); parámetros truncados a 120 caracteres (M-05); presupuesto < 100 ms p95 por evaluación.

### 8. Modelo de datos

_No aplica — no se crea almacén ni columna: el registro guarda las razones sin parámetros._

### 9. Estrategia de pruebas

§ 6: e2e `apps/cli/tests/guard_us_grd_009.rs` con Git, `raptor` y `raptor-hook` reales sobre repos y perfiles temporales; `crates/core/tests/us_grd_009.rs` para la capacidad, el suelo confirmado, los topes y el coste; `crates/policy/tests/us_grd_009_commit_policies.rs` para la gramática y las reglas (API pública); `crates/git/tests/fresh_commit_changes.rs` contra `git diff --numstat -M` como oráculo.

## Gaps y violaciones de la constitución

_No gaps. Ready to implement._

Ningún hueco bloquea la implementación ni la liberación. Sin constitución en la cascada: se aplican `AGENTS.md` y los ADRs citados.

G1 (Q-GRD-48 y Q-GRD-49, que bloqueaba la liberación) se cerró el 2026-10-21. Fueron decisiones del orquestador validadas por el Arquitecto y el PO, y la historia ya dice "un agente detectado" y tiene los escenarios 5 y 6. La validación en Linux y Windows reales sigue en la etapa multiplataforma (§ 8): no es un hueco de esta Dev Spec.

## Plan de implementación

> Orden topológico (`Depende:`). Rutas relativas a la raíz del repo. T001 son los tests en rojo.

| # | Tarea | Depende | Aterriza en |
|---|---|---|---|
| T001 | Suite e2e en rojo | — | `apps/cli/tests` |
| T002 | Claves, límites, diagnósticos y esquema | — | `crates/policy/src/settings`, `crates/policy/schema` |
| T003 | Gramática y mensajes de Git; reglas y combinación | T002 | `crates/policy/src/guard`, `crates/policy/src/layers.rs` |
| T004 | Commits nuevos con líneas por archivo y cabecera | — | `crates/git` |
| T005 | Contrato y capacidad `guard.commit-policies` | — | `crates/api` |
| T006 | Configuración efectiva y lectura de hechos en el daemon | T003, T004, T005 | `crates/core/src/guardrails`, `crates/core/src/channel` |
| T007 | Cliente: degradado y daemon sin la capacidad | T006 | `crates/core/src/guardrails/hook.rs` |
| T008 | Mensajes, ejemplo, registro y lista "no se puede impedir" | T005, T006 | `apps/cli`, `crates/policy/src/guard/mod.rs` |
| T009 | Verificar, medir y documentar | T001, T007, T008 | `docs`, `crates/core/tests` |

### T001 — Suite e2e en rojo

**Objetivo.** `apps/cli/tests/guard_us_grd_009.rs` con las filas e2e del § 6; fallan antes de implementar y no dependen de una API que aún no existe.

**Ubicación.**
- `apps/cli/tests/guard_us_grd_009.rs` (**CREATE**)

**Reglas**
- Repos, remotos y perfiles temporales, nunca este repo (NFR-01); sin esperas fijas; el agente es `raptor-fake-agent` con el trailer de `agents-commit`; los diffs se generan con un número exacto de líneas (no "aproximadamente").
- Verificar en la versión de Git del runner (2.38 a 2.56) el ⚠️ ASSUMPTION de D7 (`diff.renameLimit` por defecto) y el mensaje de `git revert` sobre un revert (`Reapply`), y anotarlo.

- **Depende:** —
- **Refs:** US-GRD-009; US-GRD-010 filas 5 y 6; DS-US-GRD-008 § 6
- **Aceptación:** `cargo test -p gitraptor-cli --test guard_us_grd_009 diff_limit_decides_the_commit`

### T002 — Claves, límites, diagnósticos y esquema

**Objetivo.** `policies.diffSize` y `policies.commitFormat` tipadas, con sus límites, los valores desconocidos leídos al más estricto y el esquema regenerado.

**Ubicación.**
- `crates/policy/src/settings/model.rs`, `crates/policy/src/settings/document.rs` (**MODIFY**)
- `crates/policy/schema/settings.schema.json` (**MODIFY**, regenerado)

**Reglas**
- Un patrón o un tipo inválido se descarta solo y deja la fuente `parcial`; `types: []`, `maxLines` fuera de rango o un tipo equivocado ignoran la fuente (PQ-8); niveles desde `x-gitraptor-levels`.

- **Depende:** —
- **Refs:** D1; ADR-GRP-007
- **Aceptación:** `cargo test -p gitraptor-policy settings::`

### T003 — Gramática y mensajes de Git; reglas y combinación

**Objetivo.** `guard/commit_format.rs` (D8, D9) y `guard/commit_policies.rs` (D2 a D4, D6, D11): una razón por regla incumplida, exclusiones por regla, `appliesTo` × actor, `effect`; `RelaxKey::{DiffSize, CommitFormat}`; la llamada desde `evaluate` en cada línea de `refs/heads/*` de `RefTransaction`.

**Ubicación.**
- `crates/policy/src/guard/commit_format.rs`, `crates/policy/src/guard/commit_policies.rs` (**CREATE**)
- `crates/policy/src/guard/mod.rs`, `crates/policy/src/layers.rs` (**MODIFY**)
- `crates/policy/tests/us_grd_009_commit_policies.rs` (**CREATE**)

**Reglas**
- Sin E/S ni reloj (ADR-GRD-003 § 1); parser de una pasada sin expresiones regulares; solo añade `deny`/`ask`; ninguna razón lleva texto del mensaje.

- **Depende:** T002
- **Refs:** D2–D4, D6, D8, D9, D11
- **Aceptación:** `cargo test -p gitraptor-policy --test us_grd_009_commit_policies`

### T004 — Commits nuevos con líneas por archivo y cabecera

**Objetivo.** `RepoReader::fresh_commit_changes`: los commits nuevos de `walk_fresh` con su número de padres, la primera línea acotada y si la segunda está vacía, y (si se piden) las líneas por archivo contra el primer padre, con renombrados, binarios, submódulos y topes.

**Ubicación.**
- `crates/git/src/guard_changes.rs` (**CREATE**)
- `crates/git/src/guard_paths.rs` (**MODIFY**: `walk_fresh` pasa a `pub(crate)`), `crates/git/src/lib.rs` (**MODIFY**)
- `crates/git/tests/fresh_commit_changes.rs` (**CREATE**)

**Reglas**
- Lector aislado, sin objetos de reemplazo ni commit-graph; sin leer `.gitattributes` ni `diff.*`; no se lee ningún árbol si no se piden líneas; las fusiones no cuentan líneas; fail-closed ante cualquier tope; la prueba estática de escrituras de gitoxide sigue verde (sin `new_commit` en nombres).

- **Depende:** —
- **Refs:** D6, D7, D15
- **Aceptación:** `cargo test -p gitraptor-git --test fresh_commit_changes`

### T005 — Contrato y capacidad `guard.commit-policies`

**Objetivo.** Reglas, causas, parámetros, `NotPreventable::CommitPolicyReach` y la capacidad (§ 5).

**Ubicación.**
- `crates/api/src/guard.rs`, `crates/api/src/methods/guard.rs` (**MODIFY**)

**Reglas**
- La capacidad se declara solo en `methods/guard.rs`; códigos en kebab-case con test de ida y vuelta; `crates/api/tests/architecture.rs` sigue verde.

- **Depende:** —
- **Refs:** D10
- **Aceptación:** `cargo test -p gitraptor-api`

### T006 — Configuración efectiva y lectura de hechos en el daemon

**Objetivo.** `Layers::commit_policies()` desde la carga única de DS-US-GRD-007 D11; `commit_policies::changes` en dos pasadas, que salta una cadena certera `fetch`/`clone`, convierte la cabecera en `Header` y suelta los bytes; `evaluate_op` y `evaluate_preset` lo cablean solo con la capacidad; el filtro de avisos de `decide_op` conserva `policy-ask-developer`.

**Ubicación.**
- `crates/core/src/guardrails/commit_policies.rs` (**CREATE**)
- `crates/core/src/guardrails/layers.rs`, `crates/core/src/guardrails/evaluate.rs`, `crates/core/src/guardrails/mod.rs` (**MODIFY**)
- `crates/core/src/channel/conn.rs` (**MODIFY**: `guard_caller`)
- `crates/core/tests/us_grd_009.rs` (**CREATE**)

**Reglas**
- Una sola `layers::load` por evaluación (no una segunda); los commits se leen solo si una regla alcanza al actor y las líneas solo si hay una regla de tamaño; nada del mensaje sale de `changes`; sin la capacidad, la decisión es idéntica a la de hoy.

- **Depende:** T003, T004, T005
- **Refs:** D2, D5, D6, D12, D13, D15
- **Aceptación:** `cargo test -p gitraptor-core --test us_grd_009`

### T007 — Cliente: degradado y daemon sin la capacidad

**Objetivo.** `floor_rules_for_everyone` y `degraded` aplican las reglas `everyone` del suelo de las dos claves; el cliente pide `guard.commit-policies` en `connection.accept`.

**Ubicación.**
- `crates/core/src/guardrails/hook.rs` (**MODIFY**)

**Reglas**
- Nunca menos que el degradado (D14); un `ask` en este camino deniega a todos; las reglas `agents` no se aplican sin actor.

- **Depende:** T006
- **Refs:** D10, D14
- **Aceptación:** `cargo test -p gitraptor-cli --test guard_us_grd_009 degraded_mode_applies_the_floor_rules_for_everyone`

### T008 — Mensajes, ejemplo, registro y lista "no se puede impedir"

**Objetivo.** Plantillas en/es del § 5, el ejemplo construido desde la lista de tipos, los textos del registro, `commit-policy-reach` y el texto ampliado de `policy-actor`.

**Ubicación.**
- `apps/cli/src/guard.rs`, `apps/cli/i18n/en/guard.txt`, `apps/cli/i18n/es/guard.txt` (**MODIFY**)
- `crates/policy/src/guard/mod.rs` (**MODIFY**, `not_preventable`)

**Reglas**
- Plantillas fijas, parámetros etiquetados y saneados; ningún texto dice cómo subir el límite ni quitar la política; el ejemplo cumple la gramática de D8 (un test lo comprueba con `commit_format::parse`); los `match` exhaustivos de `apps/cli/src/guard.rs` cubren las reglas y causas nuevas.

- **Depende:** T005, T006
- **Refs:** D10, D11, D16, § 8
- **Aceptación:** `cargo test -p gitraptor-cli --test guard_us_grd_009`

### T009 — Verificar, medir y documentar

**Objetivo.** Suite completa en verde, medición de D15 en un § 10 nuevo, estado de la historia (también las filas 5 y 6 de US-GRD-010), `backlog.md` y `release-status.md`.

**Ubicación.**
- `docs/requirements/features/guardrails/user-stories/US-GRD-009-tamano-diff-formato-commit.md`, `docs/requirements/features/guardrails/user-stories/US-GRD-010-endurecer-sin-relajar.md` (**MODIFY**)
- `docs/requirements/backlog.md`, `docs/requirements/release-status.md` (**MODIFY**)
- Este archivo (**MODIFY**, § 10)

**Reglas**
- No se declara hecho lo que no se verificó: Windows y Linux, pendientes; `release-status.md` se regenera con `node tools/status/release-status.mjs`.

- **Depende:** T001, T007, T008
- **Refs:** D15, D18; Q-GRD-48, Q-GRD-49, Q-GRD-51
- **Aceptación:** `cargo test --workspace`

## Notas del autor

- **Autor**: `nassa-architect:architect`, desde la historia, las reglas, los ADRs, las Dev Specs hermanas y el código de `main` en `3cb912f5` (US-GRD-007 PR-A, PR #268, ya fusionado).
- **G2 (informativo)**: el diff de línea de gix (imara-diff) y el de Git (xdiff) dan el mismo conteo en los casos corrientes; en entradas patológicas (archivos muy grandes con muchas líneas repetidas) Git puede dar una salida no mínima y contar algo distinto. El corpus de T004 fija la igualdad en lo corriente; una diferencia en lo patológico se acepta y se anota en § 10, porque el límite es una guía de revisión y no un control de seguridad.
- **G3 (informativo)**: con `appliesTo: everyone`, la persona no puede hacer un commit directo grande, porque la excepción consciente no cubre el commit (Q-GRD-47) aunque sí el rebase; tendrá que dividirlo o bajar el ámbito de la regla. Lo dice la documentación del esquema (§ 4).
