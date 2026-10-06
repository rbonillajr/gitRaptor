---
id: DS-INF-GRP-001
title: "Dev Spec — Arnés de verificación \"repo intacto\" (núcleo)"
type: dev-spec
status: approved
feature: motor-local
domain: GRP
created: 2026-10-04
updated: 2026-10-05
related:
  stories: [INF-GRP-001, TS-GRP-002, TS-GRP-003, TS-GRP-004, US-GRP-002, US-GRP-004, US-GRP-007, INF-GRD-001, INF-TMC-001]
  adrs: [ADR-GRP-009, ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, ADR-GRD-001]
  nfrs: [NFR-01, NFR-02, NFR-12, SEC-09, SEC-11, BR-CONS-001]
tags: [motor-local, arnes, repo-intacto, testkit, repo-canario, auditoria-exec, ci, seguridad]
---

# Dev Spec — INF-GRP-001: arnés "repo intacto" (núcleo)

Plano de ejecución compacto (AADD ligero) del **núcleo** de [INF-GRP-001](../technical-stories/INF-GRP-001-arnes-repo-intacto.md). Sigue punto por punto el apartado Validación de [ADR-GRP-009](../../../../architecture/decisions/ADR-GRP-009-frontera-solo-lectura-git.md). Las suites incrementales de TS-GRP-003, TS-GRP-004, US-GRP-002, US-GRP-004 y US-GRP-007 se describen aquí solo como ganchos (§ 8).

## 1. Decisiones

Todas son **decisiones del orquestador (2026-10-04)**, validadas por el Arquitecto (`nassa-architect:architect`) y por el PO (`nassa-aadd:product-owner`). Se incluyen los ajustes que pidieron.

| # | Decisión | Validada por |
|---|---|---|
| D1 | **Crate de soporte de pruebas `crates/testkit`** (`gitraptor-testkit`, `publish = false`), consumido **solo como `[dev-dependencies]`**. Se descartan dos alternativas: un módulo `testkit` en `crates/git/src` (obligaría a autorizar `Command::new` fuera de `invoke.rs` y debilitaría la Validación 5) y `#[path]` hacia `crates/git/tests/common` (frágil y opaco para Nx). Se enmiendan ADR-GRP-002 (estructura y lista del MVP) y el alcance de esta INF | Arquitecto |
| D2 | **El testkit no depende de ningún crate de GitRaptor.** El código bajo prueba se conecta como closures y probes. Así los tests de `crates/git` no enlazan dos copias de `gitraptor-git`. Los adaptadores (`invoker_for`, `read_everything_with`) viven en `crates/git/tests/common` | Arquitecto |
| D3 | La regla "solo dev-dependency" se comprueba con `cargo metadata` y no leyendo los `Cargo.toml` (`crates/testkit/tests/dev_only.rs`) | Arquitecto |
| D4 | **La huella incluye** ruta, tipo, tamaño, hash de contenido, mtime, **bits de permiso** y, en unix, **inodo y ctime**; nunca `atime`. Un `chmod` y una reescritura con la misma marca de tiempo son escrituras | Arquitecto |
| D5 | **La config de sistema se pide al Git resuelto** (`git config --system --show-origin --list`), porque no siempre es `/etc/gitconfig` (Homebrew, Command Line Tools, Git for Windows) | Arquitecto |
| D6 | **Ejecución de control con resta solo si el control tiene cambios.** Sin actividad del usuario o del agente, el control debe salir vacío y la ejecución con motor también (modo `Strict`). Con actividad, se resta por (ámbito, ruta, tipo de cambio) (modo `Subtracted`). En escenarios concurrentes, la resta puede tapar un efecto del motor sobre una ruta que también toca el usuario; ahí la garantía la dan la auditoría de `exec` y el registro de argv | Arquitecto |
| D7 | **Guarda positiva**: el arnés solo actúa sobre raíces temporales que el propio testkit crea y marca (`.gitraptor-testkit-root`). Además se niega siempre a operar sobre el repo de GitRaptor (este checkout y su worktree principal), sobre `/` y sobre el home real o cualquiera de sus ancestros | Arquitecto |
| D8 | **Auditoría de `exec` en dos capas.** La puerta portable y sin privilegios es la de **trampas**: el shim de `git` y las trampas son **copias del propio binario de test**, no scripts, para que la misma puerta funcione en Windows. Los **trazadores de kernel** (strace, eslogger) forman la auditoría profunda; ETW queda pendiente | Arquitecto |
| D9 | **CI con un único check obligatorio por SO**, `repo-intact`. Ejecuta `cargo test --workspace -- repo_intact` siempre, nunca con `nx affected`. Las suites se registran solo por el prefijo de nombre `repo_intact::`, sin features que puedan quedar apagadas sin que nadie lo vea | Arquitecto |
| D10 | Los escenarios de "repo de otro uid" y de `settings.json` como symlink quedan con **cobertura parcial documentada** (§ 7) | PO |
| D11 | **La INF no se cierra con este PR.** Queda en curso hasta la primera ejecución real del gate en los tres SO. La auditoría de `exec` es **parcial** (solo verificada la puerta de trampas en macOS), así que ADR-GRP-005 y ADR-GRP-009 no ganan aún la condición que dependía de ella | PO |

