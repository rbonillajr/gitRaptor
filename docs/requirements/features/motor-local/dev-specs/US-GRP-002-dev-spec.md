---
id: DS-US-GRP-002
title: "Dev Spec — El desarrollador ve los cambios y eventos de Git de sus worktrees casi al instante"
type: dev-spec
status: approved
feature: motor-local
domain: GRP
created: 2026-10-05
updated: 2026-10-05
related:
  stories: [US-GRP-002, US-GRP-001]
  enablers: [TS-GRP-001, TS-GRP-002, TS-GRP-003, TS-GRP-004, INF-GRP-001, INF-GRP-002, SPIKE-GRP-002]
  adrs: [ADR-GRP-005, ADR-GRP-007, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRP-013]
  rules: [BR-CONS-003, BR-CONS-005, BR-EDGE-001, BR-EDGE-005, BR-CONS-001]
  nfrs: [NFR-01, NFR-04, NFR-05, NFR-10, SEC-11, SEC-12]
tags: [motor-local, esqueleto-andante, watcher, notify, fsevents, debounce, reconciliacion, eventos-git, sin-atribuir, i18n]
---

# Dev Spec — US-GRP-002: cambios y eventos de Git en vivo

Plano compacto (AADD ligero) de [US-GRP-002](../user-stories/US-GRP-002-cambios-eventos-en-vivo.md), segunda mitad del esqueleto andante. Sobre lo que dejó US-GRP-001 (`repo add`, reconciliación al añadir y al arrancar, `worktree.state`), añade el **observador de cambios** de [ADR-GRP-010](../../../../architecture/decisions/ADR-GRP-010-observacion-cambios-worktrees.md) (con la Enmienda de SPIKE-GRP-002), el **historial de eventos de Git** de [ADR-GRP-013](../../../../architecture/decisions/ADR-GRP-013-modelo-eventos-atribucion.md) y la instrumentación de [ADR-GRP-011](../../../../architecture/decisions/ADR-GRP-011-presupuesto-frescura.md) § 3.

**Qué entrega**: con el motor observando, cada cambio de archivo de un worktree actualiza su estado sin intervención (`worktree.state` en el stream, `raptor status`), y cada evento de Git (commit, cambio, creación y borrado de rama, alta y baja de worktree, rebase, merge y push) queda en el historial del repo con su hora y el actor **"sin atribuir"**. Se consulta con `events.history` y con `raptor events [--json]`. El observador no escribe nada en el repo.

## 1. Medición previa: un watcher por worktree en macOS

ADR-GRP-010 § 1 dejó como candidata "un watcher por worktree en macOS" con la ⚠️ ASSUMPTION de que un alta o una baja no hagan perder eventos a los demás worktrees. Se mide con el experimento nuevo `stream_isolation` del prototipo ([`spikes/watcher-viability`](../../../../../spikes/watcher-viability/README.md), `--only stream_isolation`): un escritor crea un archivo por milisegundo en `wt-01` durante 3 s mientras se añaden y quitan 8 watches; 10 repeticiones por modo, `notify` 8.2, macOS 26 (Darwin 25.6), Apple Silicon.

| Modo | Archivos escritos | Recreaciones de stream | Archivos sin evento |
|---|---|---|---|
| Control (sin altas ni bajas) | 17.571 | 0 | **0** |
| Watcher compartido (el alta/baja recrea el stream de `wt-01`) | 18.137 | 80 | **89** (0 a 16 por repetición) |
| Un watcher por worktree (el alta/baja recrea solo el stream vecino) | 17.592 | 80 | **0** |

Resultado guardado en `spikes/watcher-viability/results/results-macos-stream-isolation.json`. **Se cumple la condición del ADR (0 pérdidas), así que en macOS se adopta un watcher por raíz vigilada** (D2). La reconciliación tras crear un stream se mantiene como red de seguridad. Linux y Windows siguen con un watcher compartido (límite de instancias de inotify; ReadDirectoryChangesW no recrea nada). Pendiente: etapa de validación multiplataforma.

## 2. Decisiones

