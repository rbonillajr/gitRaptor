---
id: DS-TS-GRP-002
title: "Dev Spec — Capa de lectura de Git sin escrituras"
type: dev-spec
status: ready
feature: motor-local
domain: GRP
created: 2026-10-04
updated: 2026-10-04
related:
  stories: [TS-GRP-002]
  adrs: [ADR-GRP-009, ADR-GRP-007, ADR-GRP-001, ADR-GRP-002]
  nfrs: [NFR-01, NFR-02, NFR-07, SEC-02, SEC-05, SEC-09, SEC-10, SEC-11]
tags: [motor-local, git, gitoxide, solo-lectura, allowlist, resolucion-git, seguridad]
---

# Dev Spec — TS-GRP-002: Capa de lectura de Git sin escrituras

Plano de ejecución compacto (AADD ligero) de [TS-GRP-002](../technical-stories/TS-GRP-002-lectura-git.md). La frontera la fija [ADR-GRP-009](../../../../architecture/decisions/ADR-GRP-009-frontera-solo-lectura-git.md) § 1 a § 4; esta spec solo dice **cómo** se construye en `crates/git`.

## 1. Decisiones previas a la spec (coordinador, 2026-10-04)

- **Gate de seguridad** (`non-functional.md`): se rompe el ciclo con INF-GRP-001. Esta tarea comprueba M1 en la versión fijada de `gix` e implementa el repo canario como tests de `crates/git`. La auditoría dinámica de `exec` (eslogger, ETW, strace) y lo que falte para cerrar las condiciones de ADR-GRP-009 quedan para INF-GRP-001.
- **SEC-10 en Windows**: *fail-closed*. Se aplica el criterio de propietario de `gix-sec` y, como la comprobación de ACE de escritura de otros usuarios no está implementada, **todo candidato se rechaza en Windows** con un diagnóstico explícito. La comprobación de ACE es requisito antes de soportar Windows.

## 2. Dependencias

| Crate | Versión | Features | Por qué |
|---|---|---|---|
| `gix` | `0.88` (fijada en `Cargo.lock`) | `sha1`, `status`, `revision`, `excludes`, `dirwalk` | Camino caliente de lectura (ADR-GRP-001). Sin `default-features`: sin red, credenciales, `blame`, `mailmap` ni `worktree-mutation` |
| `tempfile` | workspace, solo `dev-dependencies` | — | Repos temporales en los tests (NFR-01); ya está en el árbol por `gix` |

Nada más: el lanzamiento con tiempo máximo, el entorno y la validación se escriben con `std`.

## 3. Estructura de `crates/git`