## 2. Dependencias

| Crate | Dónde | Por qué |
|---|---|---|
| `tempfile` (workspace) | `gitraptor-testkit` | Raíces temporales de cada fixture (NFR-01) |
| `serde_json` 1 (workspace, nueva) | `gitraptor-testkit` | Leer `cargo metadata` (D3) y los eventos JSON de eslogger |
| `gitraptor-testkit` | `[dev-dependencies]` de `gitraptor-git` | Primer consumidor |

## 3. Estructura

| Ruta | Responsabilidad |
|---|---|
| `crates/testkit/src/fingerprint.rs` | `Scope`, `Snapshot::take`, `diff` → `Change { scope, path, kind }` (Validación 1 y 2) |
| `crates/testkit/src/exceptions.rs` | Excepciones por escenario: `engine_profile`, `autostart` (PQ-1), `guardrails_install` / `guardrails_uninstalled` (ADR-GRD-001 § 7), comparación semántica de claves de config |
| `crates/testkit/src/control.rs` | `Scenario` + `Step::{User, Engine, Concurrently}` → `Report` (Validación 3), y `check` para escenarios sin control |
| `crates/testkit/src/fixture.rs` | Máquina temporal: `home/`, `repo/`, `other-repo/`, `profile/{data,config,state}`, worktrees `wt-*`, config de sistema |
| `crates/testkit/src/guard.rs` | Guarda (D7) |
| `crates/testkit/src/canary.rs` | Repo canario de SEC-09 (unix), ampliable por INF-TMC-001 (`arm_program`, `config`) |
| `crates/testkit/src/exec_audit.rs` | `TrapAudit`, `dispatch`, `ProbeSpec`, `Tracer::{Strace, Eslogger}` y sus parsers |
| `crates/git/tests/repo_intact.rs` | Escenarios del núcleo sobre la capa de lectura real |
| `crates/git/tests/repo_intact_exec.rs` | Target `harness = false`: runner, probe, shim y trampas de la auditoría de `exec` |
| `crates/git/tests/canary.rs` | Tests del canario de TS-GRP-002, ahora sobre el canario del testkit |
| `.github/workflows/repo-intact.yml` | Propuesta de gate de CI (§ 9) |

**Determinismo de los fixtures.** `Fixture::write` fija un mtime en el pasado y lo incrementa un segundo en cada escritura. Con eso, dos fixtures construidos por el mismo código son idénticos, Git nunca ve una entrada "racy" del índice y una reescritura del mismo tamaño sigue cambiando el stat. Las dos alternativas fallaban:
- Con el mtime "ahora", Git rehashea los archivos racy y refresca al azar el mtime de objetos sueltos ya existentes. El control de `gc --auto` fallaba de forma intermitente.
- Con un mtime fijo y único, `git add` no veía una reescritura del mismo tamaño. El canario a veces no comiteaba `a.txt` filtrado.

Ambos casos se reprodujeron y se corrigieron al implementar.

## 4. Huella y excepciones (Validación 1 y 2)

- **Ámbitos**:
  - Cada carpeta de primer nivel de la raíz del fixture es un ámbito con su propio nombre: `repo` (con `.git`), `wt-*`, `home` (`.gitconfig`, `.config/git`, `.gnupg`, `.claude`), `other-repo`, `profile` y, si el escenario los crea, `markers`, `bin`, `trace` y `audit`.
  - La config de sistema es un ámbito `system` de solo lectura.
  - Los nombres estables permiten comparar dos fixtures en la ejecución de control.
