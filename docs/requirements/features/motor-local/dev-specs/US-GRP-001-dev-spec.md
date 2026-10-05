---
id: DS-US-GRP-001
title: "Dev Spec — El desarrollador ve el estado de cada worktree del repo que añadió"
type: dev-spec
status: approved
feature: motor-local
domain: GRP
created: 2026-10-04
updated: 2026-10-04
related:
  stories: [US-GRP-001]
  enablers: [TS-GRP-001, TS-GRP-002, TS-GRP-003, TS-GRP-004, INF-GRP-001]
  adrs: [ADR-GRP-003, ADR-GRP-005, ADR-GRP-006, ADR-GRP-009, ADR-GRP-010, ADR-GRP-013]
  rules: [BR-AUTH-001, BR-AUTH-002, BR-CONS-001]
  nfrs: [NFR-01, NFR-10, SEC-02, SEC-11, SEC-12, SEC-14]
tags: [motor-local, esqueleto-andante, worktrees, repo-add, repo-retire, cli, i18n, repo-intacto]
---

# Dev Spec — US-GRP-001: estado de cada worktree del repo añadido

Plano compacto (AADD ligero) de [US-GRP-001](../user-stories/US-GRP-001-estado-worktrees-repo-anadido.md), primera historia y esqueleto andante del motor. Une los enablers que ya están en `main`: almacén del perfil (TS-GRP-001), lectura de Git (TS-GRP-002), daemon (TS-GRP-003), canal y contrato (TS-GRP-004) y el arnés "repo intacto" (INF-GRP-001). El contrato resultante está en [api-contract-ipc.md](../../../../architecture/design/api-contract-ipc.md).

**Qué entrega**: `raptor repo add`, `raptor repo retire` y `raptor status [--json]`. El desarrollador añade un repo y ve, por el canal, la rama y los cambios sin commitear (con sus archivos) de cada worktree. Añadir, observar y retirar no escriben nada en el repo.

## 1. Decisiones

Todas son **Decisión del orquestador (2026-10-04), validada por el Arquitecto** (técnica) **y el PO** (alcance). La columna de la derecha recoge los ajustes que pidieron y que ya están incorporados.