Todas son **Decisión del orquestador (2026-10-05), validada por el Arquitecto** (técnica) **y el PO** (alcance), los dos con veredicto "aprobada con ajustes". La columna de la derecha recoge los ajustes que pidieron, ya incorporados.

| # | Decisión | Ajuste incorporado |
|---|---|---|
| D1 | **Alcance según la historia**: observador, debounce, reconciliación (arranque, alta, recreación, desbordamiento y periódica de 5 min), modo degradado por sondeo e historial de eventos "sin atribuir". **Fuera**: ahead/behind (US-GRP-012/016), estados especiales y estado en conflicto (US-GRP-003, Enmienda Cockpit), sesiones y atribución a agentes (US-GRP-007/009/010), evento `gap.recorded` y hueco de arranque (US-GRP-005), vigilancia de Guardrails (ADR-GRD-005) y recarga de configuración (US-GRP-013) | — |
| D2 | **Mecanismo**: `notify` 8.2.0 fijado con `=8.2.0` en `crates/core/Cargo.toml` (además de `Cargo.lock`). En macOS, **un watcher por raíz** (cada working tree y el directorio Git común si no cae dentro de la raíz principal), según la medición del § 1; en Linux y Windows, uno compartido con altas y bajas agrupadas (`paths_mut()`). Todo recursivo. Tras crear o recrear un stream se reconcilian los worktrees que cubre, **después** de arrancarlo | Arquitecto: enrutado por el **prefijo más largo**, porque Claude Code crea worktrees anidados (`.claude/worktrees/`) y un mismo cambio llega por dos streams; la ventana los funde |
| D3 | **Tareas**: un hilo por worktree (cambios del working tree y de su `HEAD`/`index`/marcadores) y uno por repo (refs, reflogs, `packed-refs` y `.git/worktrees/`). Cada uno tiene su **ventana fija** de debounce de 75 ms efectivos (programada a 75 ms menos la holgura del temporizador: 10 ms en macOS según SPIKE-GRP-002, 0 en los demás SO hasta que INF-GRP-002 la calibre) y es la tarea serializada de ADR-GRP-010 § 5. Una ráfaga de un worktree no retrasa a los demás. El hilo del observador solo enruta rutas; nunca lee Git | Arquitecto: **cachés de `gix`** (Enmienda de ADR-GRP-010 § 4): el lector se abre por recomputo y se suelta (política de `crates/git`, ADR-GRP-009), así que no hay cachés persistentes que duplicar entre los worktrees de un repo; el coste de abrirlo está dentro de los ~18 ms medidos |
| D4 | **Recomputo**: al cerrar la ventana, el hilo del worktree relee `head()` + `status()` completos con `crates/git` (sin caché de stat incremental) y solo publica si cambió la huella o el `HEAD`. Las rutas de `.git/objects/` y las de directorios ignorados por Git se descartan antes del debounce. SPIKE-GRP-002 midió ~18 ms por status de 5.000 archivos, dentro de los 150 ms. Caché de stat y publicación en dos fases quedan como optimizaciones si INF-GRP-002 muestra que no cabe | Arquitecto: la desviación queda como **Enmienda (2026-10-05, US-GRP-002) de ADR-GRP-010**. Sin filtro, un agente que compila en `target/` provocaría un recomputo cada 75 ms; por eso sí hay **filtro previo de ignorados con caché de directorios** (cada directorio se consulta a Git una vez; la caché se vacía al cambiar un `.gitignore` o `info/exclude`) y un test que comprueba que una ráfaga sostenida en un directorio ignorado no provoca recomputos |
| D5 | **Clasificación de eventos de Git** (hilo del repo), comparando la vista anterior con la nueva (puntas de ramas locales, ramas remotas conocidas, `HEAD` de cada worktree, worktrees registrados) y leyendo el **reflog** de la ref afectada **desde la punta anterior** (`RepoReader::reflog_since`, lectura nueva en `crates/git`; un evento por entrada, así que dos commits en la misma ventana son dos eventos): `commit` (reflog `commit…`), `merge` (`merge …` o `commit (merge)`), `rebase` (`rebase…`), otra actualización de rama → `branch-update`; rama nueva → `branch-create`, rama que falta → `branch-delete`; rama remota, nueva o actualizada, con reflog `update by push` → `push` (nunca `branch-create`; otra actualización remota no se registra: el fetch no es de esta historia); sin reflog (repo bare por defecto) → `branch-update`; worktree nuevo o desaparecido en `.git/worktrees/` → `worktree-create` / `worktree-delete`. El hilo del worktree emite `branch-switch` cuando cambia la rama de su `HEAD` **sin operación en curso** (durante un rebase, el `HEAD` separado no cuenta como cambio de rama) | Arquitecto: reflog completo desde la punta anterior (no solo la última entrada); push nunca como rama nueva; respaldo sin reflog. Los commits con `HEAD` separado quedan fuera (no hay rama que comparar): pendiente |
| D6 | **Worktree de un evento de ref**: el que tiene la rama en su `HEAD`; si ninguno, el worktree cuyo reflog de `HEAD` menciona la rama en su última entrada (`checkout: moving from X to Y`); si ninguno, el único cuyo `HEAD` apunta al commit de la rama; si no se puede decidir, el worktree principal (o el primero enlazado, en un repo bare) con `worktree_inferred: true` en los metadatos. Git no registra desde qué worktree se ejecutó `git branch -d`; el motor no lo inventa y lo marca. **Push**: la rama remota lleva a la rama local cuyo upstream (`branch.<b>.remote` + `branch.<b>.merge`) es ella, o a la del mismo nombre, y de ahí a su worktree por la regla anterior | PO: los tests de rama y push afirman `feat-login` con `worktree_inferred: false`; el respaldo inferido tiene su propio test en `crates/core`; `raptor events` en texto también marca el worktree inferido |
| D7 | **Persistencia y publicación** en el bucle del daemon (único escritor, ADR-GRP-005): los hilos envían `Control::Observed`; el bucle persiste en un lote (`AppendEvent` de cada evento sin sesión, `SetLastKnownState`, `UpsertWorktree`/`MarkWorktreeGone`, huecos), toma `t_persisted` y publica `worktree.state` (repo completo, como en US-GRP-001) y un `git.event` por evento. Persistir antes de publicar (ADR-GRP-013). Cada evento lleva `timings` con `t_recv` (primer evento de la ventana), `t_flush`, `t_computed`, `t_persisted` y `t_published`, y un `batch_id` por ventana | — |
| D8 | **Contrato** (aditivo: `API_VERSION` 2.1.0, `PROTOCOL_VERSION` sigue en 2): `git.event` v1 con `data` = `GitEventView {repo_id, seq, worktree, kind, actor, observed_utc_ms, utc_offset_s, details, gap}`; `seq` es la secuencia del almacén del repo (ADR-GRP-013 § 4), distinta de la del stream. `details` = `{branch?, from?, to?, commit?, remote?, worktree_inferred}` con texto `Untrusted`. Método nuevo **`events.history`** `{repo_id, worktree?, after_seq?, limit?}` → `{events: [GitEventView]}` (páginas de 200 como máximo; no reservado; **fuera del MCP** por llevar rutas, SEC-12). `git.event` sigue fuera de la allowlist del MCP. Sin `after_seq`, devuelve los `limit` más recientes en orden ascendente | Arquitecto: un daemon 2.0.0 sigue corriendo tras actualizar (el protocolo no cambia), así que `raptor events` mira `HelloResult.methods` y, si falta `events.history`, pide con i18n reiniciar el motor |
| D9 | **Actor**: el evento solo apunta a una sesión con evidencia positiva (ADR-GRP-013 § 3), y esta historia no crea sesiones, así que todo evento se guarda sin sesión y se expone como `{"actor":"unattributed"}` (BR-CONS-003). La resolución del actor ya lee la sesión si la hay (US-GRP-007/010 la completarán con los registros de atribución) | — |
| D10 | **Huecos del observador** (ADR-GRP-013 § 5): tres causas nuevas en `GapCause` (`watcher-overflow`, `stream-recreated`, `periodic-reconciliation`), con una migración del almacén (versión 2) que rehace la tabla `gaps` con el `CHECK` nuevo. Una reconciliación que encuentra diferencias **sin evento de Git que las explique** abre y cierra un hueco de su causa y registra un evento `reconciled` sin sesión enlazado a él. El evento `gap.recorded` del stream sigue siendo de US-GRP-005 | Arquitecto: como dice ADR-GRP-013 § 5, el desbordamiento y la recreación del stream **abren hueco siempre** (desde el último evento recibido hasta el fin de la reconciliación); solo la periódica lo abre únicamente si encuentra diferencias. Migración con las claves foráneas desactivadas **fuera** de la transacción y `foreign_key_check` antes del commit, con test sobre un almacén v1 con eventos enlazados a un hueco. PO: `raptor events` etiqueta `reconciled` como reconciliación, no como evento de Git |
| D11 | **Respaldo y periodicidad**: sondeo ligero cada 30 s en el hilo del repo (relee refs y `HEAD`; lo que encuentre se clasifica como en D5) y reconciliación completa periódica cada 5 min por worktree, escalonada y tras vaciar la ventana, con un contador de reconciliaciones que encontraron diferencias en el log de diagnóstico. Los intervalos viven en `WatchConfig`; su lectura de los niveles perfil y local es de US-GRP-013 | Arquitecto: la huella del sondeo incluye, por worktree, `HEAD`, tamaño y mtime del `index` y los marcadores de operación; si cambia sin evento, se reconcilia ese worktree |
| D12 | **Modo degradado**: si `watch()` falla para una raíz, ese worktree pasa a sondeo completo cada 2 s (sin NFR-04) y el daemon lo anota en el log con su motivo. El estado "observación degradada" en el contrato es de US-GRP-003/014 (preparado: el hilo sabe que está degradado). El intervalo no se adapta al número de archivos hasta tener la medición de INF-GRP-002 en worktrees grandes | — |
| D13 | **Validación del enlace (SEC-11)**: antes de vigilar un worktree enlazado se exige que su `.git` apunte de vuelta a `.git/worktrees/<id>` y que su raíz no sea `/`, `$HOME`, la raíz de una unidad ni un ancestro del directorio común. Si no cumple, no se vigila y queda `unavailable` (`untrusted`) | Arquitecto: incluir la raíz de unidad. El tope de watches por repo en Linux sigue pendiente (SEC-11, etapa multiplataforma) |
| D14 | **CLI**: `raptor events [--json] [--limit N]` lista los últimos eventos de los repos observados (hora local, worktree, tipo, rama y "sin atribuir"), con el i18n por catálogos de US-GRP-001 (`events.*` en `en.txt` y `es.txt`). `raptor status` ya refleja el estado en vivo porque lee la instantánea que actualiza el observador | — |

