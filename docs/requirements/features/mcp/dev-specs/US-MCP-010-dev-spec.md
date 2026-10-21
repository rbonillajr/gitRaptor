---
id: DS-US-MCP-010
title: "Dev Spec — Rutas de safe_commit: literales, dentro del worktree y con su modelo de amenazas"
type: dev-spec
status: draft
feature: mcp
domain: MCP
created: 2026-10-21
updated: 2026-10-21
story: US-MCP-010
scope: backend
frontend_surface: false
stack: rust
profile: backend-service
tooling: [cargo]
author: rust-architect
related:
  context: ../context.md
  story: ../user-stories/US-MCP-010-safe-commit-rutas.md
  adrs: [ADR-MCP-001, ADR-CKP-002, ADR-GRP-016]
  api_spec: null
  design_spec: null
  contracts: []
must_read:
  - ../user-stories/US-MCP-010-safe-commit-rutas.md
  - ../user-stories/US-MCP-011-escritura-en-el-worktree-correcto.md
  - ../business-rules.md
  - ../context.md
  - ./US-MCP-009-dev-spec.md
  - ./INF-MCP-001-dev-spec.md
  - ./US-MCP-005-dev-spec.md
  - ../technical-stories/INF-MCP-001-corpus-seguridad-mcp.md
  - ../../motor-local/user-stories/US-GRP-011-worktree-compartido.md
  - ../../guardrails/dev-specs/US-GRD-008-ramas-protegidas-rutas-prohibidas.md
  - ../../../../architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md
  - ../../../../architecture/decisions/ADR-CKP-002-catalogo-operaciones-ejecutor.md
  - ../../../../architecture/decisions/ADR-GRP-016-extension-registro-capacidades.md
  - ../../../../architecture/xplat-pendientes.md
  - ../../../../../AGENTS.md
evidence: []
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
tags: [mcp, safe-commit, rutas, path-traversal, worktree-compartido, modelo-de-amenazas, ola-3]
---

# DS-US-MCP-010 · Rutas de `safe_commit` y su modelo de amenazas

## Contexto rápido

Al terminar, un agente que commitea por MCP solo puede meter en el commit archivos de su worktree que nombra con su ruta exacta, o lo que ya tenía preparado. Una ruta absoluta, UNC, con caracteres de control, con `..`, que nombra `.git` con cualquiera de sus alias, que pasa por un enlace que sale del worktree o que entra en otro repo rechaza la llamada entera antes de que Git la vea. Un archivo nuevo entra solo si se nombra, uno ignorado nunca, y en un worktree con otra sesión viva de agente hay que nombrar las rutas. Hoy no puede: con DS-US-MCP-009, `safe_commit` solo acepta rutas con seguimiento y la defensa de rutas es la forma léxica de `crates/api/src/catalog.rs:check_literal_relative` más lo que Git rechace por su cuenta.

Para eso: la clasificación léxica de rutas en `crates/api`, compartida por `raptor-mcp` y el daemon; la comprobación contra el worktree en el brazo `commit` del daemon; `git add` de los archivos nuevos nombrados dentro del mismo paso protegido, con su vuelta atrás; la regla del worktree compartido; y los casos del corpus de seguridad de INF-MCP-001. El esquema de `safe_commit` lo fija DS-US-MCP-009 (D13) y esta spec no lo toca.