| # | Decisión | Ajuste incorporado |
|---|---|---|
| D1 | **Alcance según la historia, no según el brief.** El brief pedía ahead/behind y estados especiales; la historia los asigna a US-GRP-012/016 y a US-GRP-003. No se incluyen | PO: el HEAD separado y la rama sin commits se exponen de forma neutra porque hacen falta para mostrar la rama sin inventarla. Esta historia **no** cubre los estados especiales de US-GRP-003 (rebase, merge, conflictos ni su presentación). Un worktree ilegible se marca `unavailable` con su motivo, sin más semántica |
| D2 | **`repo.retire` básico ya en US-GRP-001**: el escenario "Solo se observan los repos añadidos" exige retirar. Deja de observar y conserva los datos (Q25) | Arquitecto: persiste `observed_until` con la hora del retiro, para que US-GRP-005/006 abran el hueco del tramo retirado. US-GRP-006 sigue siendo dueña de recuperar el historial y del hueco |
| D3 | **Contrato**: `RepoView.worktrees: [WorktreeView]`. Cada worktree lleva `path`, `main`, `admin_name` y `status`, que es `ready {head, counts, changes}` o `unavailable {reason}`. `head` puede ser `branch {name}`, `unborn {name}` o `detached`. `counts` tiene `{staged, unstaged, untracked}` (todo a cero = limpio) y `changes` lleva `{path, area, kind}` | Arquitecto: `head` como enum, para que no haya combinaciones imposibles. Conteos por área en lugar de `clean`, para que truncar la lista no oculte cuánto hay. PO: la salida dice cuántos archivos hay en total cuando la lista se trunca |
| D4 | **Topes de tamaño**: 200 rutas y 32 KiB de texto de rutas por worktree. Si un evento `worktree.state` o una instantánea pasan de 768 KiB, se quitan las listas y se conservan los conteos | Arquitecto: sin tope en bytes, 10 worktrees con rutas largas superan el límite de 1 MiB por mensaje |
| D5 | **Versión**: `PROTOCOL_VERSION` 1 → 2 y `API_VERSION` 2.0.0. Todos los tipos rechazan campos desconocidos, así que un cliente viejo no entiende el snapshot nuevo. `daemon.replace` (SEC-13) resuelve la convivencia | Arquitecto: es un cambio incompatible, así que la versión de la API sube a 2.0.0 y no a 1.1.0 |
| D6 | **`repo.add`**: se validan los parámetros sin tocar el disco (SEC-02), después el daemon autoriza y audita el comando reservado y solo entonces lee la ruta. Así un agente no puede usar al daemon para sondear el sistema de archivos. La ruta es la raíz de un worktree o el directorio Git, **sin buscar hacia arriba** (ni el daemon ni la CLI): una carpeta dentro de un repo no es ese repo, y "notas" no se añade por estar dentro de otro repo. Si se rechaza, devuelve `-32008 REPO_REJECTED` con `reason`: `not-a-repo`, `untrusted`, `unreadable` o `unknown-repo` | Arquitecto: código genérico, porque `untrusted` no es "no es un repo". Nuevo `ReadError::NotARepository` en `crates/git`. gix 0.88 borra el tipo del error de apertura, así que se decide con una comprobación estructural de solo metadatos: sin `.git` y sin `HEAD` + `objects/` |
| D7 | **Reconciliación** (`observe::reconcile`): lectura completa sin acceso al perfil. El worktree principal sale del repo abierto en el directorio común (un repo bare no tiene) y los enlazados, de `worktrees()`, ordenados por ruta. Cada uno se lee con `head()` + `status()`. Un worktree ilegible queda `unavailable` (`missing`, `untrusted` o `unreadable`) y los demás se leen igual (BR-EDGE-001). Rutas canónicas (`/var` → `/private/var` en macOS) | Arquitecto: la llaman el hilo de la conexión (en `add`) y el arranque, con un hilo por repo. El bucle del daemon, dueño del perfil, vuelve a mirar el perfil (dos `add` del mismo repo dejan una sola entrada) y persiste |
| D8 | **Persistencia** en el almacén del repo (base de comparación de US-GRP-002): `UpsertWorktree` por cada worktree legible; `SetLastKnownState` con `head` = commit, `refs` = puntas de las ramas locales (`nombre commit` por línea, ordenadas) y `dirty_fingerprint` = SHA-256 del status **completo** ordenado; y `MarkWorktreeGone` para los que ya no están. `operation` queda vacío (US-GRP-003) | Arquitecto: la huella sale del status completo, no de la lista truncada, y se guardan las puntas de refs |
| D9 | **Eventos**: `worktree.state` (de cambio, con `timings`; `data` = `{repo_id, worktrees}` del repo completo) al añadir. `repo.observation` (del motor) con `{repo_id, observed, state, path}` al añadir o retirar. Las transiciones de BR-WF-002 (`no-repos` ↔ `observing`) publican `engine.state` | Arquitecto: `repo.observation` lleva ruta y estado, así el cliente no necesita otro snapshot. `mcp_kind` pasa de lista de exclusión a **allowlist** (`engine.state`, `daemon.stopping`): ningún tipo con rutas llega a `raptor-mcp` (SEC-12) hasta que F-001-05 defina su proyección |
| D10 | **Sin reconciliación periódica ni watcher.** El estado se recalcula al añadir y al arrancar el motor (ADR-GRP-010 § 6) | Arquitecto: la reconciliación periódica y lo que encuentra van en la tarea serializada tras el debounce, con un hueco de "reconciliación periódica" (ADR-GRP-010 § 5, ADR-GRP-013 § 5). Sin watcher ni huecos, publicaría cambios sin ese hueco, así que se mueve a US-GRP-002. Hasta entonces, `raptor status` muestra el estado del último alta o arranque |
| D11 | **i18n mínimo de la CLI**: dos catálogos embebidos en el binario (`apps/cli/i18n/{en,es}.txt`, `include_str!`), con líneas `clave = texto` y marcadores con nombre (`{path}`), sin dependencias. El idioma sale de `LC_ALL`, `LC_MESSAGES` o `LANG`. Los mensajes que ya existían (`daemon stop`, `daemon status`) se migran a claves. Un test exige que los dos catálogos tengan las mismas claves y los mismos marcadores. El daemon nunca devuelve texto para el usuario final, solo códigos y razones | Arquitecto: extraer `tr(en, es)` con literales no cumple ADR-GRP-003 / NFR-10 (mensajes en archivos de recursos, sin concatenar). Solución: catálogos con claves |
| D12 | **CLI**: `raptor repo add [RUTA]` y `raptor repo retire [RUTA]` (la ruta es el directorio Git, la carpeta del repo o cualquiera de sus worktrees; por defecto, la carpeta actual) y `raptor status [--json]`. El texto sale saneado (SEC-12). El JSON tiene forma estable, con cadenas planas en lugar del envoltorio `{"untrusted": …}`: el escape de JSON deja inertes los controles de terminal. La TUI es del Cockpit (F-001-02) | PO: sin repos, `status` devuelve la lista vacía; el estado guiado es de US-GRP-015 |

## 2. Estructura