- **Criterio**: cero diferencias, salvo las excepciones que el escenario declara. `Exceptions::engine_profile("profile")` admite solo `data/` y `state/` del perfil; `config/` es de solo lectura para el motor (ADR-GRP-006 § 1).
- **Excepciones de otras historias**:
  - `autostart(...)`: solo las rutas exactas y los tiempos de su carpeta. La usa la suite de US-GRP-004.
  - `guardrails_install(...)`: solo `core.hooksPath` del `config` del directorio común (comparado clave a clave), `<common>/gitraptor/**` y los tiempos del directorio común.
  - `guardrails_uninstalled(...)`: el `config` puede estar reescrito, pero debe decir exactamente lo mismo.

  Las dos de Guardrails las usa INF-GRD-001. Fuera de esos escenarios no aplica ninguna excepción.
- **Informe**: `Report` nombra el escenario, el modo, cada ruta con su ámbito y el tipo de cambio (`created`, `removed`, `modified ([Mtime, Ctime…])`). Esto cubre la verificación manual de la INF.

## 5. Ejecución de control (Validación 3)

`Scenario::run` construye dos fixtures con el mismo builder y ejecuta los pasos dos veces:
- Con motor: se ejecutan `User`, `Engine` y `Concurrently` (en hilos).
- Control: solo se ejecuta la parte del usuario.

Si el control queda vacío, todo lo que haya en la ejecución con motor se imputa al motor (`Strict`). Si no, se aplica la resta (`Subtracted`, D6).

## 6. Auditoría de `exec` (Validación 5 y 7; SEC-09; M1)

**Puerta de trampas** (portable y sin privilegios):
1. `TrapAudit::install` enlaza o copia el binario de test en `audit/shim/git` y en `audit/trap/<n>`. Los nombres atrapados son `git`, `sh`, `bash`, `zsh`, `dash`, `gpg*`, `ssh`, `less`, `more`, `git-lfs`, `git-upload-pack`, `git-receive-pack`, `git-credential-osxkeychain`, `perl`, `python3`, `cmd` y `powershell`.
2. El probe es el mismo binario, relanzado con un entorno vacío salvo `HOME`, `PATH=audit/trap` y sus variables de control. Resuelve Git con `engine.gitPath = shim`, es decir, con la resolución real de la capa, y ejecuta todas las lecturas sobre el **repo canario**.
3. `exec_audit::dispatch()` es lo primero que ejecuta `main`:
   - Como shim, registra el argv **desde el lado del hijo**, de forma independiente al `ArgvSink` de la capa, y ejecuta el Git real.
   - Como trampa, deja un marcador y sale con 127.
4. Se exige lo siguiente:
   - Probe correcto.
   - Ninguna trampa disparada: `gix` no lanza `git` y Git no busca por nombre un pager ni `gpg`.
   - Ningún marcador del canario.
   - Huella intacta.
   - Cada argv del shim pertenece a la allowlist: opciones fijas, subcomando permitido, sin `status`, `diff` ni listados de config, y sin `%G`.
5. **Controles negativos**: un probe que lanza `git` por nombre dispara la trampa, y un probe que llama al shim con `status` no pasa la allowlist.

**Trazadores de kernel** (`GITRAPTOR_EXEC_AUDIT` = `auto` | `off` | `strace` | `eslogger`). Una elección explícita que no está disponible falla; `auto` la salta. Cada `exec` del árbol del probe debe ser el shim, el Git real, su `exec-path` o, en macOS, el toolchain al que despacha `/usr/bin/git`, y nunca una trampa. Los lanzamientos por ruta absoluta que escapan a la puerta de trampas los cubren el canario y estos trazadores.