| Módulo | Responsabilidad |
|---|---|
| `lib.rs` | Superficie pública y errores (`ReadError`) |
| `paths.rs` | Validación de rutas antes de tocar el FS (SEC-02): absolutas, sin UNC, `\\?\`, `\\.\`, nombres de dispositivo ni ADS |
| `refname.rs` | `RefName`: reglas de `check-ref-format` (vía `gix::validate`) más rechazo de `-` inicial |
| `redact.rs` | URLs de remotos sin userinfo (SEC-05) |
| `reader.rs` | `RepoReader`: lecturas con `gix` en solo lectura |
| `invoke.rs` | **Único** módulo con `std::process::Command` (Validación 5): argv fijo, entorno por allowlist, tiempo máximo, registro de argv |
| `cli.rs` | Funciones tipadas de la allowlist del Git CLI (ADR-GRP-009 § 3) |
| `resolve.rs` | Resolución de Git por candidatos, validación del ejecutable y versión mínima 2.38 (§ 4) |

## 4. Lectura con `gix` (§ 1, M1, SEC-09, SEC-11)

**Apertura** (`RepoReader::open`): la ruta pasa por `paths::validate`; se abre con `gix::open_opts` (sin descubrimiento hacia arriba) y estas opciones:

- `permissions.config.git_binary = false` y `permissions.attributes.git_binary = false`: `gix` no lanza `git` para buscar la config de instalación (**M1**). Comprobado en `gix 0.88`/`gix-path 0.13`: los únicos lanzamientos de `git` de `gix-path` son `installation_config()` (solo con `git_binary`), `core_dir()`/`system_prefix()` (en Windows para la config de sistema y en `editor()`, que la capa no usa) y `system_config_path()` en Windows.
- Config de sistema solo fuera de Windows (`/etc/gitconfig`, ruta fija); en Windows se desactiva para que `gix-path` no lance `git` al buscarla. Global y XDG sí se cargan: la config del usuario rige lo que no ejecuta programas (NFR-07).
- Entorno de `gix` aislado (`Environment::isolated()` salvo `HOME`): las variables `GIT_*` (`GIT_CONFIG_PARAMETERS`, `GIT_DIR`, `GIT_REPLACE_REF_BASE`…) del proceso del motor no alteran la lectura (SEC-10).
- `bail_if_untrusted(true)`: `gix` aplica el criterio de propiedad de `safe.directory` (honra las entradas que el usuario ya tenga) y un repo no confiable se reporta `ReadError::Untrusted` ("no disponible"), sin escribir config (SEC-11, Q17).
- **Neutralización de programas en memoria**: tras abrir se eliminan de la config *en memoria* (`config_snapshot_mut`, nunca se persiste) todas las secciones `filter.<driver>` y `diff.<driver>`. Así ni `status` ni ninguna otra lectura puede lanzar un filtro `clean`/`smudge`/`process`, un `textconv` ni un diff externo (SEC-09). Las conversiones de fin de línea integradas no son programas y se mantienen.
- Submódulos ignorados en `status` (`Submodule::Given { ignore: All }`): abrir un submódulo cargaría su config sin neutralizar.
- **Comprobado al implementar**: sin la neutralización, el `status` de `gix` **sí ejecuta** los drivers `filter.*` del repo (el canario falla); y con `git_binary = true` `gix` lanza `git` desde el `PATH` (el test de M1 falla). Ambos tests se verificaron rompiendo la protección a propósito.

**Lecturas** (ninguna usa APIs de escritura; el índice se lee y nunca se escribe; los cambios de stat que `gix` detecta se descartan):

| Función | Qué devuelve |
|---|---|
| `head()` | Rama, commit, separado o sin nacer |
| `local_branches()` | Ramas locales con su commit |
| `index_entry_count()` | Entradas del índice (lectura del índice sin refrescarlo) |
| `status()` | Cambios `staged` (HEAD↔índice), `unstaged` (índice↔working tree) y `untracked`, a nivel de ruta (es el "diff" que consume el motor; sin diff de líneas ni `textconv`). Stat sucio con contenido distinto sin filtros ⇒ `Modified` |
| `worktrees()` | Worktrees enlazados: id, ruta, `gitdir` y si está bloqueado |
| `in_progress()` | Operación en curso (merge, rebase, cherry-pick, revert, bisect, am) |
| `is_ignored(path)` | Reglas de ignore del repo y del usuario |
| `remote_url(name)` | URL del remoto **sin userinfo** |
| `merge_base(a, b)` | Base de fusión |
| `ahead_behind(a, b, limit)` | Recuentos acotados: `Count::Exact(n)` o `Count::AtLeast(limit)` si se alcanza el tope (repos > 100K commits) |

Los `RepoReader` son de vida corta (se abren por recomputo y se sueltan), para no retener mapeos de packs (consecuencia de Windows del ADR). En Windows, `std::fs` abre con `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE` por defecto.

## 5. Invocación del Git CLI (§ 3, SEC-05, SEC-10, SEC-02)

- `invoke.rs` construye cada hijo con el **ejecutable por ruta absoluta** (el resuelto en § 6), sin shell, stdin nulo y, en Windows, `CREATE_NO_WINDOW`.
- **Opciones fijas** antes del subcomando: `--no-optional-locks`, `-c core.fsmonitor=false`, `-c core.untrackedCache=keep`, `-c core.splitIndex=false`, `-c gc.auto=0`, `-c maintenance.auto=false`, `-c log.showSignature=false`, `-c credential.helper=`, `-c color.ui=false`, `-c core.pager=cat`, `-c trace2.normalTarget=`, `-c trace2.eventTarget=`, `-c trace2.perfTarget=`.
- **Entorno por allowlist** (`env_clear` + construcción): `HOME`, `PATH` sin entradas relativas y, en Windows, `SystemRoot`, `SystemDrive`, `USERPROFILE`, `HOMEDRIVE`, `HOMEPATH`, `APPDATA`, `LOCALAPPDATA`, `TEMP` y `TMP`; más las fijas `GIT_OPTIONAL_LOCKS=0`, `GIT_TERMINAL_PROMPT=0`, `GIT_PAGER=cat` y `LC_ALL=C`. El entorno padre entra como parámetro (por defecto `std::env::vars_os()`), lo que permite probarlo sin tocar el del proceso.
- **Tiempo máximo** por invocación (por defecto 10 s): al excederlo se mata el hijo y el resultado es `ReadError::TemporarilyUnavailable`. Salida acotada a 64 MiB.
- **Registro de argv** opcional (`ArgvSink`) para el modo diagnóstico; INF-GRP-001 lo persiste en el perfil y audita la allowlist.
- **Allowlist tipada** (`cli.rs`): `version`, `rev_parse_verify`, `for_each_ref`, `worktree_list` (`--porcelain -z`), `rev_list_left_right_count`, `merge_base`, `log` (formato fijo sin `%G*`, con `--no-ext-diff --no-textconv --no-show-signature`) y `config_get` de claves tipadas (`ConfigKey`: `init.defaultBranch`, `remote.<n>.url` redactada, `branch.<n>.remote`, `branch.<n>.merge`). `cat-file --batch` y `ls-files` no se implementan: `gix` cubre esas lecturas. `status`, `diff` y `config --list`/`--get-regexp` no existen.
- **Refs (SEC-02)**: todo `RefName` se valida antes de usarse. Interpretación de "siempre tras `--`": en `rev-parse`, `rev-list`, `merge-base` y `log`, `--` separa revisiones de *rutas*, así que las revisiones van tras **`--end-of-options`** (Git ≥ 2.24), que garantiza lo mismo: nunca se interpretan como opciones.
- Un repo rechazado por `safe.directory` en el CLI ("dubious ownership") se mapea a `ReadError::Untrusted`.

## 6. Resolución de Git (§ 4, SEC-10, Q28)

- **Candidatos en orden**: `engine.gitPath` (lo pasa el llamador; TS-GRP-001 lo lee del perfil), entradas absolutas del PATH heredado y rutas conocidas por SO (macOS: Homebrew, `/usr/local`, Command Line Tools, Xcode y el enlace de `xcode-select`; Linux: `/usr/bin`, `/usr/local/bin`, perfil de Nix; Windows: `%ProgramFiles%`, `%LOCALAPPDATA%` y Scoop). Las entradas relativas se ignoran. La lectura del registro de Windows queda pendiente junto con las ACE.
- **Validación sin ejecutar**: absoluto; se canonicaliza; archivo regular; propiedad del usuario actual (`gix-sec`) o de root; sin escritura para grupo ni otros; con bit de ejecución. En Windows: rechazo *fail-closed* (§ 1). Un `gitPath` inválido deja diagnóstico y la resolución sigue.
- **Shim de macOS**: un candidato que canonicaliza a `/usr/bin/git` solo se acepta si existe en disco el `git` de las Command Line Tools o de Xcode; si no, se descarta **sin lanzarlo**.
- **Versión**: `git version` con el invocador de § 5; se acepta 2.38.0 o superior (formatos `2.50.1 (Apple Git-155)` y `2.45.1.windows.1`).
- **Resultado**: `Resolution::Found { git, diagnostics }` o `Resolution::NotFound { diagnostics }`, donde cada diagnóstico dice el candidato y el motivo (ausente, inválido, shim sin toolchain, versión insuficiente, no responde). El estado "Esperando Git" y su recomprobación son de US-GRP-014.
- Las rutas conocidas y el shim son parámetros (`ResolveConfig`) para poder probarlos con ejecutables falsos.

## 7. Plan de pruebas (criterio → test)

Todos con repos temporales (`tempfile`), nunca este repo. La **huella** (`tests/common`) recorre el directorio Git común, `.git/worktrees/*` y cada working tree y registra ruta, tipo, tamaño, hash de contenido y mtime de archivos **y directorios** (excluye `atime`).

| Criterio de la TS | Test |
|---|---|
| Huella idéntica tras cada lectura | `boundary::every_read_leaves_repo_byte_identical` |
| fsmonitor | `boundary::fsmonitor_enabled_repo_is_untouched` |
| Índice con untracked cache, split index y stat sucio | `boundary::untracked_cache_split_index_dirty_stat_not_rewritten` |
| Filtros con stat sucio (SEC-09) | `canary::clean_filter_never_runs_and_file_reported_modified` |
| Programas del usuario | `canary::user_programs_never_run` (filtro, `textconv`, diff externo, `gpg.program` con commit firmado y `log.showSignature`, pager, `trace2.eventTarget`, hook `post-index-change`) |
| Concurrencia | `boundary::concurrent_agent_git_add_never_fails` |
| Allowlist y comprobación estática | `cli_invoke::argv_log_only_contains_allowlisted_subcommands`, `static_check::process_spawn_only_in_invoke_module` |
| Repo hostil | `resolve::relative_path_entries_are_ignored` (Unix); el `git.exe` de Windows queda **sin verificar** |
| Config (SEC-05) | `redaction_and_trust::remote_url_never_exposes_userinfo`, `redaction_and_trust::extra_header_not_reachable` |
| Entorno (SEC-10) | `cli_invoke::hostile_parent_env_does_not_reach_child`, `resolve::world_writable_git_rejected`, `resolve::relative_git_path_rejected` |
| Refs (SEC-02) | `refname` (unitarios), `cli_invoke::option_like_ref_rejected` |
| Resolución | `resolve::minimal_path_finds_known_location`, `resolve::macos_shim_not_launched_without_toolchain`, `resolve::old_git_reported_insufficient`, `resolve::invalid_git_path_diagnosed_and_resolution_continues` |
| Tiempo máximo | `cli_invoke::hung_invocation_is_killed_and_temporarily_unavailable` |
| safe.directory | `redaction_and_trust::untrusted_repo_is_unavailable` (rebaja de confianza forzada) e `invoke::tests::dubious_ownership_maps_to_untrusted` (CLI); un repo de otro uid real **no se verifica** (exige root) |
| M1: `gix` no lanza `git` | `redaction_and_trust::gitoxide_never_launches_git`: un proceso de test hijo con `PATH` solo de falsos (`git`, `sh`, `bash`) que dejan marcador |
| Controles (el arnés detecta lo que debe) | `boundary::fingerprint_detects_plain_git_status_index_refresh` y `canary::canary_is_armed_for_plain_git`: el Git CLI sin endurecer sí reescribe el índice y sí ejecuta el filtro y `gpg.program` |

## 8. Fuera de alcance y pendientes

- Capa de escritura de la Time Machine (TS-TMC-003) y de Guardrails; almacén del perfil (TS-GRP-001); "Esperando Git" (US-GRP-014); observador (US-GRP-002); ahead/behind frente a la rama base (US-GRP-012); validación bidireccional de `gitdir` (ADR-GRP-010, SEC-11 M2).
- **Para INF-GRP-001**: auditoría dinámica de `exec` (eslogger/ETW/strace), huella fuera del repo (config global, `~/.gnupg`), ejecución de control, `gc` concurrente, LFS real y los tres SO.
- **Windows**: comprobación de ACE del ejecutable, lectura del registro, `git.exe` hostil en el working tree y rutas UNC con captura de red. Sin verificar desde macOS.
- Tiempo máximo para las lecturas de `gix` (hoy solo en el CLI).