## 3. Estructura

```
crates/api      event.rs     git.event definido por US-GRP-002
                messages.rs  GitEventView, GitEventKind, GitEventDetails, EventsHistoryParams/Result
                methods.rs   events.history (no reservado, fuera del MCP); API_VERSION 2.1.0
crates/git      reader.rs    reflog_since(ref, punta), remote_branches(), branch_upstream() (solo lectura)
crates/core     watch/       mod.rs (Observer, WatchConfig, rutas y enrutado), watchers.rs (notify:
                             uno por raíz en macOS, compartido en el resto), worktree.rs (hilo por
                             worktree: ventana, recomputo, degradado, periódica), repo.rs (hilo por
                             repo: vista de refs, clasificación D5/D6, sondeo de respaldo)
                profile/     GapCause nuevas + migración 2 del almacén
                daemon/      Control::Observed y Control::EventHistory; el observador arranca con
                             el motor, se amplía en repo.add y se reduce en repo.retire
                channel/     conn.rs: events.history
apps/cli        main.rs (raptor events), events.rs (texto/JSON), i18n/{en,es}.txt
spikes/         watcher-viability: experimento stream_isolation (§ 1)
```

## 4. Plan de pruebas (escenario → test)

Escenarios de punta a punta en `apps/cli/tests/live_changes.rs` (macOS): binario `raptor` real como daemon y como cliente, repo temporal del arnés (INF-GRP-001) y el desarrollador bajo un pty, como en US-GRP-001. La frescura se comprueba como ADR-GRP-011 § 2: el test toma `t0` al terminar la escritura (o el comando de Git) y `t_client_recv` al recibir el primer evento que refleja el cambio, con el reloj común `clock::monotonic_ns`; `t_client_recv − t0` ≤ 300 ms (parte del motor). Es una comprobación por escenario, no el p95 de 200 muestras de INF-GRP-002.