| SO | Puerta de trampas | Trazador profundo | Estado en este PR |
|---|---|---|---|
| macOS | Sí, bloqueante | eslogger vía `sudo -n` (necesita root y un permiso de TCC) | Trampas **verificadas** en el Mac de desarrollo. eslogger **no verificado**: no hay root aquí. En CI, job no bloqueante hasta su primera ejecución en verde |
| Linux | Sí, bloqueante | strace `-f` sobre el árbol del probe, sin root, bloqueante | **No verificado** (sin Linux aquí); solo el parser, con una muestra capturada. **Pendiente: etapa de validación multiplataforma** |
| Windows | Sí: las copias son binarios `.exe` | ETW **pendiente**. Alternativa sin admin: avisos de "proceso nuevo" de un Job Object; choca con `unsafe_code = forbid` | **No verificado**. Además, TS-GRP-002 rechaza todo Git en Windows (*fail-closed*), así que los tests de `crates/git` no pueden pasar ahí todavía. **Pendiente: etapa de validación multiplataforma** |

La **comprobación estática** (Validación 5, `static_check.rs`) sigue cubriendo solo `crates/git/src`. El testkit queda fuera porque solo puede ser dev-dependency (D3) y lanza `git` para montar fixtures, no como motor.

## 7. Plan de pruebas (criterio de la INF → test)

Todos los nombres empiezan por `repo_intact::`.

| Criterio | Test | Estado |
|---|---|---|
| Sensibilidad: lock creado y borrado | `testkit/harness::sensitivity_lock_created_and_deleted_is_detected`, `git/repo_intact::injected_write_during_reads_is_caught` | Verde |
| Sensibilidad: escritura fuera del perfil | `testkit/harness::sensitivity_write_outside_profile_is_detected`, `git/repo_intact::injected_write_during_reads_is_caught` | Verde |
| Sensibilidad: `touch` y `chmod`; config de sistema en la huella | `sensitivity_touch_and_chmod_are_detected`, `sensitivity_system_config_is_fingerprinted_when_present` | Verde |
| Pasa con la capa de lectura real | `git/repo_intact::read_layer_leaves_everything_intact` | Verde |
| Control: `gc --auto` por commit de un agente | `testkit/harness::control_gc_auto_by_agent_commit_is_not_imputed` (comprueba que el pack se crea) | Verde |
| Control: daemon fsmonitor del usuario arrancado | `git/repo_intact::fsmonitor_daemon_running_is_not_imputed` (macOS y Windows; Git no tiene daemon en Linux) | Verde en macOS; Windows: **Pendiente: etapa de validación multiplataforma** |
| Control: un efecto del motor junto a actividad del usuario se sigue imputando | `control_still_imputes_an_engine_write`, `control_is_strict_without_user_activity` | Verde |
| Escenarios: worktrees, merge y rebase en curso, HEAD separado, fsmonitor, untracked cache y split index, hooks, LFS, firmas y trace2, `gc` concurrente, `safe.directory` | `git/repo_intact::{linked_worktrees, merge_in_progress, rebase_in_progress_with_detached_head, detached_head, fsmonitor_untracked_cache_and_split_index, hooks_present, lfs_filters, signatures_and_trace2_target, concurrent_user_gc, rejected_by_safe_directory}` | Verde en macOS; Linux y Windows: **Pendiente: etapa de validación multiplataforma** |
| Repo canario (SEC-09) con stat sucio | `git/canary::repo_intact::*` | Verde (unix) |
| Allowlist de argv y comprobación estática | `git/repo_intact_exec::trap_gate_on_reads`, `git/static_check::repo_intact::process_spawn_only_in_invoke_module` (más `cli_invoke::argv_log_only_contains_allowlisted_subcommands` de TS-GRP-002) | Verde |
| Auditoría de `exec`: solo allowlist, `gix` no lanza `git` | `git/repo_intact_exec::{trap_gate_on_reads, detects_launch_by_name, detects_argv_outside_allowlist, kernel_tracer}` | Trampas en verde en macOS; trazadores sin verificar (§ 6) |
| SEC-11: `gitdir` hacia `$HOME` | `git/repo_intact::manipulated_gitdir_towards_home`: huella intacta. Que no se vigile es del observador (suite de US-GRP-002) | Parcial |
| SEC-11: repo de otro uid | `rejected_by_safe_directory` con `Trust::Reduced`. Un uid real exige un segundo usuario en CI | Parcial (D10) |
| SEC-11: `settings.json` como symlink a un secreto | `settings_symlink_to_a_secret`: la capa de lectura no lo sigue. El diagnóstico sin contenido lo añade el lector de settings (ADR-GRP-007/008) cuando exista | Parcial (D10) |
| Excepciones de autoarranque y de Guardrails | `autostart_exception_allows_exactly_its_artifacts`, `guardrails_install_allows_only_hookspath_and_its_folder` | Verde (mecanismo); los escenarios reales son de US-GRP-004 e INF-GRD-001 |
| Guarda | `guard_refuses_the_gitraptor_repo`, `guard_refuses_home_and_root_and_unmarked_dirs`, `snapshot_of_the_gitraptor_repo_panics`, `fixture_roots_pass_the_guard` | Verde |
| Solo dev-dependency | `testkit/dev_only::testkit_is_only_a_dev_dependency` | Verde |
| Gate en CI en los tres SO | `.github/workflows/repo-intact.yml` | **No ejecutado** (D11). **Pendiente: etapa de validación multiplataforma** |

