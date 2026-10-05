---
id: DS-TS-TMC-001
title: "Dev Spec — Almacén de snapshots en el perfil y captura de estado"
type: dev-spec
status: review
feature: time-machine
domain: GRP
story: TS-TMC-001
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-TMC-001, ADR-TMC-002, ADR-TMC-003, ADR-TMC-004, ADR-TMC-006, ADR-GRP-006, ADR-GRP-009, ADR-GRP-010]
  nfrs: [NFR-01, NFR-04, SEC-TMC-01, SEC-TMC-06, SEC-TMC-09, SEC-TMC-12]
tags: [time-machine, snapshots, almacen, captura, gitoxide, siembra, rendimiento, crates-git, crates-core]
---

# Dev Spec — TS-TMC-001: almacén de snapshots y captura

Blueprint compacto de [TS-TMC-001](../technical-stories/TS-TMC-001-almacen-captura-snapshots.md). Fuentes: ADR-TMC-001 (forma, contenido, siembra y aislamiento), ADR-TMC-002 § 1 (frontera del escritor gitoxide), ADR-TMC-004 § 2 (prioridad del previo), ADR-TMC-006 (presupuesto por etapa; escalones 2 y 3 obligatorios), SEC-TMC-01/06/09 y el prototipo `gix-hint` de SPIKE-TMC-001. Implementada en esta misma rama.

**Estado `review`**: la TS queda **implementada en parte**. Lo que falta está en § 9 con su historia de destino, y falta la verificación manual de la TS (siembra con un repo real en la máquina de dogfooding).

## 1. Ubicación en el código

Decisión del orquestador (2026-10-04), validada por el Arquitecto: el escritor gix vive en `crates/git/src/tm_write/store/`, dentro de la capa de escritura de la Time Machine que ya define ADR-TMC-002. Así TS-TMC-003 añade el aplicador a su lado sin mover nada.

| Archivo | Responsabilidad |
|---|---|
| `crates/git/src/tm_write/store/mod.rs` | `StoreRepo` y `StoreHandle`: **el único código del crate que escribe con gix**. Abre solo un almacén validado (`<tm>/<id>/store.git`, carpeta real del usuario sin permisos para otros, dentro de `tm/` ya canonicalizado, bare y con la marca `gitraptor.store=1`). Crea el almacén en una carpeta temporal y lo renombra. Escribe blobs en memoria o en flujo (con sondeo de cesión cada 256 KiB), árboles con el editor de gix y commits. Gestiona las refs `refs/tm/snap/<id>` |
| `.../store/durable.rs` | `fsync` simple por objeto (con `rustix`; `std::File::sync_all` hace `F_FULLFSYNC` en macOS), barrera completa y comprobación de carpetas privadas |
| `.../store/seed.rs` | Siembra desde los packs del usuario |
| `.../store/anchor.rs` | Anclaje: copia lo que falta leyendo con `RepoReader` y comprueba que el id escrito sea el pedido |
| `.../store/verify.rs` | Re-hash del commit, del árbol y de todo lo que cuelga de él (SEC-TMC-09) |
| `crates/git/src/capture.rs` | Lecturas nuevas de solo lectura sobre `RepoReader`: índice con stat y marcas, firma barata del índice (stat + checksum final), recorrido de no seguidos con gix sin ignorados, atributos de conversión, reglas de ignore reutilizables, puntas de rama y `HEAD` sin cargar packs, stash y huecos de historial (*shallow*, *partial clone*) |
| `crates/core/src/timemachine/store/mod.rs` | `SnapshotStore`: abre o crea el almacén, lo aparta si no es de confianza, lo excluye de las copias de seguridad, siembra, ancla, verifica, expone el tamaño e implementa `SnapshotRefs` |
| `.../store/capture.rs` | Captura: detección, contenido bruto, exclusiones, camino rápido, blobs en paralelo, árboles, punto de validez y prioridad del previo |
| `.../store/meta.rs` | Blob `meta` (JSON, `format: 1`, `deny_unknown_fields`) |
| `crates/core/src/daemon/mod.rs` | La recuperación al arrancar usa el almacén real si existe y es de confianza; si no, `AbsentStore` |
| `crates/testkit/src/repogen.rs` | Generador determinista de SPIKE-TMC-001 (`S`, `P50`, `M`, `L`). Con la semilla 42, `M` da `HEAD=1a8bd23…`, el mismo commit que en el spike |
| `crates/core/benches/tm_snapshot.rs` | Banco del p95 frente a 200 ms |