| Escenario Gherkin | Test |
|---|---|
| Un cambio de archivo se refleja casi al instante | `a_file_change_is_reflected_almost_instantly`: se modifica `login.txt` en `feat-login`; llega un `worktree.state` con `login.txt` `unstaged/modified` dentro de 300 ms y `raptor status --json` lo muestra |
| Cada evento de Git queda registrado con su momento y su actor (9 ejemplos) | Un test por ejemplo (`git_event_commit`, `git_event_branch_switch`, `git_event_branch_create`, `git_event_branch_delete`, `git_event_worktree_create`, `git_event_worktree_delete`, `git_event_rebase`, `git_event_merge`, `git_event_push`), con una aserción común: el evento está en `events.history` en `feat-login` con `worktree_inferred: false`, su hora y `actor: unattributed`, y el estado de `feat-login` refleja el resultado. En la creación y el borrado de worktree, el worktree creado o borrado **es** `feat-login`: aparece en el estado o deja de estar. El de commit comprueba además `raptor events` en texto, en inglés y en español |
| Ningún evento se presenta como hecho por un agente sin atribución | `no_event_is_shown_as_done_by_an_agent_without_attribution`: commit del desarrollador; el evento no tiene sesión en el almacén, su actor es `unattributed` y la salida de texto dice "unattributed"/"sin atribuir" |
| Diez worktrees activos a la vez se siguen sin perder eventos | `ten_active_worktrees_are_followed_without_losing_events`: 10 worktrees, un commit en cada uno a la vez (hilos); el historial tiene los 10 commits, cada uno en su worktree, y cada `worktree.state` que los refleja llega dentro de 300 ms |