```
crates/api      messages.rs  WorktreeView, HeadView, WorktreeStatus, ChangeCounts, FileChangeView,
                             RepoAddResult, RepoRetireResult, RepoRejection, RepoObservationData,
                             WorktreeStateData; MAX_WORKTREE_CHANGES / _BYTES
                methods.rs   repo.add y repo.retire implementados (reservados, fuera del MCP)
                event.rs     repo.observation (motor) y worktree.state (cambio)
                rpc.rs       REPO_REJECTED = -32008
crates/git      reader.rs    is_bare(); ReadError::NotARepository al abrir
crates/core     observe.rs   locate(), reconcile(), RepoRead::store_ops()  (sin perfil)
                daemon/      Control::RepoAdd / RepoRetire; add_repo, retire_repo, transition;
                             reconciliación de arranque en paralelo
                channel/     conn.rs: repo_add / repo_retire; snapshot con tope;
                             bus.rs: allowlist MCP
apps/cli        i18n.rs + i18n/{en,es}.txt, status.rs (texto/JSON), main.rs (repo add|retire, status)
```

## 3. Plan de pruebas (escenario → test)

Escenarios de punta a punta con el binario `raptor` real como daemon y como cliente, en un repo temporal del arnés (`apps/cli/tests/repo_state.rs`, macOS). El desarrollador usa un pty (`script`); el agente es el agente simulado de TS-GRP-004 (`GITRAPTOR_AGENT_EXECUTABLES`, solo en debug).

| Escenario Gherkin | Test |
|---|---|
| El estado de cada worktree queda disponible al añadir el repo | `the_state_of_each_worktree_is_available_once_the_repo_is_added`: 2 worktrees con su rama; `feat-login` con `login.txt` `unstaged/modified`; el principal, limpio. También en texto. El suscriptor recibe `repo.observation` y `worktree.state` con `timings` |
| Solo se observan los repos añadidos | `only_the_added_repos_are_observed`: está "demo" y no "otro"; al retirarlo, la lista queda vacía y el motor pasa a `no-repos` |
| Un agente no puede añadir ni retirar repos | `an_agent_cannot_add_or_retire_repos`: añadir "otro" o retirar "demo" como agente (también con pty propio) se rechaza con "only the developer can change the observed repos". La lista no cambia y la auditoría tiene 3 intentos `rejected` / `agent-ancestry` |
| Un directorio que no es un repo Git no se añade | `a_folder_that_is_not_a_git_repo_is_not_added`: "notas is not a Git repository" (y en español con `LANG=es_ES.UTF-8`); la lista no cambia |
| Observar el repo no lo modifica | `repo_intact_observing_the_repo_does_not_modify_it`: huella del testkit con un archivo modificado, uno preparado, uno no rastreado, un stash, una rama remota conocida, un worktree enlazado, un hook propio y un stat sucio. Se añade, se observa, se retira y se para el motor: cero diferencias fuera del perfil (`data`, `state`, `run`). Comprobado que no es vacío: una escritura deliberada en `.git` se detecta |
| Preparar todos los cambios no recoge nada del motor | `staging_everything_picks_up_nothing_from_the_engine`: con el motor observando, `git add -A` prepara solo `login.txt` |

Además: `crates/core/tests/observe.rs`, que corre en todos los SO. `repo_intact_every_worktree_is_read_and_one_that_is_gone_is_unavailable` usa la huella estricta: un worktree borrado queda `missing` y los demás se leen. También cubre el HEAD separado, la rama sin commits, el repo bare sin worktree principal y el rechazo de "notas" y de una ruta inexistente. Las pruebas unitarias cubren los topes de la lista, la huella del status, que los dos catálogos coincidan, la salida de texto saneada y la forma del JSON.

## 4. Pendientes

- **Watcher, debounce, reconciliación periódica y su hueco**: US-GRP-002 (D10). Hasta entonces el estado no se actualiza solo.
- **Ahead/behind contra la rama base**: US-GRP-012 / US-GRP-016. **Estados especiales y "ya no existe"**: US-GRP-003. **Historial y hueco al volver a añadir**: US-GRP-006. **Proyección MCP del estado de worktrees**: F-001-05.
- **Linux y Windows**: los escenarios de proceso usan `script` y el canal Unix, así que solo corren en macOS. En Windows no hay canal todavía (`TRANSPORT_UNSUPPORTED`). `crates/core/tests/observe.rs` corre en todos los SO del CI. Pendiente: etapa de validación multiplataforma.
- **Interfaces preparadas**: `WorktreeStatus` admite estados nuevos (US-GRP-003); `RepoView` / `WorktreeView` pueden crecer con una subida de protocolo (ahead/behind, sesiones de US-GRP-007); `KnownState.operation` espera a US-GRP-003; y la configuración por repo (US-GRP-013) se leerá en `Daemon::add_repo` y en `reconcile_all`.