`crates/core` no depende de gix. Dos comprobaciones estáticas protegen la frontera: `crates/git/tests/static_check.rs` (las funciones de escritura de gix solo aparecen en `tm_write/store/`, y `RepoReader` no expone su `gix::Repository`) y `crates/core/tests/tm_boundary.rs` (solo `crates/core/src/timemachine/` usa `tm_write`).

## 2. Dependencias

| Cambio | Dónde | Por qué |
|---|---|---|
| `gix-pack` 0.75 (`streaming-input`, sin features por defecto) | workspace, `crates/git` | Regenerar el `.idx` de un pack sembrado en el proceso. Es la misma versión que ya trae gix 0.88 |
| Feature `parallel` de gix | workspace | Hace `ThreadSafeRepository` compartible entre hilos para escribir blobs en paralelo. El spike midió con `max-performance-safe`, que la incluye. No añade red, credenciales ni mutación del worktree. Aprobada por el Arquitecto con una condición, ya aplicada: el `status` de `RepoReader` fija `thread_limit = 1` para que las lecturas del motor sigan acotadas (ADR-GRP-009 § 3) |
| `rustix` (`fs`, `process`) | `crates/git` (unix) | `fclonefileat`, `ioctl_ficlone`, `fsync` simple, `F_FULLFSYNC`, `openat` sin seguir enlaces, xattrs, `geteuid` |

Licencias MIT o Apache-2.0. No se añade ninguna feature de gix con red.

## 3. Almacén (`<datos>/tm/<id-repo>/store.git`)

- **Configuración** escrita por el propio módulo, sin plantillas de gix (ni `hooks/` de ejemplo ni `description`): `gc.auto=0`, `maintenance.auto=false`, `core.logAllRefUpdates=false`, `core.hooksPath` a `tm/<id>/nohooks` (vacía, 0700), `core.compression=1`, `core.looseCompression=1` (zlib 1 en el escritor gix), `pack.compression=1`, `core.bigFileThreshold=128k`, `core.fsync=committed`, `core.fsyncMethod=batch` (para el mantenimiento con Git CLI) y `gitraptor.store=1`. Sin remotos.
- **Permisos**: carpetas 0700 y archivos de configuración 0600 desde su creación; el umask 077 del daemon cubre lo que crea gix. Al abrir se comprueban propietario, permisos, que no sea un enlace y que la ruta canónica quede dentro de `tm/`. Un almacén que no pasa se **aparta** (renombrado a `store.git.aside-<ms>`, nunca borrado) y se crea uno nuevo (`StoreStatus::Replaced`). La recuperación al arrancar nunca aparta ni crea: si no hay almacén de confianza, no decide nada sobre refs.
- **gix aislado**: `open::Options::isolated()` (ni configuración de sistema, global ni del entorno, nunca un proceso `git`), `strict_config` y `ignore_replacements`.
- **Copias de seguridad** (SEC-TMC-06): sobre `tm/` (no solo `tm/<id>/`), el atributo extendido de Apple `com.apple.metadata:com_apple_backup_excludeItem` con el bplist de `com.apple.backupd` (lo mismo que escribe `tmutil addexclusion`) y un `CACHEDIR.TAG`.

## 4. Forma del snapshot (ADR-TMC-001 § 1)

Commit del almacén con `wt/<clave>/files`, `wt/<clave>/index` y `meta`. Sus padres son los commits de `HEAD` de cada worktree del ámbito, de `refs/heads/*` y de `refs/stash`, anclados antes. La ref `refs/tm/snap/<id-snapshot>` se crea con `PreviousValue::MustNotExist`. El mensaje del commit anota la detección de cada worktree (`engine` o `full:<motivo>`).