Transversal: `repo_intact_watching_the_repo_does_not_modify_it` con la huella del testkit (cambios, commit, rama y worktree con el motor observando; cero diferencias del motor fuera del perfil). Pruebas de `crates/core` (todos los SO): clasificación D5/D6 sobre repos temporales, recreación de stream con reconciliación, hueco de reconciliación periódica con un evento perdido a propósito, validación SEC-11 del enlace y migración 2 del almacén. Unitarias: ventana fija y holgura, enrutado de rutas, `GitEventView` en el contrato.

## 5. Pendientes

- **Linux y Windows**: los escenarios de proceso usan `script` y el canal Unix, así que corren solo en macOS. El watcher compartido de inotify (un watch por directorio, incluidos los ignorados: cota superior), el agotamiento de `max_user_watches` y los handles de Windows al borrar un worktree no se han verificado. Las pruebas de `crates/core` corren en los tres SO del CI. Pendiente: etapa de validación multiplataforma.
- **Gate de frescura y escala con p95** (200 muestras, repo de 100K commits, ráfaga de 10K): INF-GRP-002. **Holgura calibrada por SO**: INF-GRP-002.
- **Interfaces preparadas**: sesión y actor por evento (US-GRP-007/009/010), `gap.recorded` y hueco de arranque (US-GRP-005), "observación degradada" en el contrato (US-GRP-003/014), intervalos desde la configuración (US-GRP-013), ahead/behind en segunda fase (US-GRP-012), estado en conflicto (US-GRP-003), proyección MCP de los eventos (F-001-05).
- **Optimizaciones medibles** (D4): caché de stat incremental y reanudar FSEvents desde el último `FSEventStreamEventId`.
- **Commits con `HEAD` separado** (fuera de un rebase): no se registran como evento; el estado del worktree sí se actualiza.