## 8. Ganchos para las suites incrementales

Una suite entra con su historia dueña y cumple estas reglas:
1. Añade `gitraptor-testkit` a sus `[dev-dependencies]`.
2. Nombra sus tests `repo_intact::…`, con un `mod repo_intact` en el archivo de test. El gate de CI ya existente la recoge sin cambios en la protección de rama. Una historia anterior nunca queda bloqueada por una suite que todavía no existe: la suite no está en el árbol hasta que se mergea con su dueña.
3. Construye sus escenarios con `Fixture`, `Scenario`/`Step` o `check`, con la excepción que le corresponda y nunca otra.
4. Si lanza procesos del motor, reutiliza `TrapAudit` y `Tracer` con un target `harness = false` que llame a `exec_audit::dispatch()` al principio de `main`.

| Suite (dueña) | Qué usa del núcleo | Qué añade |
|---|---|---|
| Proceso (TS-GRP-003) | `Fixture` con `profile/`, `engine_profile`, `TrapAudit` sobre el binario del daemon | Bloqueo, logs y estado solo en `data/` y `state/`; entorno del daemon (SEC-10); escáner de secretos (SEC-05) |
| Canal (TS-GRP-004) | Lo mismo | Socket o pipe dentro del perfil; UNC sin SMB (SEC-11); stream IPC sin secretos |
| Observación (US-GRP-002) | `Scenario` con `Step::concurrently`, `Canary` y trazadores | Escenarios del § 7 con el observador en marcha; `gitdir` hacia `$HOME` no se vigila |
| Continuidad y autoarranque (US-GRP-004) | `Exceptions::autostart` | `raptor daemon enable` y `disable` |
| `~/.claude` (US-GRP-007) | Ámbito `home/.claude` | Archivos hostiles y canario de prompt (SEC-04) |
| Guardrails (INF-GRD-001) | `guardrails_install` y `guardrails_uninstalled` | Instalación y desinstalación explícitas |
| Time Machine (INF-TMC-001) | `Canary::arm_program`, `Canary::config` | Casos de SEC-TMC-02 |

## 9. CI (propuesta)

En `.github/workflows/repo-intact.yml` hay tres jobs:
- **`lint-and-test`**: `fmt`, `clippy --all-targets -D warnings` y `cargo test --workspace`, con matriz de 3 SO.
- **`repo-intact`**: el **check obligatorio**, `cargo test --workspace -- repo_intact`. En Linux corre con `GITRAPTOR_EXEC_AUDIT=strace` y es bloqueante.
- **`deep-exec-audit-macos`**: eslogger, con `continue-on-error`.

En los dos primeros, Windows va con `continue-on-error` hasta que TS-GRP-002 implemente la comprobación de ACE. Las actions se fijan por SHA y no se usa `nx affected` (D9). No se pudo ejecutar desde aquí.

### Enmienda 2026-10-05: un solo build por SO y filtro por ruta

> Decisión del orquestador (2026-10-04), validada por Arquitecto. Rama `ci/speedup-cache-and-paths`.