`meta` contiene el ámbito; por worktree, `HEAD` (rama, commit, separado) y las marcas del índice que un árbol no guarda (*intent-to-add*, *skip-worktree*, entradas en conflicto con su etapa); la lista de worktrees registrados (ruta, rama, bloqueo); las ramas; el stash; las exclusiones con su motivo; y los huecos de historial. **No** lleva el id ni la hora, para que el camino rápido pueda reutilizar el árbol raíz. La clave de worktree es `[A-Za-z0-9._-]{1,64}`; las rutas de `meta` son UTF-8 con pérdida, y las del árbol son bytes exactos.

## 5. Captura

**Lecturas**: solo con `RepoReader` (gix en solo lectura, programas neutralizados, sin locks). Nunca `git status`. Nunca se escribe ni se refresca el índice del usuario. gix recorre las carpetas por su cuenta y no usa la untracked cache (`core.untrackedCache=false` en la práctica). Lo prueba un test con una caché UNTR desactualizada a propósito.

**Árbol `index`**: entradas en etapa 0 sin *intent-to-add*, editadas sobre el árbol anterior solo cuando cambia la firma del índice (stat **y** checksum final, que distingue dos escrituras en el mismo tic). Los blobs preparados que el almacén no tiene se anclan (copian) desde el repo.

**Detección** (escalón 2, diseño base):

- **Con continuidad**: el llamador pasa `ChangeHint { since, mark, paths, continuous }`. Solo se hace `lstat` de esas rutas, más las que cambiaron de seguidas a no seguidas en el índice. Se comparan con la caché de stat.
- **La continuidad se da por rota mientras no se demuestre**, y entonces la detección es **completa**: primera captura o daemon reiniciado (`first-capture`), sin pista (`no-hint`), `continuous=false` (`not-continuous`), `since` distinto de la marca de la captura anterior (`mark-mismatch`), cambio de `info/exclude` o `core.excludesFile` o un `.gitignore` en las pistas (`ignore-rules-changed`), una ruta inválida (`invalid-hint`: absoluta, con `..` o `.git`) o una carpeta en las pistas (`folder-in-hint`).
- **Detección completa**: cada entrada del índice frente a la caché de stat o al stat de su entrada (reglas de Git: mtime, tamaño, inodo y tipo, y **no** *racy* frente al mtime del índice), más los no seguidos que encuentra gix (nunca los ignorados).
- **Qué se re-lee**: stat distinto; entrada de la caché *racy* (mtime ≥ inicio de la captura que la leyó); entrada del índice *racy*; ruta con atributos de conversión (`text`, `eol`, `crlf`, `filter`, `ident`, `working-tree-encoding`, o `core.autocrlf` activo) sin caché, porque su blob del índice no son sus bytes en disco.

**Contenido bruto**: los archivos se abren con `O_NOFOLLOW`, con el stat tomado del descriptor. Los enlaces guardan su destino. Los archivos de más de 1 MiB van en flujo (hash y zlib en una pasada) y se comprueba que el tamaño no cambie mientras se leen. Los modos posibles son archivo, ejecutable, enlace y gitlink.

**Exclusiones** (en `meta` y en el `CompleteInfo` del oplog, como `<clave>:<ruta>` y su motivo): `submodule` (se guarda el gitlink), `nested-repo`, `credential` (lista cerrada de SEC-TMC-06, solo sin seguimiento: `.env*`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `id_rsa*`, `id_ed25519*`, `.npmrc`, `.pypirc`, `.netrc`, `*.tfstate*`, `credentials*.json`) y `too-large` (> 50 MB, **solo** en la captura por observación, que queda `partial`). Los huecos `shallow` y `partial-clone` van con ruta vacía. Los ignorados no se declaran uno a uno.

**Camino rápido**: si ningún árbol de worktree cambió y `meta` es idéntica a la de la última captura del mismo ámbito, se reutiliza el árbol raíz. Se paga un commit nuevo, la ref y la fila.

**Estado en memoria** (caché de stat, árboles, marca por worktree): se pierde al reiniciar. Si una captura falla o cede, se conserva lo leído y se borra la marca, así que la siguiente hace detección completa (§ 6).

## 6. Escritor y prioridad (ADR-TMC-004 § 2)