| Término | Qué es aquí |
|---|---|
| Ruta | Un elemento de `paths`: texto del agente, relativo a la raíz del worktree, no confiable |
| Forma léxica | Lo que se comprueba sin leer el disco: longitud, caracteres, componentes |
| Alias de `.git` | Un nombre que algún sistema de archivos trata como `.git`: mayúsculas, puntos o espacios finales (NTFS), `GIT~1` (8.3), sufijo de flujo `:` y los puntos de código que HFS+ ignora |
| Pathspec literal | Cada ruta va a Git como `:(literal)<ruta>` después de `--`, sin magia ni opciones (ADR-CKP-002, Enmienda (2026-10-17, TS-CKP-003)) |
| Sin seguimiento | Archivo del working tree que no está en el índice ni está ignorado |
| Puerta del `.git` | Abrir un worktree solo si el repo lo registra y es dueño de su `.git` (`observe::open_registered_worktree`, #223 I-03) |
| Sesión presente | Sesión viva de otro agente en el mismo worktree; el desarrollador no cuenta (BR-MCP-EDGE-003) |

⚠️ **ASSUMPTION**: no existe `architecture-constitution.md`; rigen `AGENTS.md` y los ADR de `must_read`, como en DS-US-MCP-009.

---

## 📋 Índice

> **Para aprobar:** [Contexto rápido](#contexto-rápido) · [⚠️ Gaps](#gaps-y-violaciones-de-la-constitución) · [🔭 La forma](#la-forma) · [El trabajo de un vistazo](#el-trabajo-de-un-vistazo) · [Modelo de amenazas](#modelo-de-amenazas) · [Decisiones](#decisiones-validadas).
> **Para implementar:** [🚀 Plan](#plan-de-implementación), en orden. Las secciones `_(ref)_` se abren desde la tarea que las cita.

| Sección | Propósito |
|---------|-----------|
| [Contexto rápido](#contexto-rápido) | Qué se construye, por qué, y el glosario |
| [⚠️ Gaps y violaciones de la constitución](#gaps-y-violaciones-de-la-constitución) | Qué impide empezar o liberar |
| [🔭 La forma](#la-forma) | Qué piezas quedan y cómo fluye una ruta |
| [🚀 Plan de implementación](#plan-de-implementación) | T001…T008, en orden |
| ↳ [El trabajo de un vistazo](#el-trabajo-de-un-vistazo) | Las tareas en una tabla, y su orden |
| [Estructura de ficheros](#estructura-de-ficheros) _(ref)_ | Tramos disjuntos de ficheros |
| [Contratos compartidos](#contratos-compartidos) _(ref)_ | Tipos y firmas |
| [Contrato de API](#contrato-de-api) _(ref)_ | Motivos, códigos, numéricos |
| [Modelo de datos](#modelo-de-datos) _(ref)_ | Sin migraciones |
| [Estrategia de pruebas y cobertura](#estrategia-de-pruebas-y-cobertura) _(ref)_ | Escenario → prueba |
| [Gate de seguridad](#gate-de-seguridad) | Checklist pre-merge |
| [Modelo de amenazas](#modelo-de-amenazas) | Amenaza → mitigación → prueba guarda |
| [Fuera de alcance](#fuera-de-alcance) | Not Built (diferido) |
| [Decisiones validadas](#decisiones-validadas) | Lo que fija esta spec, con los ajustes de Arquitecto y PO |
| [Notas del autor](#notas-del-autor) _(ref)_ | Hechos del código y dependencias |

---

## ⚠️ Gaps y violaciones de la constitución

| ID | Qué falta | Severidad | Alcance | Acción | Owner |
|----|-----------|-----------|---------|--------|-------|
| G1 | Los casos que solo existen en Windows (UNC real como cwd, unidad, flujos alternativos `a.txt:x`, nombres reservados `CON`/`NUL`, punto o espacio final en cualquier componente, nombres cortos 8.3 distintos de `GIT~1`, *junctions*) se rechazan por forma léxica con `cfg(windows)`, pero no se pueden ejecutar desde macOS. El canal no existe en Windows (XP-01) | Bloquea liberación | — | Verificarlos en la etapa de validación multiplataforma (fila XP-46, T008) antes de liberar en Windows; macOS y Linux no esperan | Arquitecto |

Todas las tareas son construibles hoy, después de DS-US-MCP-009 en `main`. US-GRP-011 está en `draft`: la regla del worktree compartido se apoya en las sesiones vivas que el detector ya registra en el perfil y no en el registro de un segundo agente (N2).

---

## 🔭 La forma

Una ruta pasa por dos filtros: la forma léxica, igual en `raptor-mcp` y en el daemon, y el contraste con el worktree, solo en el daemon, en `prepare` y otra vez bajo el cerrojo en `run`. Solo lo que pasa los dos llega a Git como pathspec literal.

```mermaid
classDiagram
    class PathRefusal:::nuevo { <<nuevo T002>> }
    class check_commit_paths:::nuevo { <<nuevo T002>> }
    class Selection:::modificado { <<modificado T004>> }
    class WorktreeView:::nuevo { <<nuevo T004>> }
    class CommitStep:::modificado { <<modificado T005>> }
    class Presence:::nuevo { <<nuevo T006>> }
    class DaemonBackend:::modificado { <<modificado T006>> }
    class RepoReader:::existente { <<existente>> }
    check_commit_paths ..> PathRefusal : la misma en raptor-mcp y en el daemon
    Selection --> WorktreeView : coincidencia exacta, en NFC si Git precompone
    WorktreeView --> RepoReader : status, is_ignored, banderas del índice, con gitoxide
    Selection ..> PathRefusal : enlace que sale, otro repo, ignorada, skip-worktree
    CommitStep --> Selection : add_first antes del commit, se deshace si no entra
    DaemonBackend --> Presence : sesiones vivas de otros agentes, ilegible cuenta como compartido
    classDef nuevo fill:#dcfce7,stroke:#16a34a,color:#14532d
    classDef modificado fill:#fef3c7,stroke:#d97706,color:#78350f
    classDef existente fill:#f1f5f9,stroke:#94a3b8,color:#334155
```

🟩 nuevo · 🟨 modificado · ⬜ existente. La arista de `Selection` a `WorktreeView` resuelve mayúsculas sin plegar nada y la normalización Unicode como la resuelve Git: una ruta vale solo si es la misma entrada que Git ya conoce o que gitoxide lista en el disco.

**Cómo fluye:**

```mermaid
sequenceDiagram
    autonumber
    participant Ag as agente · Claude Code
    participant Mcp as raptor-mcp
    participant Ex as ejecutor del daemon
    participant Sel as commit_paths
    participant Gt as git
    Ag->>Mcp: safe_commit {message, paths}
    Mcp->>Mcp: tope de rutas y check_commit_path por ruta, sin llamar al motor (T003)
    Mcp->>Ex: operation.prepare (DS-US-MCP-009)
    Ex->>Ex: OperationArgs::parse, la misma forma léxica (T002)
    Ex->>Ex: otra sesión viva de agente y todo lo preparado, rechazo (T006)
    Ex->>Sel: select con WorktreeView, enlaces, otro repo, ignorados (T004)
    Sel-->>Ex: Selection en la huella del plan
    Ex->>Sel: run, bajo el cerrojo, la misma comprobación (T004)
    Ex->>Gt: git add -- :(literal) de los nuevos nombrados (T005)
    Ex->>Gt: git commit -F - -- :(literal) rutas (DS-US-MCP-009)
    Gt-->>Ex: punta sin mover, git rm --cached de lo añadido, también tras time-limit o cancelación (T005)
```

El paso 8 cierra la ventana entre `prepare` y `run`: un enlace o un `.git` puestos entre medias cambian la `Selection` o los hechos, y el plan acaba en `state-changed`. El paso 11 deja el índice como estaba cuando el commit no entra.

---

## 🚀 Plan de implementación

> Orden topológico (`Depende:`). Rutas relativas a la raíz del repo. Requiere DS-US-MCP-009 en `main`: T004 y T005 modifican archivos que crea esa spec.

### El trabajo de un vistazo

Un camino trazador y tres endurecimientos, más el cierre: T001 a T004 llevan de extremo a extremo los escenarios 1 a 3 (suite en rojo, forma léxica, `raptor-mcp`, contraste con el worktree); T005 y T006 añaden los archivos nuevos y el worktree compartido; T007 amplía el corpus; T008 es documentación.

| # | Tarea | Depende | Aterriza en |
|---|---|---|---|
| T001 | Escribir la suite de aceptación de rutas en rojo | — | `apps/cli/tests/` |
| T002 | Clasificar la forma léxica de las rutas en `crates/api` | — | `crates/api/src/` |
| T003 | Validar las rutas en `raptor-mcp` antes de llamar al daemon | T002 | `apps/mcp/src/` |
| T004 | Comprobar las rutas contra el worktree en el daemon | T002 | `crates/core/src/executor/ops/commit_paths.rs`, `crates/git/src/paths.rs` |
| T005 | Añadir los archivos sin seguimiento nombrados y deshacerlo si el commit no entra | T004 | `crates/git/src/user_ops.rs`, `crates/core/src/executor/ops/commit.rs` |
| T006 | Exigir rutas explícitas en un worktree compartido | T004 | `crates/core/src/executor/`, `crates/core/src/timemachine/protected/` |
| T007 | Ampliar el corpus de seguridad con el modelo de amenazas de rutas | T003, T004 | `crates/testkit/src/mcp_corpus/`, `apps/cli/tests/mcp_corpus/` |
| T008 | Aplicar las enmiendas y cerrar los pendientes multiplataforma | T003, T005, T006, T007 | `docs/` |

### En qué orden

Dos frentes tras el contrato (servidor y daemon) que convergen en el corpus; el daemon se abre en dos ramas paralelas (archivos nuevos y worktree compartido).

```mermaid
graph LR
    T002 --> T003
    T002 --> T004
    T004 --> T005
    T004 --> T006
    T003 --> T007
    T004 --> T007
    T003 --> T008
    T005 --> T008
    T006 --> T008
    T007 --> T008
```

### T001 — Escribir la suite de aceptación de rutas en rojo

**Objetivo.** Un test por escenario de la historia, por proceso real, contra el JSON del cable.

**Ubicación.** `apps/cli/tests/mcp_safe_commit_paths.rs` (**CREATE**)

**Reglas**
- Copiar el arnés de `apps/cli/tests/mcp_safe_commit.rs` (DS-US-MCP-009 T001) sin editarlo. `#![cfg(target_os = "macos")]` y `#![cfg(debug_assertions)]`.
- Rechazos con `LANG=es_ES.UTF-8` y el texto literal de la historia: "ruta fuera del worktree", "ruta no válida", "demasiadas rutas", "ruta no encontrada", "la ruta .env está ignorada por Git; no se añade", "indica tus rutas: el worktree es compartido".
- La ruta rechazada vuelve en `params.path` como `{"untrusted": …}`.
- Cada rechazo comprueba con `gitraptor_testkit::fingerprint` que el repo, el worktree y su `.git` no cambiaron, y con `git rev-list --count` que no hay commit.
- Worktree compartido: dos `raptor-fake-agent` vivos con su cwd en `shop-feat-a`, esperando con `DEADLINE` a que `raptor status --json` muestre dos sesiones. Si el detector las funde en una, el test siembra la segunda sesión con el almacén del perfil temporal (N2) y lo dice en un comentario. El desarrollador con la TUI abierta en ese worktree no lo vuelve compartido.

- **Depende:** —
- **Refs:** US-MCP-010 (Gherkin), DS-US-MCP-009 T001
- **Aceptación:** los nueve tests "e2e" de § Estrategia de pruebas existen y fallan en rojo con el código de DS-US-MCP-009

### T002 — Clasificar la forma léxica de las rutas en `crates/api`

**Objetivo.** Una sola función, usada por `raptor-mcp` y por `OperationArgs::parse`, que dice si una ruta tiene forma válida y, si no, por qué.

**Ubicación.**
- `crates/api/src/commit_paths.rs` (**CREATE**): `PathRefusal`, `check_commit_path`, `check_commit_paths`, `is_dotgit_alias`, `is_git_special_name`
- `crates/api/src/catalog.rs` (**MODIFY**): `OperationArgs::check` de `Commit` llama a `check_commit_paths`; se retira `check_literal_relative`; cuatro variantes de `RejectReason`
- `crates/api/src/mcp_view.rs` (**MODIFY**): cinco variantes de `McpToolError`
- `crates/api/tests/us_mcp_010_paths.rs` (**CREATE**)

**Pasos**
1. Escribir primero `us_mcp_010_paths.rs` en rojo con la tabla de § Contrato de API completa.
2. `check_commit_path(p)`, en este orden, el primero que falla decide:
   2.1 Vacía, más de `MAX_PATH_BYTES` bytes (antes de recorrer caracteres) o con NUL → `Invalid`.
   2.2 Algún `is_forbidden_char` → `Invalid`.
   2.3 Empieza por `/`, contiene `\` en cualquier posición, empieza por una letra ASCII seguida de `:`, o empieza por `-` → `Invalid`.
   2.4 Partida por `/`: un componente vacío (`a//b`, `/` final), `.` → `Invalid`; `..` → `Outside`; un componente para el que `is_dotgit_alias` es `true` → `Outside`, en todas las plataformas.
   2.5 `cfg(windows)`: además `:` en cualquier posición, un componente que acaba en `.` o en espacio, o cuyo nombre sin extensión es `CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9` (sin distinguir mayúsculas) → `Invalid`.
   2.6 ⛔2.1 `:(glob)**` y `:!x` pasan la forma léxica: son nombres literales. Si se rechazan aquí, el escenario 3 ("ruta no encontrada") pasa por el motivo equivocado y se pierde la prueba de que Git las recibe como literales.
3. `is_dotgit_alias(component)`: quitar los puntos de código que HFS+ ignora (la lista `HFS_IGNORABLE` de `crates/git/src/tm_write/tree_path.rs:45`, copiada literal), pasar a minúsculas, cortar en el primer `:`, quitar puntos y espacios finales; `true` si queda `.git` o `git~1`. `is_git_special_name(component)`: el mismo plegado; `true` si queda `.gitmodules`, `gitmod~1`, `.gitattributes`, `gitatt~1`, `.gitignore` o `gitign~1`.
   3.1 ⛔2.2 `is_forbidden_char` no cubre U+206A a U+206F, que HFS+ ignora: sin la lista copiada, `.g\u{206A}it` pasa y en macOS abre `.git` (CVE-2014-9390).
4. `check_commit_paths(paths)`: más de `MAX_COMMIT_PATHS` → `TooMany` sin mirar ninguna; si no, la primera ruta que falla rechaza todas.
5. `OperationArgs::check`: `PathRefusal` → `Invalid::new("paths", refusal.as_str())`. `RejectReason` gana `PathInvalid`, `PathOutsideWorktree`, `PathIgnored` y `SharedWorktreeNeedsPaths`, solo para conexiones con `CAP_OPERATION_COMMIT`.

> **Nota técnica.** `gix-validate` 0.13 está en `Cargo.lock`, pero solo como dependencia de `gix`; `crates/api` no depende de `gix` y no puede usarlo sin tocar `Cargo.toml`. Por eso las reglas se copian de `tree_path.rs`, que ya las aplica a la Time Machine, y una prueba de `crates/core` compara las dos sobre el mismo corpus (T004).

> **Nota técnica.** En Unix, `\` y `C:` son nombres de archivo legales. Se rechazan en todas las plataformas para que la misma petición dé el mismo resultado en macOS y en Windows (D2); un archivo así no se puede commitear por MCP.

- **Depende:** —
- **Refs:** BR-MCP-VAL-001, ADR-MCP-001 § 4.2 y § 9 (MCP05), ADR-GRP-016 § 3
- **Aceptación:** `cargo test -p gitraptor-api --test us_mcp_010_paths` en verde, y `cargo test -p gitraptor-api` sigue en verde (incluidas las aserciones de `catalog.rs` que hoy rechazan `../etc/passwd`, `/abs`, `--amend` y `a/.git/config`)
- **Guard ⛔2.1:** `pathspec_magic_is_a_literal_name_for_the_lexical_check`
- **Guard ⛔2.2:** `dot_git_aliases_are_outside_on_every_platform`

### T003 — Validar las rutas en `raptor-mcp` antes de llamar al daemon

**Objetivo.** Una ruta con forma no válida, o más rutas que el tope, se rechazan en `raptor-mcp` con su código, sin conectar con el daemon.

**Ubicación.**
- `apps/mcp/src/safe_commit_paths.rs` (**CREATE**): `check`
- `apps/mcp/src/safe_commit.rs` (**MODIFY**, una llamada a `safe_commit_paths::check` en `call`, antes de `Engine::commit`)
- `apps/mcp/src/main.rs` (**MODIFY**, una línea `mod safe_commit_paths;`)
- `apps/mcp/src/messages.rs` (**MODIFY**): plantillas en/es de los cinco códigos
- `apps/mcp/src/engine.rs` (**MODIFY**): filas de `rejected_refusal` para los cuatro motivos nuevos y para `INVALID_PARAMS` de `paths` con `too-many`

**Reglas**
- 257 rutas o más → `too-many-paths` con `params.max: 256`, nunca `invalid-params`: el esquema de DS-US-MCP-009 no declara `maxItems` y `arguments()` no cuenta las rutas.
- `PathRefusal::Invalid` → `invalid-path`; `Outside` → `path-outside-worktree`, con `params.path` como `{"untrusted": …}` recortado a 1.024 bytes.
- El orden de los rechazos de la llamada no cambia: el esquema (`invalid-params`), después el mensaje (`invalid-text`), después las rutas.
- Las filas del daemon: `path-invalid` → `invalid-path`, `path-outside-worktree` → `path-outside-worktree`, `path-ignored` → `path-ignored` con `params.path`, `shared-worktree-needs-paths` → `other-session-present`; `INVALID_PARAMS` con `field: "paths"` y `too-many` → `too-many-paths` (un cliente que habló directo con el daemon).

- **Depende:** T002
- **Refs:** ADR-MCP-001 § 4.2 y § 5; DS-US-MCP-005 D4
- **Aceptación:** `cargo test -p gitraptor-mcp` en verde, con `a_bad_path_never_reaches_the_engine`, `two_hundred_fifty_seven_paths_are_too_many_paths`, `the_refused_path_travels_as_untrusted` y `the_spanish_texts_say_what_the_story_says_for_paths`

### T004 — Comprobar las rutas contra el worktree en el daemon

**Objetivo.** `commit_paths::select` acepta solo rutas que el worktree tiene, dentro de él y de este repo, y añade los archivos sin seguimiento nombrados a `Selection.add_first`.

**Ubicación.**
- `crates/core/src/executor/ops/commit_paths.rs` (**MODIFY**): `WorktreeView`, `select` con el contraste, `Selection.add_first`
- `crates/core/src/executor/ops/commit.rs` (**MODIFY**, `plan_op` construye `WorktreeView` y la pasa a `select`)
- `crates/git/src/paths.rs` (**MODIFY**): `precomposed`, sobre el `unicode-normalization` que ya usa el crate
- `crates/core/tests/us_mcp_010.rs` (**CREATE**)

**Pasos**
1. Escribir primero `us_mcp_010.rs` en rojo, con el rig de `crates/core/tests/us_mcp_009.rs` copiado y la pareja de repos de `crates/core/tests/tm_untrusted_git.rs:Pair` copiada.
2. `WorktreeView::read(root, reader)`: el `Status` de gitoxide, las entradas del índice y de `HEAD` con sus banderas (`skip-worktree`, `assume-unchanged`, gitlink), el listado de archivos sin seguimiento uno a uno, y `core.precomposeUnicode` del repo.
3. Con `core.precomposeUnicode = true`, la ruta pedida y los nombres del listado sin seguimiento se comparan en NFC (`gitraptor_git::paths::precomposed`), como hace Git; el índice ya está en NFC. A Git se le pasa la forma NFC. Sin esa opción, bytes exactos.
4. Para cada ruta, en este orden:
   4.1 Cada componente padre que existe, con `symlink_metadata` desde la raíz: enlace → `PathOutsideWorktree`; carpeta que contiene una entrada `.git` (archivo o carpeta) → `PathOutsideWorktree`.
   4.2 El último componente: carpeta → `PathInvalid` (D8). Enlace llamado como `is_git_special_name` → `PathInvalid` (D5). Otro enlace: su destino con `read_link`, resuelto léxicamente contra su carpeta, absoluto, fuera de la raíz o dentro de `.git` → `PathOutsideWorktree`; si no, la ruta sigue (se commitea el enlace, no su destino).
   4.3 Igual a una entrada del índice o de `HEAD` → con seguimiento (también si se borró del disco). Una entrada gitlink, o con `skip-worktree` o `assume-unchanged` → `PathInvalid`.
   4.4 Si no, igual a un archivo del listado sin seguimiento → `add_first`.
   4.5 Si no, existe en el disco y `RepoReader::is_ignored(ruta, false)` es `true` → `PathIgnored`.
   4.6 Si no → `PathNotFound`.
   4.7 ⛔4.1 Nunca plegar mayúsculas para buscar la ruta. En APFS, `SRC/A.rs` abre `src/a.rs`; si se acepta, `git add` crea una segunda entrada del índice para el mismo archivo.
   4.8 ⛔4.3 Una entrada con `skip-worktree` (o fuera del cono de un *sparse checkout*) no está en el disco aunque siga en el índice: commitearla con `--only` registraría su borrado.
5. "Todo lo preparado" no mira el listado sin seguimiento: `add_first` queda vacío (BR-MCP-EDGE-010).
6. Ninguna ruta con cambio → `NothingToCommit`, como en DS-US-MCP-009.
7. `run` vuelve a planear bajo el cerrojo (ADR-CKP-002 § 5): la `Selection` entra en `expected` y en la huella, así que un enlace o un `.git` cambiados entre `prepare` y `run` dan `state-changed`.
   7.1 ⛔4.2 El `git_dir` del commit sigue saliendo de `DaemonBackend::facts` (guarda 3.3 de DS-US-MCP-009). Esta tarea no lee el `.git` del worktree para nada.
8. Prueba cruzada: `is_dotgit_alias` de `crates/api` y `tree_path::is_dotgit` de `crates/git` dan lo mismo sobre el corpus de nombres de `tree_path.rs` y los de § Modelo de amenazas (A7, A20).

> **Nota técnica.** `RepoReader::is_ignored` (`crates/git/src/reader.rs:395`) aplica las reglas del repo y del usuario con gitoxide. Un archivo con seguimiento nunca cuenta como ignorado en Git, por eso 4.5 va después de 4.3.

> **Nota técnica.** En HFS+ el sistema de archivos devuelve nombres en NFD y Git con `core.precomposeUnicode` los guarda en NFC; `crates/git/src/tm_write/tree_path.rs:fold` ya trata la misma colisión para la Time Machine. Mayúsculas y minúsculas no se pliegan nunca (4.7).

> **Nota técnica.** ⚠️ **ASSUMPTION**: `RepoReader::status().untracked` lista archivos uno a uno y no colapsa una carpeta nueva en `dir/`. Si la colapsa, el listado sale de `CaptureReader::untracked` (`crates/git/src/capture.rs:280`), que recorre archivos. Lo prueba `a_new_file_in_a_new_folder_is_committed_when_named`.

- **Depende:** T002
- **Refs:** BR-MCP-VAL-001, BR-MCP-EDGE-010, ADR-CKP-002 § 1 y § 5, #223 I-03
- **Aceptación:** `cargo test -p gitraptor-core --test us_mcp_010` en verde, con `an_nfd_name_matches_its_nfc_entry_when_git_precomposes`, `dotgit_rules_match_the_tree_path_rules` y `a_symlink_named_gitmodules_is_invalid`; `us_mcp_009` sigue en verde
- **Guard ⛔4.1:** `a_path_with_other_case_is_not_found`
- **Guard ⛔4.2:** `a_dot_git_pointed_at_another_repo_after_prepare_commits_nothing`
- **Guard ⛔4.3:** `a_skip_worktree_or_sparse_entry_is_invalid_and_commits_no_deletion`

### T005 — Añadir los archivos sin seguimiento nombrados y deshacerlo si el commit no entra

**Objetivo.** Un archivo nuevo nombrado entra en el commit; si el commit no entra, por rechazo, `time-limit` o cancelación, el índice queda como estaba.

**Ubicación.**
- `crates/git/src/user_ops.rs` (**MODIFY**): `UserOp::AddPaths`, `UserOp::UnstagePaths`
- `crates/core/src/executor/ops/commit.rs` (**MODIFY**, `CommitStep::run`)
- `crates/core/tests/us_mcp_010_untracked.rs` (**CREATE**)

**Pasos**
1. `UserOp::AddPaths { paths }` → `add -- :(literal)p…`; `UserOp::UnstagePaths { paths }` → `rm --cached --quiet --ignore-unmatch -- :(literal)p…`. Sin `-f`, `-A`, `--all` ni `-u`; lista vacía → error al construir.
   1.1 ⛔5.1 Nunca `add -f`: sin `-f`, Git rechaza un ignorado y es la segunda barrera de 4.5 de T004.
2. `CommitStep::run`: con `add_first` no vacío, `AddPaths` por `run_git` antes del commit; si falla, `StepError::unchanged(StepFailure::Unchanged)` sin lanzar el commit.
3. El commit lleva todas las rutas, también las de `add_first`.
4. Si la punta no se movió (DS-US-MCP-009, paso 6.2 de T003), por cualquier causa (rechazo de un hook, `time-limit` o cancelación): `UnstagePaths` de `add_first`, con un `CancelToken` nuevo para que la interrupción del commit no la corte. Si `UnstagePaths` falla → `StepFailure::Changed` (`failed-changed`, deshacer con el previo).
   4.1 ⛔5.2 `rm --cached` solo de `add_first`, nunca de las rutas con seguimiento: quitaría del índice trabajo preparado por el agente o por el desarrollador.
   4.2 ⛔5.3 Si el `git commit` interrumpido deja un `index.lock` huérfano, `rm --cached` falla: el resultado es `failed-changed`, nunca `failed-unchanged`, y el lock no se borra (ADR-CKP-002 § 6: un lock de Git nunca se quita).

> **Nota técnica.** `rm --cached` de una ruta que no estaba en el índice antes de `add` devuelve el índice a su estado previo con y sin commits en la rama (rama sin nacer incluida); `reset -- <ruta>` necesita `HEAD`.

- **Depende:** T004
- **Refs:** BR-MCP-EDGE-010, ADR-CKP-002 § 6 y § 11, NFR-01
- **Aceptación:** `cargo test -p gitraptor-git` y `cargo test -p gitraptor-core --test us_mcp_010_untracked` en verde, con `a_new_file_enters_only_when_named`, `a_rejected_commit_leaves_the_index_as_it_was`, `a_timed_out_or_cancelled_commit_unstages_what_it_added`, `a_new_file_in_a_new_folder_is_committed_when_named` y `user_ops::tests::add_beyond_a_symbolic_link_is_refused_by_git`
- **Guard ⛔5.1:** `user_ops::tests::add_paths_never_forces`
- **Guard ⛔5.2:** `a_rejected_commit_keeps_what_was_staged_before`
- **Guard ⛔5.3:** `an_orphan_index_lock_after_a_time_limit_is_failed_changed`

### T006 — Exigir rutas explícitas en un worktree compartido

**Objetivo.** Con otra sesión viva de agente en el worktree, "todo lo preparado" se rechaza con la acción; con rutas, el commit sigue y no lleva aviso. El desarrollador y una sesión de un agente que murió no cuentan.

**Ubicación.**
- `crates/core/src/timemachine/protected/scope.rs` (**MODIFY**): `ProtectedBackend::live_sessions`, con valor por defecto
- `crates/core/src/timemachine/protected/backend.rs` (**MODIFY**): `DaemonBackend::with_presence` y su `live_sessions`
- `crates/core/src/executor/mod.rs` (**MODIFY**): la regla en `plan`, y `commit` fuera del aviso `ActiveSession`
- `crates/core/src/daemon/serve.rs` (**MODIFY**, una llamada `with_presence`)
- `crates/core/tests/us_mcp_010_shared.rs` (**CREATE**)

**Pasos**
1. `live_sessions(&self, repo) -> Result<Vec<String>, RejectReason>`: por defecto `Ok(vec![])`, documentado como "solo para dobles de prueba". `DaemonBackend` usa su `Presence` (§ Tipos).
2. Una sesión cuenta si: es de un agente (tiene `agent_kind`; la del desarrollador y la de la TUI no), `end_cause IS NULL`, su ruta canónica es la del worktree (`profile::Store::sessions_for_worktree`), y está viva: su proceso `(pid, inicio)` existe según `ProcSource::read`, o, si la sesión no guarda proceso, el detector la vio en los últimos `PRESENCE_TTL`.
3. `executor::plan`, con `op == Commit`, capa `mcp`, `args.paths == None`: si `live_sessions` falla o tiene una sesión distinta de la del solicitante → `RejectReason::SharedWorktreeNeedsPaths`.
   3.1 ⛔6.1 Una presencia ilegible cuenta como compartido. Si cuenta como "nadie", un agente commitea lo que otro tenía preparado.
   3.2 ⛔6.2 Una sesión sin fin cuyo proceso ya no existe no cuenta: si contara, un agente que se cayó bloquearía "todo lo preparado" en ese worktree para siempre.
4. `prepare`: `commit` nunca recibe `WarningCode::ActiveSession` (ADR-MCP-001 § 4.3: `commit` no lleva avisos).
5. `serve.rs`: `DaemonBackend::new(…).with_presence(presence)`, con la presencia leída de los almacenes del perfil por repo que usa `daemon/sessions.rs`. ⚠️ **ASSUMPTION**: si el almacén no se puede compartir entre hilos, la presencia abre el del repo en solo lectura en cada llamada (como `crates/core/tests/profile_read_only.rs`).

> **Nota técnica.** ⚠️ **ASSUMPTION**: el detector de US-GRP-007 cierra la sesión (`end_cause`) cuando su proceso desaparece, y guarda la identidad `(pid, inicio)` del proceso detectado (`detection_key`). El paso 2 no depende de lo primero: comprueba la vida del proceso él mismo.

- **Depende:** T004
- **Refs:** BR-MCP-EDGE-003, BR-MCP-ELIG-002, ADR-CKP-002 § 1 (`commit`), ADR-MCP-001 § 4.3
- **Aceptación:** `cargo test -p gitraptor-core --test us_mcp_010_shared` en verde, con `all_staged_in_a_shared_worktree_is_refused_with_the_action`, `named_paths_in_a_shared_worktree_commit_only_them`, `the_developers_session_does_not_make_it_shared` y `a_commit_never_carries_the_active_session_warning`
- **Guard ⛔6.1:** `an_unreadable_presence_counts_as_shared`
- **Guard ⛔6.2:** `a_crashed_agents_session_does_not_block_all_staged`

### T007 — Ampliar el corpus de seguridad con el modelo de amenazas de rutas

**Objetivo.** Cada fila de § Modelo de amenazas que el atacante puede provocar por MCP tiene su caso en el corpus de INF-MCP-001, y el KPI sigue en 100 %.

**Ubicación.**
- `crates/testkit/src/mcp_corpus/case.rs` (**MODIFY**): `Setup.files` y `Setup.dot_git_to`
- `crates/testkit/src/mcp_corpus/schema.rs` (**MODIFY**): las claves nuevas
- `apps/cli/tests/mcp_corpus/machine.rs` (**MODIFY**): crear los archivos y reescribir el `.git`
- `apps/cli/tests/mcp_corpus/cases/` (**MODIFY**): los archivos `safe-commit-path-*.json` de § Modelo de amenazas

**Reglas**
- `setup.files`: lista de `{at, state}` con `state` en `modified`, `untracked`, `ignored` (escribe el archivo y su línea en `.gitignore`), `staged`, `skip-worktree`. `setup.dot_git_to`: el worktree de la sesión cuyo `.git` se reescribe para apuntar a `other_repo`.
- Nivel `server` para las formas léxicas (todas las plataformas salvo las de `cfg(windows)`, que declaran `platforms: ["windows"]` y `pending: XP-46`). Nivel `engine` para el resto (macOS y Linux, `pending: XP-42`).
- Cada caso con contenido de otro repo declara `forbidden` con `{other_repo}`: ni su ruta ni su contenido salen por stdout o stderr.
- `every_case_file_is_valid_unique_and_named_after_its_id` sigue en verde con las claves nuevas.

- **Depende:** T003, T004
- **Refs:** DS-INF-MCP-001 D1 y D2; ADR-MCP-001 § 9
- **Aceptación:** `cargo test -p gitraptor-testkit` y `cargo test -p gitraptor-cli --test mcp_security_corpus` en verde

### T008 — Aplicar las enmiendas y cerrar los pendientes multiplataforma

**Objetivo.** Documentos, contrato del canal y pendientes al día; no toca código. La historia la enmienda el orquestador en su PR.

**Ubicación.**
- `docs/architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md` (**MODIFY**, Enmienda (2026-10-21, US-MCP-010))
- `docs/requirements/features/mcp/business-rules.md` (**MODIFY**, BR-MCP-VAL-001, BR-MCP-EDGE-003 y BR-MCP-EDGE-010)
- `docs/architecture/design/api-contract-ipc.md` (**MODIFY**)
- `docs/architecture/xplat-pendientes.md` (**MODIFY**, fila XP-46)
- `docs/requirements/features/mcp/dev-specs/US-MCP-010-dev-spec.md` (**MODIFY**, "Estado de la implementación")

**Reglas**
- ADR-MCP-001, Enmienda: § 4.3, la frase "otra sesión presente y trabajo ajeno ya son precondición o control y se rechazan" pasa a admitir las rutas nombradas en `commit` (D10 b); § 9, fila MCP05 apunta a § Modelo de amenazas de esta spec; § 5, los cinco códigos nuevos; § 4.2, "rutas relativas a la raíz del worktree, nunca al cwd".
- `business-rules.md` (PO): BR-MCP-VAL-001, "archivos, no carpetas ni submódulos", "relativas a la raíz del worktree, nunca al cwd", tope real de 256, `\` y unidad rechazados en todas las plataformas, alias de `.git` (D2, D8, D13); BR-MCP-EDGE-003, "sesión = de otro agente, viva; el desarrollador no cuenta; ilegible = compartido" (D10); BR-MCP-EDGE-010, "un archivo nuevo se nombra uno a uno; una carpeta no lo añade" (D8).
- XP-46: lo de § 9.5 que no se verifica en macOS y G1.
- No editar `docs/ARTIFACTS.md`. El `status` de la historia lo cambia el PR.

- **Depende:** T003, T005, T006, T007
- **Refs:** § Decisiones validadas, `AGENTS.md`
- **Aceptación:** revisión de Arquitecto y PO; los nueve tests de T001 en verde en macOS

---

> Las secciones siguientes son de referencia. Se abren desde la tarea que las cita, no se leen en orden.

## Estructura de ficheros

Seis tramos disjuntos. Los archivos de `crates/core/src/executor/ops/` y `executor/mod.rs` los crea DS-US-MCP-009; aquí se modifican.

### Tramo A — forma léxica (T002)

- `crates/api/src/commit_paths.rs`
- `crates/api/src/catalog.rs`
- `crates/api/src/mcp_view.rs`
- `crates/api/tests/us_mcp_010_paths.rs`

### Tramo B — servidor MCP (T003)

- `apps/mcp/src/safe_commit_paths.rs`
- `apps/mcp/src/safe_commit.rs`
- `apps/mcp/src/main.rs`
- `apps/mcp/src/messages.rs`
- `apps/mcp/src/engine.rs`

### Tramo C — brazo `commit` y Git (T004 y T005)

- `crates/core/src/executor/ops/commit_paths.rs`
- `crates/core/src/executor/ops/commit.rs`
- `crates/git/src/paths.rs`
- `crates/git/src/user_ops.rs`
- `crates/core/tests/us_mcp_010.rs`
- `crates/core/tests/us_mcp_010_untracked.rs`

### Tramo D — worktree compartido (T006)

- `crates/core/src/timemachine/protected/scope.rs`
- `crates/core/src/timemachine/protected/backend.rs`
- `crates/core/src/executor/mod.rs`
- `crates/core/src/daemon/serve.rs`
- `crates/core/tests/us_mcp_010_shared.rs`

### Tramo E — aceptación y corpus (T001 y T007)

- `apps/cli/tests/mcp_safe_commit_paths.rs`
- `crates/testkit/src/mcp_corpus/case.rs`
- `crates/testkit/src/mcp_corpus/schema.rs`
- `apps/cli/tests/mcp_corpus/machine.rs`
- `apps/cli/tests/mcp_corpus/cases/safe-commit-path-*.json`

### Tramo F — documentación (T008)

- `docs/architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md`
- `docs/requirements/features/mcp/business-rules.md`
- `docs/architecture/design/api-contract-ipc.md`
- `docs/architecture/xplat-pendientes.md`
- `docs/requirements/features/mcp/dev-specs/US-MCP-010-dev-spec.md`

---

## Contratos compartidos

### Tipos y datos compartidos

```rust
// crates/api/src/commit_paths.rs (T002)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PathRefusal {
    Invalid,   // "invalid": absolute, UNC, drive, backslash, control chars, leading '-', not canonical
    Outside,   // "outside": "..", ".git" and its aliases
    TooMany,   // "too-many"
}
/// Code points HFS+ ignores, copied from crates/git/src/tm_write/tree_path.rs:HFS_IGNORABLE.
const HFS_IGNORABLE: &[char] = &[ /* U+200C..=U+200F, U+202A..=U+202E, U+206A..=U+206F, U+FEFF */ ];

// crates/api/src/catalog.rs (T002): only with `operation.commit`
pub enum RejectReason { /* … */
    PathInvalid,               // "path-invalid": a directory, a gitlink, a skip-worktree entry, a special-name symlink, or a lexical refusal
    PathOutsideWorktree,       // "path-outside-worktree"
    PathIgnored,               // "path-ignored"
    SharedWorktreeNeedsPaths,  // "shared-worktree-needs-paths"
}

// crates/api/src/mcp_view.rs (T002)
pub enum McpToolError { /* … */
    InvalidPath,           // "invalid-path", params.path untrusted
    PathOutsideWorktree,   // "path-outside-worktree", params.path untrusted
    TooManyPaths,          // "too-many-paths", params.max
    PathIgnored,           // "path-ignored", params.path untrusted
    OtherSessionPresent,   // "other-session-present"
}

// crates/core/src/executor/ops/commit_paths.rs (T004; Selection was created by DS-US-MCP-009)
pub(super) struct Selection {
    pub paths: Vec<String>,
    pub all_staged: bool,
    /// Named untracked files, added before the commit and unstaged if it does not land.
    pub add_first: Vec<String>,
}
// crates/core/src/executor/ops/commit.rs (T005; created by DS-US-MCP-009): CommitStep runs AddPaths before the commit.
/// What the worktree knows of its paths, read once per plan with gitoxide.
pub(super) struct WorktreeView { /* status, tracked: BTreeMap<String, EntryFlags>, untracked: BTreeSet<String>, precompose: bool, root */ }

// crates/git/src/user_ops.rs (T005)
pub enum UserOp { /* … */
    /// `add -- :(literal)<p>…`; never `-f`, `-A` nor `-u`.
    AddPaths { paths: Vec<String> },
    /// `rm --cached --quiet --ignore-unmatch -- :(literal)<p>…`.
    UnstagePaths { paths: Vec<String> },
}

// crates/core/src/timemachine/protected/backend.rs (T006)
/// Live session ids of other agents in the worktree at a canonical root; `None` when unreadable.
pub type Presence = Arc<dyn Fn(&Path) -> Option<Vec<String>> + Send + Sync>;
pub const PRESENCE_TTL: Duration = Duration::from_secs(600); // ⚠️ ASSUMPTION, only for sessions without a process identity
```

### Ciclos de vida (DI)

| Servicio / componente | Ámbito / ciclo de vida | Razón |
|---|---|---|
| `WorktreeView` | Uno por plan (`prepare` y otra vez en `run`) | La segunda lectura, bajo el cerrojo, es la que cuenta |
| `Presence` | Uno por daemon, en `DaemonBackend` | Lee el perfil y la vida de los procesos en cada plan; no guarda estado |

### Firmas del stack

```rust
// crates/api/src/commit_paths.rs
pub fn check_commit_path(path: &str) -> Result<(), PathRefusal>;
/// TooMany before looking at any path; otherwise the first refusal, with its index.
pub fn check_commit_paths(paths: &[String]) -> Result<(), (usize, PathRefusal)>;
/// `.git` on some file system: case, NTFS trailing dots and spaces, `GIT~1`, stream suffix, HFS+ ignorables.
pub fn is_dotgit_alias(component: &str) -> bool;
/// `.gitmodules`, `.gitattributes`, `.gitignore` and their aliases, with the same folding.
pub fn is_git_special_name(component: &str) -> bool;

// crates/git/src/paths.rs
/// NFC form, as Git stores names with `core.precomposeUnicode`.
pub fn precomposed(path: &str) -> String;

// crates/core/src/executor/ops/commit_paths.rs (replaces the DS-US-MCP-009 signature)
impl WorktreeView { pub(super) fn read(root: &Path, reader: &RepoReader) -> Result<Self, RejectReason>; }
pub(super) fn select(args: &CommitArgs, view: &WorktreeView) -> Result<Selection, (RejectReason, Option<String>)>;

// crates/core/src/timemachine/protected/scope.rs
pub trait ProtectedBackend { /* … */
    /// Live sessions of other agents in the request's worktree. Default for test doubles only.
    fn live_sessions(&self, repo: &RepoHandle) -> Result<Vec<String>, RejectReason> { let _ = repo; Ok(Vec::new()) }
}
// crates/core/src/timemachine/protected/backend.rs
impl DaemonBackend { pub fn with_presence(self, presence: Presence) -> Self; }

// apps/mcp/src/safe_commit_paths.rs
pub fn check(paths: Option<&[String]>) -> Result<(), ToolRefusal>;
```

`select` devuelve la ruta rechazada junto al motivo; el canal la pone en `RejectedData` y `raptor-mcp` en `params.path`.

---

## Contrato de API

Sin llamadas nuevas: cambia el contenido aceptado de `args.paths` de `operation.prepare {operation: "commit"}` (DS-US-MCP-009) y aparecen cuatro motivos bajo `OPERATION_REJECTED`, con `data.path` (untrusted) cuando aplica.

| Ruta (ejemplo) | Dónde se rechaza | Motivo del canal | `code` de la herramienta | Mensaje (es) |
|---|---|---|---|---|
| `/etc/passwd`, `\\servidor\compartido\a.rs`, `C:x`, `a\b`, `-f`, `a//b`, `./a`, `a/`, con controles | `raptor-mcp` y daemon, forma léxica | `path-invalid` | `invalid-path` | "Ruta no válida: {path}." |
| `../otro-repo/secreto`, `a/../../x`, `.git/config`, `sub/.GIT/hooks/x`, `.git./x`, `.git /x`, `GIT~1/config`, `.g\u{200C}it/x` | ídem | `path-outside-worktree` | `path-outside-worktree` | "Ruta fuera del worktree: {path}." |
| 257 rutas | ídem | `INVALID_PARAMS` (`paths`, `too-many`) | `too-many-paths` | "Demasiadas rutas: como mucho 256." |
| Enlace que sale del worktree, padre enlace, dentro de una carpeta con `.git` propio | daemon | `path-outside-worktree` | `path-outside-worktree` | ídem |
| Una carpeta, un gitlink, una entrada `skip-worktree`, un enlace llamado `.gitmodules` | daemon | `path-invalid` | `invalid-path` | "Ruta no válida: {path}. Nombra archivos, no carpetas." |
| `:(glob)**` que no existe, `SRC/A.rs` frente a `src/a.rs` | daemon | `path-not-found` | `path-not-found` | "Ruta no encontrada: {path}." |
| `.env` ignorado, sin seguimiento | daemon | `path-ignored` | `path-ignored` | "La ruta {path} está ignorada por Git; no se añade." |
| "Todo lo preparado" con otra sesión viva de agente | daemon | `shared-worktree-needs-paths` | `other-session-present` | "Hay otra sesión de agente en este worktree." · acción "Indica tus rutas: el worktree es compartido." |

Las acciones de los cuatro primeros: "Nombra archivos del worktree, relativos a su raíz, con su nombre exacto."

### Forma del error y del cuerpo de respuesta

Rechazo de dominio de la herramienta, como DS-US-MCP-005 D4:

```json
{"code":"path-outside-worktree","message":"…","action":"…","params":{"path":{"untrusted":"../otro-repo/secreto"}}}
```

Canal:

```json
{"code":-32014,"message":"operation rejected","data":{"reason":"path-outside-worktree","path":"../otro-repo/secreto"}}
```

`data.path` es un campo nuevo y opcional de `RejectedData`, solo con `CAP_OPERATION_COMMIT`. Un rechazo nunca lleva el contenido del archivo, el destino de un enlace ni una ruta de otro repo.

### Forma de la configuración

_No aplica — no se añade configuración._ Las reglas de ignorado y `core.precomposeUnicode` son las del repo y del usuario, leídas por gitoxide.

### Valores numéricos

| Concepto | Valor | Fuente |
|---|---|---|
| Rutas por llamada | ≤ 256 (`MAX_COMMIT_PATHS`); la 257.ª da `too-many-paths` | `crates/api/src/catalog.rs:38`; ADR-MCP-001 § 6 (500, endurecido) |
| Bytes por ruta | ≤ 4.096 (`MAX_PATH_BYTES`) | ADR-MCP-001 § 6 |
| Ruta en una respuesta | ≤ 1.024 bytes, con `truncated` | ADR-MCP-001 § 6 |
| Lecturas del disco por ruta | un `symlink_metadata` por componente y, si es enlace, un `read_link` | T004 |
| Vida de una sesión sin identidad de proceso | 600 s desde que el detector la vio (`PRESENCE_TTL`) | ⚠️ **ASSUMPTION** (D10 a) |

---

## Modelo de datos

_No aplica — sin migraciones ni datos nuevos en el perfil; la presencia lee las sesiones que ya existen._

---

## Estrategia de pruebas y cobertura

### 9.1 Pirámide de pruebas

| Tipo | Cantidad | Tareas dueñas | Herramientas | Cuándo |
|------|---------:|-------------|---------|------|
| Unit | 9 | T002, T003, T005 | `cargo test` | PR gate |
| Integration | 25 | T004, T005, T006 | `cargo test -p gitraptor-core --test us_mcp_010*` | PR gate |
| E2E | 9 | T001 | `cargo test -p gitraptor-cli --test mcp_safe_commit_paths` (macOS) | PR gate en macOS |
| Security | 28 | T007 | `cargo test -p gitraptor-cli --test mcp_security_corpus` | PR gate |

### 9.2 Umbrales de cobertura

| Capa | Línea | Rama | Mutación | Camino crítico 100% |
|-------|-----:|-------:|---------:|:------------------:|
| `commit_paths::check_commit_path` | — | — | — | ✅ cada regla de 2.1 a 2.5 y cada alias de `.git` |
| `ops::commit_paths::select` | — | — | — | ✅ cada paso de 4.1 a 4.6, con y sin `core.precomposeUnicode` |

### 9.3 Datos de prueba

- Builders / fixtures: el rig de `us_mcp_009.rs` y la pareja de `tm_untrusted_git.rs:Pair`, copiados; repos, HOME y perfil temporales. Nunca este repo ni el perfil real (NFR-01).
- Multi-tenant data: dos agentes simulados vivos en el mismo worktree para BR-MCP-EDGE-003; un tercero cuya sesión queda sin fin tras matar su proceso; el desarrollador con la TUI.
- PII / PHI: el `.env` de las pruebas contiene un canario de `gitraptor_testkit::canary`, que no puede aparecer en ninguna salida.
- Time / clock: sin esperas fijas; las sesiones presentes se esperan con `DEADLINE`; `PRESENCE_TTL` se prueba con el instante inyectado en la presencia.

Escenario de la historia → prueba:

| Escenario | Prueba | Comando |
|---|---|---|
| 1. Una ruta fuera del worktree rechaza la llamada entera, ruta como no confiable | e2e `a_path_outside_the_worktree_refuses_the_whole_call` | `cargo test -p gitraptor-cli --test mcp_safe_commit_paths a_path_outside_the_worktree_refuses_the_whole_call -- --exact` |
| 2. `/etc/passwd`, UNC, controles → "ruta no válida" | e2e `invalid_paths_never_reach_git` | ídem con el nombre |
| 2. Enlace que sale → "ruta fuera del worktree" | e2e `a_link_out_of_the_worktree_is_outside` | ídem |
| 2. Más rutas que el tope → "demasiadas rutas" | e2e `too_many_paths_are_refused` | ídem |
| 3. `:(glob)**` → "ruta no encontrada" | e2e `pathspec_magic_is_a_literal_name_not_found` | ídem |
| 4. Un archivo nuevo entra solo si se nombra | e2e `a_new_file_enters_only_when_named` | ídem |
| 5. Un ignorado nunca se commitea | e2e `an_ignored_file_is_never_committed` | ídem |
| 6. En compartido hay que nombrar las rutas; con ellas, solo esas | e2e `a_shared_worktree_needs_named_paths` | ídem |
| 6. El desarrollador no vuelve compartido el worktree | e2e `the_developers_tui_does_not_make_the_worktree_shared` | ídem |

Integración (`us_mcp_010.rs`): las guardas de T004, `a_parent_symlink_is_outside`, `a_link_inside_is_committed_as_a_link`, `a_symlink_named_gitmodules_is_invalid`, `a_path_into_a_nested_repo_is_outside`, `a_folder_is_an_invalid_path`, `a_deleted_tracked_file_is_committed_as_a_deletion`, `a_link_swapped_in_between_prepare_and_run_is_state_changed`, `an_nfd_name_matches_its_nfc_entry_when_git_precomposes`, `dotgit_rules_match_the_tree_path_rules` y `the_ignore_rules_of_info_exclude_and_the_user_apply`. `us_mcp_010_untracked.rs` y `us_mcp_010_shared.rs`: las de T005 y T006.

### 9.4 Comportamientos críticos verificados

- [ ] Ninguna ruta rechazada llega a un argv de Git: el rechazo léxico ocurre antes de conectar y el del daemon antes de la intención.
- [ ] Toda ruta llega a Git como `:(literal)<ruta>` después de `--`.
- [ ] Ningún alias de `.git` pasa, en ninguna plataforma.
- [ ] Un rechazo deja el repo, el worktree, su `.git` y el otro repo de la pareja byte a byte iguales.
- [ ] Un ignorado no entra nunca, ni nombrado; "todo lo preparado" no añade archivos nuevos.
- [ ] Un commit que no entra, también por `time-limit` o cancelación, deja el índice como estaba antes del `add`; si no puede (`index.lock` huérfano), el resultado es `failed-changed`.
- [ ] Una presencia ilegible cuenta como worktree compartido; una sesión muerta y la del desarrollador no.
- [ ] Ningún archivo de otro repo, ni el destino de un enlace, aparece en stdout, stderr o una respuesta.

### 9.5 Plataformas

| Plataforma | Cómo se verifica | Pendiente |
|---|---|---|
| macOS | e2e de T001, integración, unitarias y corpus en local; APFS sin distinguir mayúsculas y con `core.precomposeUnicode` | — |
| Linux | integración, unitarias y corpus `server` en CI `ubuntu-latest`; e2e no (DEP-MCP-9) | Pendiente: etapa de validación multiplataforma (XP-46 a): ext4 distingue mayúsculas, primera ejecución verde del corpus `engine` (XP-42) |
| Windows | `us_mcp_010_paths.rs` con las reglas de `cfg(windows)` y `clippy --target x86_64-pc-windows-msvc`; el commit se rechaza con `git-unavailable` (DS-US-MCP-009 D15) | Pendiente: etapa de validación multiplataforma (XP-46 b): UNC real, unidad, flujos alternativos, nombres reservados, punto o espacio final, nombres 8.3, *junctions* (G1) |

---

## Gate de seguridad

- Validación de entrada en dos sitios con la misma función (`check_commit_path`); el daemon no confía en `raptor-mcp` (S-02: un agente puede hablar con el socket).
- Sin shell, argv fijo, rutas tras `--` con `:(literal)` (NFR-02).
- Ningún enlace se sigue: `symlink_metadata` y `read_link`, nunca `canonicalize` de una ruta del agente.
- El `git_dir` sale de la puerta del registro (#223 I-03); una ruta dentro de un `.git` ajeno o con un alias de `.git` se rechaza.
- Comprobación repetida bajo el cerrojo; la `Selection` en la huella.
- Toca validación de rutas y ejecución de procesos: revisión de `security-expert` obligatoria antes de mezclar.

### Riesgos residuales

- **Bajo**: entre la comprobación bajo el cerrojo y la lectura de Git, un proceso del mismo usuario puede cambiar un componente por un enlace. Git no sigue enlaces en un pathspec ("beyond a symbolic link") ni en `add` (A22); el snapshot previo lo recupera.
- **Bajo**: un `.git` anidado que aparece entre la comprobación y el `add`: Git lo trataría como otro repo y el `add` fallaría o añadiría un gitlink; el snapshot previo lo recupera (D9).
- **Bajo**: en un worktree compartido, un agente puede commitear una ruta con cambios de otro si la nombra; es la regla de BR-MCP-EDGE-003 y la atribución por archivo está fuera del MVP (Q7).

Corre `/security-review --scope devspec docs/requirements/features/mcp/dev-specs/US-MCP-010-dev-spec.md` antes de mezclar.

---

## Modelo de amenazas

Atacante: el de ADR-MCP-001 § 9 (agente comprometido por *prompt injection*, con shell del mismo usuario y acceso directo al socket). Categorías STRIDE: S suplantación, T manipulación, R repudio, I revelación, D denegación de servicio, E elevación.

| # | STRIDE | Amenaza | Mitigación | Prueba guarda |
|---|---|---|---|---|
| A1 | I, T | `..` sale del worktree (`../otro-repo/secreto`) | Forma léxica 2.4 en `raptor-mcp` y daemon | `us_mcp_010_paths::dot_dot_is_outside`; corpus `safe-commit-path-traversal` |
| A2 | I | Ruta absoluta (`/etc/passwd`) | 2.3 | `absolute_paths_are_invalid`; corpus `safe-commit-path-absolute` |
| A3 | I | UNC (`\\servidor\x`), unidad (`C:x`), `\` | 2.3 | `unc_drive_and_backslash_are_invalid`; corpus `safe-commit-path-unc`, `safe-commit-path-drive` |
| A4 | T, E | Caracteres de control, bidi, anchura cero en la ruta (falsear lo que ve el desarrollador) | 2.2 (`is_forbidden_char`) | `control_and_bidi_are_invalid`; corpus `safe-commit-path-bidi`, `safe-commit-path-nul` |
| A5 | E | Magia de pathspec (`:(glob)**`, `:!src`, `:(top)`) | `:(literal)` por ruta y coincidencia exacta 4.3 a 4.6 | ⛔2.1, `pathspec_magic_is_a_literal_name_not_found`; corpus `safe-commit-path-glob`, `safe-commit-path-exclude` |
| A6 | E | Inyección de opción (`--amend`, `-f`, `--pathspec-from-file=x`) | `-` inicial inválido (2.3); `--` antes de las rutas | `leading_dash_is_invalid`; `user_ops::tests::the_commit_message_never_travels_in_argv`; corpus `safe-commit-path-option` |
| A7 | I, T | Escribir o leer dentro de `.git` (`.git/config`, `sub/.GIT/hooks/x`) | 2.4 con `is_dotgit_alias` | ⛔2.2; corpus `safe-commit-path-dot-git` |
| A8 | I | Enlace como último componente que apunta fuera | 4.2 | `a_link_out_of_the_worktree_is_outside`; corpus `safe-commit-path-link-out` |
| A9 | I | Carpeta padre que es un enlace a otro repo | 4.1 | `a_parent_symlink_is_outside`; corpus `safe-commit-path-link-parent` |
| A10 | I, T | Ruta dentro de un repo anidado o de un submódulo | 4.1 (carpeta con `.git`), 4.3 (gitlink) | `a_path_into_a_nested_repo_is_outside`; corpus `safe-commit-path-nested-repo` |
| A11 | T, E | `.git` del worktree reescrito para apuntar a otro repo, antes o entre `prepare` y `run` | `git_dir` de `DaemonBackend::facts` y la puerta del registro; hechos y huella otra vez bajo el cerrojo | ⛔4.2; corpus `safe-commit-path-dot-git-swap` |
| A12 | T | Enlace o carpeta cambiados entre `prepare` y `run` (TOCTOU) | `run` planea otra vez bajo el cerrojo; `Selection` en la huella | `a_link_swapped_in_between_prepare_and_run_is_state_changed` |
| A13 | I | Secretos ignorados (`.env`) nombrados | 4.5 (`is_ignored`) y `add` sin `-f` | `an_ignored_file_is_never_committed`, ⛔5.1; corpus `safe-commit-path-ignored` |
| A14 | I, E | "Todo lo preparado" se lleva archivos nuevos o el trabajo preparado de otro agente | `add_first` solo con rutas nombradas (5); compartido exige rutas (T006) | `a_new_file_enters_only_when_named`, ⛔6.1; corpus `safe-commit-path-shared-all-staged` |
| A15 | T | Mayúsculas que apuntan a otra entrada (`SRC/A.rs`) | Sin plegado de mayúsculas (4.7) | ⛔4.1; corpus `safe-commit-path-case` |
| A16 | D | Miles de rutas o rutas enormes para agotar el daemon | Tope de 256 y de 4.096 bytes antes de recorrer; tope de entrada de 1 MiB de `raptor-mcp` (`apps/mcp/src/input.rs`, #225) | `too_many_paths_are_refused`; corpus `safe-commit-path-too-many`, `jsonrpc-oversized-message` |
| A17 | I | La respuesta filtra rutas o contenido de otro repo | Solo `params.path` de la ruta pedida, untrusted; presupuesto de respuesta de US-MCP-005 | `forbidden` `{other_repo}` en los casos `engine` |
| A18 | R | El commit no se atribuye al agente | Operación protegida con solicitante y canal `mcp` (DS-US-MCP-009) | `the_commit_is_recorded_with_the_agent_and_channel_mcp` |
| A19 | S | Formas de Windows: flujos alternativos, nombres reservados, punto o espacio final en cualquier componente, 8.3, *junctions* | 2.5 con `cfg(windows)` | `us_mcp_010_paths::windows_forms_are_invalid` (`cfg(windows)`); corpus `safe-commit-path-windows-*` con `pending: XP-46` |
| A20 | T, E | Alias de `.git` en cualquier plataforma: ignorables de HFS+ (`.g\u{200C}it`, CVE-2014-9390), `GIT~1`, `.git.`, `.git `, `.git::$INDEX_ALLOCATION`; enlace llamado `.gitmodules` o `.gitattributes` | `is_dotgit_alias` en todas las plataformas (3); `is_git_special_name` para enlaces (4.2) | ⛔2.2, `dotgit_rules_match_the_tree_path_rules`, `a_symlink_named_gitmodules_is_invalid`; corpus `safe-commit-path-dot-git-hfs`, `safe-commit-path-dot-git-short`, `safe-commit-path-dot-git-trailing` |
| A21 | I | Enlace duro o copia de un secreto dentro del worktree, hecho por el agente con su shell | Fuera del modelo del atacante: con su shell el agente ya lee el secreto (I-01 de ADR-MCP-001 § 9); un enlace duro es un archivo normal del worktree | _no comprobable — fuera del modelo del atacante, declarado_ |
| A22 | T | Carrera: una carpeta padre se cambia por un enlace justo antes del `add` | Git rechaza un pathspec "beyond a symbolic link"; el `add` falla y el commit no se lanza (T005 paso 2) | `user_ops::tests::add_beyond_a_symbolic_link_is_refused_by_git` |
| A23 | T | Entrada con `skip-worktree`, `assume-unchanged` o fuera de un *sparse checkout*: la ruta sigue en el índice sin estar en el disco y commitearla registraría su borrado | 4.3 la rechaza con `path-invalid` | ⛔4.3; corpus `safe-commit-path-skip-worktree` |
| A24 | T | Nombre en NFD frente a la entrada en NFC (macOS), o al revés, para colar una segunda entrada | 3: comparación en NFC con `core.precomposeUnicode`, como Git; bytes exactos sin ella | `an_nfd_name_matches_its_nfc_entry_when_git_precomposes`; corpus `safe-commit-path-nfd` |
| A25 | T | `index.lock` huérfano tras un `time-limit` impide deshacer el `add` | 4.2 de T005: `failed-changed`, el lock no se borra, el previo lo recupera | ⛔5.3 |

---

## Fuera de alcance

| Ítem / no-objetivo | Historia que lo cubre | Gate (cómo se verifica) |
|----------------|--------------------|-------------------------|
| `expect_worktree`, operación en curso, HEAD separado | US-MCP-011 | esquema de `safe_commit` sin cambios (DS-US-MCP-009 D13) |
| Carpetas como ruta y commit de submódulos | — | `a_folder_is_an_invalid_path` |
| Archivos cuyo nombre empieza por `-`, contiene `\` o empieza por `X:` | — | `leading_dash_is_invalid`, `unc_drive_and_backslash_are_invalid` |
| Rutas con `skip-worktree`, `assume-unchanged` o fuera del cono de un *sparse checkout* | — | ⛔4.3 |
| Atribución por archivo en un worktree compartido | Q7 (fuera del MVP) | sin cambios en el motor |
| Registro de un segundo agente en el mismo worktree | US-GRP-011, US-MCP-006 | la presencia lee las sesiones que ya existen |
| Rutas prohibidas de Guardrails | US-GRD-008 (ya en `main`, por el plan de `commit`) | `PlanShape::Commit.paths` sin cambios |
| Usar `gix-validate` directamente desde `crates/api` | cuando `crates/api` dependa de `gix` | `Cargo.toml` sin cambios; prueba cruzada de A20 |

---

## Decisiones validadas

Cada decisión es **Decisión del orquestador (2026-10-21)**, validada por quien indica la columna "Validación", con el ajuste aplicado. El pathspec `:(literal)` tras `--` ya lo fija ADR-CKP-002, Enmienda (2026-10-17, TS-CKP-003), y no se redecide.

| # | Decisión | Validación y ajuste aplicado | Alternativas descartadas |
|---|---|---|---|
| D1 | Dos filtros: forma léxica en `crates/api`, compartida por `raptor-mcp` y el daemon; contraste con el worktree solo en el daemon, en `prepare` y otra vez bajo el cerrojo, con la `Selection` en la huella | Validada por Arquitecto y PO, tal cual | Comprobar el disco en `raptor-mcp`: no lee Git (ADR-MCP-001 § 1) y el agente puede saltárselo hablando con el socket |
| D2 | Forma canónica obligatoria y la misma en todas las plataformas: sin `\`, sin `X:` inicial, sin `-` inicial, sin componentes vacíos ni `.`, sin barra final; `..` y todos los alias de `.git` (ignorables de HFS+, `GIT~1`, puntos y espacios finales, sufijo de flujo) cuentan como "fuera del worktree" en todas las plataformas, con las reglas de `tree_path.rs:is_dotgit` copiadas y comprobadas por una prueba cruzada | Validada por Arquitecto y PO, con ajuste: alias de `.git` en todas las plataformas; `gix-validate` está en `Cargo.lock` solo a través de `gix`, que `crates/api` no usa, así que se copian las reglas de `tree_path.rs` y se verifica que cubren la lista completa de ignorables de HFS+ (U+206A a U+206F no están en `is_forbidden_char`) | Normalizar la ruta (oculta lo que pidió el agente); alias de `.git` solo en su plataforma; añadir `gix-validate` a `crates/api` |
| D3 | Motivos: `invalid-path`, `path-outside-worktree`, `too-many-paths`, `path-not-found`, `path-ignored`, `other-session-present`; una ruta mala rechaza toda la llamada y vuelve como untrusted. 257 rutas dan `too-many-paths` ("demasiadas rutas"), nunca `invalid-params` del esquema | Validada por Arquitecto y PO, con ajuste: el esquema de DS-US-MCP-009 no declara `maxItems` y el tope es un rechazo de dominio | Commitear las rutas buenas y avisar de las malas (contradice el escenario 1); el tope en el esquema (`invalid-params` genérico) |
| D4 | Una ruta vale si es la misma entrada del índice o de `HEAD`, o un archivo del listado sin seguimiento de gitoxide: bytes exactos, y en NFC en los dos lados cuando el repo tiene `core.precomposeUnicode`, como hace Git; mayúsculas nunca se pliegan | Validada por Arquitecto y PO, con ajuste: comparar como Git con `core.precomposeUnicode` (listado en NFD frente a índice en NFC, como trata `tree_path.rs`) y su prueba | Plegar mayúsculas (crea entradas dobles en el índice); bytes exactos siempre (rechaza en macOS nombres con tildes que Git acepta) |
| D5 | Enlaces: un padre enlace siempre es "fuera del worktree"; el último componente enlace se acepta si su destino, resuelto léxicamente, queda dentro del worktree y fuera de `.git` (se commitea el enlace); un enlace llamado `.gitmodules`, `.gitattributes` o `.gitignore` (o un alias) es `invalid-path` | Validada por Arquitecto y PO, con ajuste: rechazar los enlaces con nombre especial de Git | Rechazar todo enlace (impide commitear enlaces del propio repo); seguir el enlace |
| D6 | Ignorados con `RepoReader::is_ignored` (gitoxide), solo para rutas sin seguimiento; `git add` sin `-f` como segunda barrera | Validada por Arquitecto y PO, tal cual | Lanzar `git check-ignore`: otro proceso fuera de la lista cerrada de operaciones de usuario |
| D7 | Archivos nuevos nombrados: `git add` en el mismo paso protegido antes del commit, y `git rm --cached` de esos mismos si el commit no entra, también tras `time-limit` o cancelación; un `index.lock` huérfano que impide deshacer da `failed-changed` y no se borra | Validada por Arquitecto y PO, con ajuste: el motivo para descartar `GIT_INDEX_FILE` corregido; el `index.lock` huérfano declarado y probado; la vuelta atrás también en los caminos de `time-limit` y cancelación | Índice propio con `GIT_INDEX_FILE`: `commit --only` ya da a los hooks un índice temporal, pero un índice propio dejaría el índice del usuario sin la entrada nueva tras un commit que sí entró, y la variable llegaría a los hooks del usuario |
| D8 | No se aceptan carpetas, gitlinks ni entradas con `skip-worktree`/`assume-unchanged`: hay que nombrar archivos que están en el disco | Validada por Arquitecto y PO; el PO enmienda BR-MCP-VAL-001 ("archivos, no carpetas ni submódulos") | Aceptar carpetas: con `add` meterían archivos nuevos sin nombrarlos (BR-MCP-EDGE-010) |
| D9 | Puerta del `.git`: ninguna lectura del `.git` del worktree; una ruta bajo una carpeta con su propio `.git` es "fuera del worktree". Riesgo residual declarado: un `.git` anidado que aparece entre la comprobación y el `add`, cubierto por el previo | Validada por Arquitecto y PO, con ajuste: el riesgo residual declarado | — |
| D10 | Worktree compartido: (a) presencia = sesiones de otros agentes, sin fin y vivas (proceso `(pid, inicio)` existente o, sin identidad de proceso, vistas en los últimos 600 s), en el perfil del worktree, por `ProtectedBackend::live_sessions`; ilegible cuenta como compartido. (b) Enmienda de ADR-MCP-001 § 4.3 para admitir las rutas nombradas. (c) La sesión del desarrollador o de la TUI no cuenta. `commit` nunca lleva el aviso `active-session` | Validada por Arquitecto y PO, con ajustes (a) vida de la sesión, (b) Enmienda del § 4.3 y (c) solo otros agentes; el PO enmienda BR-MCP-EDGE-003 ("sesión = de otro agente; el desarrollador no cuenta; ilegible = compartido") | Aviso reconocible con `acknowledge` (ADR-MCP-001 § 4.3: `commit` sin avisos); contar toda sesión sin fin (un agente caído bloquearía para siempre) |
| D11 | Formas de Windows (`:`, nombres reservados, punto o espacio final en cualquier componente) con `cfg(windows)`, verificadas en la etapa multiplataforma (XP-46, G1) | Validada por Arquitecto, tal cual | Esperar a Windows para escribirlas |
| D12 | Corpus: casos nuevos `safe-commit-path-*.json` en `apps/cli/tests/mcp_corpus/cases/`; el esquema del caso gana `setup.files` (con `skip-worktree`) y `setup.dot_git_to` | Validada por Arquitecto, tal cual | Un corpus aparte para rutas: dos KPI en vez de uno (DS-INF-MCP-001 D2) |
| D13 | Rutas relativas a la raíz del worktree, nunca al cwd del agente | Validada por Arquitecto y PO; el PO lo escribe en BR-MCP-VAL-001 | Relativas al cwd: el cwd de `raptor-mcp` puede ser una subcarpeta y la misma ruta cambiaría de sentido |
| D14 | Orden de construcción: esta spec va después de DS-US-MCP-009 y no depende de US-MCP-011 (D13 de DS-US-MCP-009); las tres se liberan juntas | Validada por Arquitecto y PO, tal cual | Esperar a la Dev Spec de US-MCP-011 (D-11 original) |

---

## Notas del autor

| ID | Nota | Acción | Owner |
|----|------|--------|-------|
| N1 | `check_literal_relative` (`crates/api/src/catalog.rs:557`) ya rechaza `/`, `\` y `-` iniciales, controles, `..` y `.git` en ASCII; no rechaza `\` en medio, `X:`, componentes vacíos ni `.`, ni los alias de `.git`. T002 la sustituye sin aflojar ningún caso | T002 | — |
| N2 | US-GRP-011 está en `draft`; las sesiones de dos agentes detectados en el mismo worktree ya se guardan en el perfil (`profile/store.rs:sessions_for_worktree`) | Ninguna | — |
| N3 | El brazo de prueba de `commit` leía el `.git` del worktree; DS-US-MCP-009 D16 lo retira | Ninguna | — |
| N4 | El pathspec literal va como `:(literal)` por ruta y no con `GIT_LITERAL_PATHSPECS`, porque esa variable llegaría a los hooks del usuario (ADR-CKP-002, Enmienda (2026-10-17)) | Ninguna | — |
| N5 | `tree_path.rs:HFS_IGNORABLE` incluye U+206A a U+206F, que `catalog.rs:is_forbidden_char` no rechaza (cubre U+2060 a U+2069) | T002 copia la lista | — |

### Convenciones observadas

- Forma léxica de rutas en el contrato: `crates/api/src/catalog.rs:check_literal_relative`.
- Alias de `.git` y colisiones de normalización: `crates/git/src/tm_write/tree_path.rs:is_dotgit`, `fold`.
- Ningún componente seguido como enlace: `crates/core/src/executor/new_path.rs:check_new_worktree_path`.
- Puerta del `.git`: `crates/core/src/observe.rs:open_registered_worktree`.
- Ignorados con gitoxide: `crates/git/src/reader.rs:RepoReader::is_ignored`.
- Pathspec literal por ruta: `crates/git/src/user_ops.rs:UserOp::words` (`CommitPaths`).
- Casos de corpus en JSON por archivo: `apps/cli/tests/mcp_corpus/cases/status-param-path-traversal.json`.

### Inventario de reutilización

- Reutiliza: `gitraptor_api::catalog::{is_forbidden_char, MAX_COMMIT_PATHS, MAX_PATH_BYTES}`; las reglas de `gitraptor_git::tm_write::tree_path::{is_dotgit, HFS_IGNORABLE, fold}`; `unicode-normalization` (ya en `crates/git/Cargo.toml`); `gitraptor_git::RepoReader::{status, is_ignored}`; `gitraptor_git::capture::CaptureReader::untracked` (si hace falta, nota de T004); `crate::observe::open_registered_worktree` vía `DaemonBackend::facts`; `crate::profile::Store::sessions_for_worktree`; `crate::channel::peer::ProcSource` para la vida de una sesión; el rig de `crates/core/tests/tm_untrusted_git.rs:Pair`; `gitraptor_testkit::{fingerprint, canary, mcp_corpus}`.
- Genera: ninguno.
- Crea nuevo: `crates/api/src/commit_paths.rs`, `apps/mcp/src/safe_commit_paths.rs`, `UserOp::AddPaths` y `UserOp::UnstagePaths`, `gitraptor_git::paths::precomposed`. Búsquedas sin resultado: `grep -n "AddPaths\|\"add\"" crates/git/src/user_ops.rs` (la lista cerrada no tiene `add`); `grep -rn "gix_validate\|gix::validate::path" crates` (solo `refname.rs` usa `gix::validate::reference`).