`main` exige 9 checks con `strict`, entre ellos `lint and test (<so>)` y `repo-intact (<so>)` en los tres SO. Los nombres no cambian. Lo que cambia es dónde se hace el trabajo:
- **`lint and test (<so>)` compila el workspace una sola vez.** Justo después de `clippy` corre `cargo test --workspace -- repo_intact` con el trazador del SO (`GITRAPTOR_EXEC_AUDIT`: `strace` en Linux, `auto` en el resto), y luego `cargo test --workspace -- --skip repo_intact`. Las suites van primero para que corran en un runner que ningún otro test ha cargado todavía, como en el antiguo job separado: con el orden inverso, la auto-maintenance de git de un fixture (`maintenance.lock`) dio falsos rojos en Linux. El paso del gate lleva `if: !cancelled()`, así que reporta su propia suite aunque fallen `fmt` o `clippy`. El resultado (`steps.suites.outcome`) se guarda en el artefacto `repo-intact-<so>`.
- **`repo-intact (<so>)` es un gate ligero en `ubuntu-latest`.** Descarga ese artefacto y falla si el resultado no es `success`. En Windows emite un aviso, igual que antes. Así desaparecen tres compilaciones completas por PR y un job de macOS y otro de Windows.
- **Fail-closed:** el gate falla si falta el artefacto (job cancelado o que no corrió) o si el job `changes` no terminó en `success`. Un re-run reemplaza el artefacto (`overwrite: true`).
- **Filtro por ruta (solo en `pull_request`).** El job `changes` usa `dorny/paths-filter`. Solo se salta el trabajo con un `'false'` explícito de un filtro que terminó en `success`, y nunca en PRs de 3000 archivos o más (es el límite de la API). Cuando se salta, los dos checks reportan `success` desde un runner Linux, sin compilar. En `push` a `main` y en ejecuciones manuales siempre corre todo.
- **Invariante que justifica el filtro frente a D9:** el resultado de los tests solo depende de las rutas del filtro (`crates/**`, `apps/**`, `Cargo.toml`, `Cargo.lock`, toolchain, configuración de rustfmt y clippy, `.cargo/**`, `.gitattributes` y el propio workflow). Un `include_str!` o una lectura nueva fuera de `crates/` o `apps/` **obliga a ampliar el filtro**.
- **El filtro depende de `strict`.** Un PR apilado que se redirige a `main` solo se reevalúa porque `strict` obliga a actualizar la rama.
- **Caché:** `Swatinem/rust-cache` usa una clave por job, SO, arquitectura, versión de rustc y `Cargo.lock`, y solo guarda en `main`. Los cachés de PR llevaban el repo por encima del límite de 10 GB y desalojaban los de `main`.
- **Pendiente (otra rama):** los fixtures del testkit no desactivan la auto-maintenance de git (`maintenance.auto`, `gc.auto`). Es una inestabilidad que ya existía: falló en `main` en macOS (`c5644f6`).
- **Pendiente (otra rama):** `cargo test -- repo_intact` pasa aunque no encuentre ningún test, por ejemplo tras renombrar un módulo. Un mínimo de tests ejecutados (hoy 42) cerraría ese hueco, que ya existía.

## 10. Verificación realizada

En macOS (Darwin 25.6, Apple Git 2.50.1, Rust 1.99), sin root:
- `cargo clippy --workspace --all-targets -- -D warnings` en verde.
- `cargo test --workspace` en verde.
- `cargo test --workspace -- repo_intact` en verde (42 tests).
- `GITRAPTOR_EXEC_AUDIT=eslogger` falla como se espera ("needs passwordless sudo").

No verificado: Linux, Windows, strace y el workflow de GitHub Actions (**Pendiente: etapa de validación multiplataforma**). Tampoco eslogger, que necesita root en macOS.

## 11. Fuera de alcance y pendientes

- **Canario en Windows**: los programas marcador son scripts `/bin/sh`. Hace falta una variante `.exe`, que podría reutilizar la técnica de copias del binario. **Pendiente: etapa de validación multiplataforma**.
- **ETW en Windows**, o Job Object sin admin, que requiere una excepción a `unsafe_code`. **Pendiente: etapa de validación multiplataforma**.
- **Repo de otro uid real** en CI, con un segundo usuario en el runner Linux. **Pendiente: etapa de validación multiplataforma**.
- **Medir los falsos positivos de LFS**: archivos con filtro y stat sucio (consecuencia de ADR-GRP-009). El escenario `lfs_filters` ya existe; falta la métrica.
- **Migrar la huella de `crates/git/tests/common`** (la de TS-GRP-002) al testkit. Se dejó intacta para no reescribir los tests de TS-GRP-002 en este PR.
- **`AGENTS.md`** lista los crates del MVP sin `testkit`. Se anota en el PR; no se toca aquí.