- Un escritor por almacén (`Mutex`). Los blobs se escriben en hasta 8 hilos que reparten el trabajo con un contador atómico; cada hilo tiene su `StoreHandle`. Los árboles se escriben con el editor y se sincronizan en paralelo.
- **Prioridad**: un previo garantizado se anuncia con un contador atómico protegido por guarda RAII (un `panic` no lo deja anunciado) antes de pedir el escritor.
- La captura por observación consulta ese contador en estos puntos: al empezar, antes de reconstruir el árbol `index`, cada 512 entradas de una detección completa, antes de recorrer los no seguidos, tras cada worktree, antes de cada archivo, **dentro** de cada blob en flujo (cada 256 KiB), antes de los árboles y antes del punto de validez.
- Si hay un previo esperando, la observación devuelve `CaptureError::Yielded` sin ref ni fila. Los objetos sueltos que deja los recogerá el mantenimiento.
- Al ceder o fallar se conserva lo leído (la caché de stat y el árbol `index` siguen siendo ciertos), pero se borra la marca del worktree: la siguiente captura hace detección completa sin reconstruir el índice.
- Lo que el previo no puede interrumpir es la escritura de árboles y el punto de validez de una observación ya avanzada.

## 7. Punto de validez y durabilidad (ADR-TMC-001 § 1 y § 4, ADR-TMC-003)

Orden: objetos (cada uno con `fsync` simple) → fila `pending` → ref (`fsync` del archivo y de sus carpetas) → fila `complete`. Si la ref no se puede crear, la fila pasa a `discarded`.

- **El oplog corre con `PRAGMA fullfsync` y `checkpoint_fullfsync`** además de `synchronous=FULL`. Antes no estaban activados, y en macOS una fila `complete` podía perderse con un corte de luz después de ejecutar la operación protegida (riesgo para NFR-01 que señaló el Arquitecto).
- **La barrera de ADR-TMC-001 § 4 es el flush de la fila `pending`**. Decisión del orquestador (2026-10-04), validada por el Arquitecto. `F_FULLFSYNC` vacía la caché del disco, así que todos los objetos ya sincronizados llegan a almacenamiento estable antes de que exista la ref. No se paga una barrera aparte.
- **Se descartó una barrera única en `complete`**. Se probó `pending` sin flush, y ref + oplog bajaba de unos 35 a 19–24 ms p95. El Arquitecto lo rechazó en su forma actual: el lote de `pending` mueve también la cabeza de la cadena (`oplog.head`), que podría quedar en disco por delante de filas no durables, y el oplog acabaría en cuarentena por una manipulación que no existió. Hacerlo bien exige cambiar la tolerancia de la cabeza de TS-TMC-002 con su test de corte. Queda como mejora pendiente (§ 9).
- **La cabeza de la cadena** conserva `sync_all` en su archivo, así que sus datos son durables antes del `rename`. La **carpeta** pasa a un `fsync` simple. Decisión del orquestador (2026-10-04), validada por el Arquitecto. El `rename` se persiste con el flush completo del commit SQLite del lote siguiente, y hasta entonces la cabeza solo puede quedar un lote por detrás, que `check_head` ya tolera: nunca por delante ni dos lotes por detrás. Ahorra un `F_FULLFSYNC` por escritura del oplog.
- **Coste que queda**: cuatro `F_FULLFSYNC` por snapshot, dos por escritura del oplog (el commit SQLite y el archivo de cabeza). Sin carga, ref + oplog se queda en unos 20 ms en el camino rápido.

`SnapshotStore` implementa `SnapshotRefs`, así que solo se ofrecen snapshots con ref **y** fila `complete`.

## 8. Tiempos, diagnóstico y banco

Cada captura devuelve `StageTimings` (cola, detección, anclaje, blobs, árboles + commit, ref + oplog y total, con reloj monótono), más los archivos y bytes leídos, el camino rápido, la detección por worktree, las exclusiones, si fue parcial y los bytes nuevos. `SnapshotStore::size_bytes()` da el tamaño del almacén.

Banco: `cargo bench -p gitraptor-core --bench tm_snapshot [-- --profile M --iters 100 --worktrees 10]`, en `TM_BENCH_ROOT` o en una carpeta temporal fuera de cualquier repo. Hace esto:

1. Genera el perfil (lo reutiliza en las siguientes pasadas) y siembra un almacén nuevo.
2. Mide 100 iteraciones (más 3 de calentamiento) por delta, con pistas continuas: 0, 1, 10, 100, 100 / ~20 MB y 1.000 archivos.
3. Mide el previo de `w0` mientras 9 worktrees capturan por observación una vez por segundo.
4. Compara la huella de `.git` antes y después.
5. **Falla si el p95 del delta de referencia llega a 200 ms** (con 1 y con 10 worktrees) o si cambia el repo. Avisa por etapa y si la espera del escritor pasa de 10 ms.

### Resultados (macOS, Apple M5, 10 núcleos, Git 2.50.1; perfil `M`, `HEAD=1a8bd23…`; n = 100)

La máquina **no estaba en reposo**: otros agentes de Orca compilaban en paralelo y la carga media varió entre 6,7 y 8,8 en las pasadas con el código final. Siembra: 2,5 s (clon APFS de un pack de 610 MiB, 642.132 objetos re-hasheados). Huella de `.git` idéntica en todas las pasadas.

| p95 total (ms) | Pasada A (carga 6,8–8,8) | Pasada B (carga 6,7–7,3) | Referencia (spike, `gix-hint`) |
|---|---|---|---|
| Camino rápido | 26,9 | 29,2 | 4,1 |
| 1 archivo / 10 / 100 de texto | 29,9 / 38,0 / 104,5 | 29,7 / 37,2 / 97,0 | 7,8 / 16,2 / 73,5 |
| **Delta de referencia, 1 worktree** | **120,8 ✅** | **126,8 ✅** | 142–146 |
| 1.000 archivos (fuera de referencia) | 491,4 | 391,4 | 357–367 |
| **Delta de referencia, 10 worktrees** | **145,5 ✅** | **217,0 ❌** | — |
| 100 de texto, 10 worktrees | 205,5 ❌ | 345,8 ❌ | 79,8 |

- **1 worktree**: cumple con margen en todas las pasadas. Detección 1–5 ms, blobs 44–46 ms y árboles 40–45 ms, dentro de presupuesto. **Ref + oplog: 42–43 ms, por encima de sus 25 ms** (solo aviso). Son cuatro `F_FULLFSYNC` por snapshot frente a uno en el spike (§ 7).
- **10 worktrees**: con esta carga **no cumple de forma estable**. Los árboles se disparan (114–233 ms p95, con máximos de 2 s) y la espera del escritor pasa de 10 ms (23–26 ms).
- Una pasada anterior con carga 3,7 y la variante de una sola barrera (descartada en § 7) dio 96 / 104 / 122 ms, todo en verde. No es el código final y no se presenta como resultado.
- **Hipótesis sin verificar**: el coste por captura de las barreras completas del oplog, multiplicado por 9 observaciones por segundo, y la E/S de la máquina compartida. Separarlas y bajar a una barrera por snapshot es el trabajo pendiente de § 9. ADR-GRP-011 § 4 prevé un runner dedicado para este gate.

## 9. Fuera de este corte (pendiente)

Decisión del orquestador (2026-10-04), validada por el PO y el Arquitecto: la TS queda implementada en parte, con estos pendientes explícitos.

| Pendiente | Dónde se resuelve |
|---|---|
| Mantenimiento del almacén (`repack -d --geometric=2`, periodo de gracia) y su prueba "captura en curso durante el mantenimiento" | `repack` y `prune` ya están en la lista cerrada de ADR-TMC-002 § 2 y los expone la capa de escritura (`StoreRepo::repack`/`prune`, TS-TMC-003); cuándo corren y su gracia → US-TMC-016 (ADR-TMC-007 § 4) |
| Cuotas de disco (SEC-TMC-12). La **reserva del snapshot previo** es requisito previo de US-TMC-004 (en cuanto exista la observación, compite por el disco) y de US-TMC-016 | US-TMC-004 / US-TMC-016. Mientras no haya disparadores, nada captura solo |
| Disparadores del daemon: anclaje al publicar commits, verificación periódica fuera de la ruta crítica, cadencia de la observación | Daemon + US-TMC-004. `anchor`, `verify` y la detección completa ya existen |
| Interfaz del motor "rutas cambiadas desde la marca X" | TS-GRP-002/003. La API ya acepta las pistas; sin ellas, detección completa (siempre correcta, fuera del presupuesto) |
| Opción del perfil para incluir la lista de credenciales | US de configuración del perfil |
| **Gate de 10 worktrees bajo carga** y una sola barrera por snapshot (en `complete`): en el banco final, ref + oplog son 42–58 ms p95 y el gate de 10 worktrees falla con la máquina cargada (§ 8) | Ajustar la tolerancia de la cabeza del oplog (TS-TMC-002) con su test de corte, y enmendar ADR-TMC-001 § 4, ADR-TMC-003 y ADR-TMC-006 § 2 (§ 7) |
| Escribir los árboles de un snapshot en un pack en lugar de objetos sueltos (con 100 archivos dispersos se reescriben unos 250 árboles) | Optimización si el gate de CI lo pide |
| Progreso en CLI/TUI para un previo con archivos grandes (US-TMC-020, escenario 3) | US-TMC-020; no es de esta TS |
| Linux y Windows | **Pendiente: etapa de validación multiplataforma**. Linux compila con `FICLONE`, copia y `fsync`, sin ejecutar aquí. En Windows, `StoreRepo` devuelve `Unsupported` |

## 10. Verificación

| Criterio de la TS | Test |
|---|---|
| Garantías: push (`--mirror`, `--all`, `refs/*:refs/*`), `gc --prune=now --aggressive` tras `reset --hard` y `reflog expire`, trabajo de un agente | `crates/core/tests/tm_store_safety.rs`: `snapshots_are_never_pushed`, `aggressive_maintenance_…`, `agent_work_…` |
| Estado observable intacto con la huella del testkit (INF-GRP-001), con fsmonitor y untracked cache activados, siembra, previo de 2 worktrees y observaciones incrementales | `tm_store_safety.rs::repo_intact::capture_leaves_the_repo_intact` |
| Ida y vuelta: CRLF con `autocrlf` y `eol`, `ident`, filtro `clean` canario (no se ejecuta), ejecutable, enlace, Unicode, binario | `tm_store_capture.rs::content_round_trips_bit_for_bit` |
| Ignorados y excluidos (credenciales, repo anidado, submódulo, archivo grande) | `ignored_credentials_…`, `observation_leaves_out_large_files_…` |
| Seguridad: permisos 0700/0600, almacén abierto a otros o enlazado (apartado), clave que escapa, `.idx` falsificado, pack enlazado fuera, checksum ≠ nombre, `alternates`, xattrs y ACL del clon, objeto corrupto, exclusión de copias de seguridad (`tmutil isexcluded`), "disco lleno" simulado con escritura denegada | `tm_store_safety.rs` y `capture_failure_leaves_no_snapshot_…` |
| Siembra sin efecto en el repo: inodo, `mtime`, tamaño y modo de los packs del usuario tras sembrar y escribir objetos que ya están en ellos | `seeding_and_writing_known_objects_…` |
| Continuidad rota: hueco, `continuous=false`, `info/exclude`, `.gitignore`, reinicio, archivo *racy* y entrada del índice *racy*, caché UNTR desactualizada | `broken_continuity_…`, `racy_…`, `a_racy_index_entry_…`, `stale_untracked_cache_…` |
| Validez: fila sin ref, ref sin fila y `pending` con ref (la recuperación la descarta y borra su ref; la ref desconocida se conserva) | `only_snapshots_with_ref_and_complete_row_are_offered` |
| Incremental: un archivo cambiado, una ruta leída y el resto reutilizado | `incremental_capture_…` |
| Camino rápido, borrados y renombrados, varios worktrees, entradas inválidas | `fast_path_…`, `deleted_and_renamed_…`, `every_worktree_…`, `bad_requests_…` |
| Prioridad del previo (≤ 20 ms en release, 250 ms en debug; el banco mide el p95 de la espera) | `an_observation_gives_way_to_a_guaranteed_prior` |
| Frontera del escritor gix | `crates/git/tests/static_check.rs`, `crates/core/tests/tm_boundary.rs` |
| p95 frente a 200 ms con el perfil `M` | `crates/core/benches/tm_snapshot.rs` |
