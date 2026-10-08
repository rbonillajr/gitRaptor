---
id: DS-TS-TMC-003
title: "Dev Spec — Capa de escritura acotada y aplicador de estados"
type: dev-spec
status: review
feature: time-machine
domain: GRP
story: TS-TMC-003
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-TMC-002, ADR-TMC-001, ADR-TMC-003, ADR-GRP-002, ADR-GRP-009]
  nfrs: [NFR-01, NFR-02, NFR-07, NFR-TMC-07, NFR-TMC-12, SEC-TMC-02, SEC-TMC-04, SEC-TMC-05, SEC-TMC-09, SEC-TMC-11, SEC-TMC-14]
tags: [time-machine, escritura, aplicador, restauracion, locks, intercambio-atomico, rutas-seguras, crates-git, crates-core]
---

# Dev Spec — TS-TMC-003: capa de escritura acotada y aplicador de estados

Blueprint compacto de [TS-TMC-003](../technical-stories/TS-TMC-003-escritura-aplicador.md). Fuentes: ADR-TMC-002 (frontera, reglas y orden de aplicación, con sus enmiendas), ADR-GRP-009 § 3 (invocación que se hereda), SEC-TMC-02/04/05/09/11/14 y el almacén y el oplog ya en main (TS-TMC-001 y TS-TMC-002). Implementada en esta misma rama.

**Estado `review`**: la TS queda **implementada en parte**, verificada solo en macOS. Lo que falta está en § 8 con su historia de destino.

Decisiones del orquestador (2026-10-04), validadas por el Arquitecto (aprobó con ajustes, todos incorporados): D1–D9 de las secciones siguientes. El alcance y los pendientes los validó el PO, que pidió tres ajustes ya incorporados: `repack` y `prune` en la lista cerrada (§ 2), un test de "no restaurable con garantía" (§ 5) y no perder nunca entradas del stash (§ 4).

## 1. Ubicación en el código

Toda escritura sobre el repo del usuario vive en `crates/git/src/tm_write/` (ADR-GRP-002, ADR-TMC-002 § 1). `crates/core` solo orquesta.

| Archivo | Responsabilidad |
|---|---|
| `crates/git/src/invoke.rs` | Sigue siendo el **único módulo que lanza procesos** (ADR-GRP-009, Validación 5). Añade el **perfil de escritura**: `WriteSubcommand` (lista cerrada con sus opciones fijas), `WRITE_OPTIONS`, `WRITE_ENV`, `WriteTarget` y `run_write`, con stdin en bytes o desde archivo y stdout a archivo |
| `crates/git/src/tm_write/mod.rs` | `WriteContext` (Git resuelto una vez por el daemon, invoker, carpeta de hooks vacía, configuración global vacía y carpeta temporal del perfil, todas privadas) y `WriteError` |
| `.../tm_write/cli.rs` | Invocaciones tipadas: `update_ref`, `index_info`, `skip_worktree`, `pack_objects`, `index_pack`. Es el único llamador de `run_write` |
| `.../tm_write/worktree.rs` | `WriteWorktree`: raíz canónica del estado del daemon y carpetas de Git halladas leyendo `.git` sin seguir enlaces (nunca `core.worktree`). Precondiciones del paso 1 y lectura de `HEAD` |
| `.../tm_write/lock.rs` | `GitLock`: protocolo de lock de Git (`O_EXCL`, `O_NOFOLLOW`, identidad dev+inodo, commit por rename con `fsync`; solo se borra o confirma si sigue siendo nuestro) |
| `.../tm_write/refs.rs` | Transacción `update-ref --stdin -z` con valor anterior esperado; `swap_head` por compare-and-swap; validación de nombres (SEC-TMC-14) |
| `.../tm_write/files.rs` | `RootDir`: apertura relativa a la raíz, intercambio atómico, borrado comparado, carpetas vacías y sondeo de mayúsculas y NFC/NFD |
| `.../tm_write/tree_path.rs` | Corpus hostil y colisiones (SEC-TMC-04) |
| `.../tm_write/index.rs` | Índice destino en un temporal del perfil e instalación por el `index.lock` propio |
| `.../tm_write/objects.rs` | Objetos del almacén al repo: `pack-objects` → `index-pack --keep` |
| `.../tm_write/recreate.rs` | Recreación de un worktree enlazado sin checkout |
| `crates/core/src/timemachine/apply/` | `Applier`: pasos 1 y 3–8 con el diario; `plan.rs` carga y revalida los dos snapshots |
| `crates/core/src/timemachine/repo_lock.rs` | Un escritor de la Time Machine por repo en el daemon (sin esperas) |

**Dependencia nueva**: `unicode-normalization` 0.1.25 (MIT/Apache-2.0) en `crates/git`, ya presente en `Cargo.lock` por gix.

## 2. Perfil de escritura (SEC-TMC-02, SEC-TMC-14) — D1

| Variante | argv fijo | Dónde |
|---|---|---|
| `UpdateRef` | `update-ref --stdin -z` | carpeta común del repo |
| `IndexInfo` | `update-index -z --index-info` | índice temporal (`GIT_INDEX_FILE`) |
| `SkipWorktree` | `update-index --skip-worktree -z --stdin` | índice temporal |
| `PackObjects` | `pack-objects --revs --stdout --quiet` | almacén |
| `IndexPack` | `index-pack --stdin --strict --keep=gitraptor-tm` | carpeta común del repo |
| `Repack` | `repack -d -q --geometric=2` | almacén (ADR-TMC-007 § 4, E10) |
| `Prune` | `prune --expire=1.hour.ago` | almacén; periodo de gracia de ADR-TMC-007 § 4 (⚠️ ASSUMPTION: 1 hora) |

- Los datos (refs, revisiones, rutas) **solo viajan por stdin**: ninguna revisión ni ruta puede leerse como opción. `run_write` no acepta argumentos libres.
- Antes de cada subcomando: `--no-optional-locks`, `-c core.hooksPath=<perfil>/nohooks`, `-c safe.directory=<ruta validada>`, `--git-dir=…` y, si aplica, `--work-tree=…`, más `core.fsmonitor=false`, `core.untrackedCache=false`, `core.splitIndex=false`, `index.sparse=false`, `index.skipHash=false`, `gc.auto=0`, `maintenance.auto=false`, `gpg.program=`, `commit.gpgSign=false`, `protocol.allow=never`, `credential.helper=`, `core.sshCommand=`, `core.askPass=`, pager y trace2 neutralizados.
- Entorno: la allowlist de lectura sin `HOME`, más `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=<perfil>/empty.gitconfig`, `GIT_NO_REPLACE_OBJECTS=1`, `GIT_ALLOW_PROTOCOL=` y `GIT_PROTOCOL_FROM_USER=0`. `GIT_INDEX_FILE` es la única variable variable, tipada y siempre en el perfil.
- **`safe.directory`**: al neutralizar la configuración global se perdería la del usuario. El aplicador decide antes la confianza con `RepoReader` (que sí la respeta) y la capa pasa `-c safe.directory=<esa ruta exacta>` (ajuste del Arquitecto).
- Binario: el `SystemGit` que el daemon ya resolvió (Enmienda E6 de ADR-TMC-002).
- `repack` y `prune` se exponen como `StoreRepo::repack` y `StoreRepo::prune` (ajuste del PO: ADR-TMC-002 § 2 ya los incluye). Cuándo corren y con qué gracia lo decide US-TMC-016.

## 3. Orden de aplicación (ADR-TMC-002 § 3) — D8, D9

El aplicador empieza donde termina la operación protegida (TS-TMC-004, en paralelo): la operación está `ready` y su previo garantizado (paso 2) está completo. **Ese previo es el estado esperado del repo**; toda escritura se compara con él.

| Paso | Qué hace | Si falla |
|---|---|---|
| 1 | `check_preconditions` (público, para que la operación protegida lo llame antes del previo): sin operación en curso (`rebase-*`, `MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `BISECT_LOG`, `sequencer`), sin locks de Git (`index.lock`, `HEAD.lock`, `packed-refs.lock`, `config.lock`, `shallow.lock`, `refs/**/*.lock`), repo de confianza, ambos snapshots re-verificados y `meta` revalidado (SEC-TMC-09), y refs y `HEAD` donde dice el previo | `rejected`, sin cambios |
| 3 | Lock del repo en el daemon (`repo_lock`, sin esperas) e `index.lock` de cada worktree, anotados (`lock-taken` con inodo y nacimiento). Bajo los locks se repiten las precondiciones y se valida el árbol destino contra el FS sondeado. `applying{3}` se anota **después** de tomarlos | `rejected` (nueva transición `ready → rejected`), locks liberados y anotados |
| 4 | Objetos que el repo no tiene (commits destino y blobs del índice), con exclusión de lo alcanzable desde las refs del previo. El pack queda con `.keep` hasta el paso 5 | `interrupted` (sin cambios visibles) |
| 5 | Una transacción de ramas y `refs/stash` con valor anterior; se libera el `.keep`; después, cada `HEAD` por compare-and-swap | `interrupted`; si falla la transacción, sin cambios |
| 6 | Archivos: borrados del más profundo al menos, carpetas vacías, escrituras del menos profundo al más. Un worktree a recrear se recrea aquí y toma su `index.lock` | `interrupted` |
| 7 | Índice: construido en un temporal e instalado por el `index.lock` propio (comprobando que sigue siendo nuestro) | `interrupted` |
| 8 | Se libera el repo y la operación queda `finished` con su informe | — |

Sin rollback automático (TQ-10 → a): un fallo a mitad deja la operación `interrupted` y `raptor undo` vuelve al previo. Los locks se liberan y anotan también al interrumpir. `ApplyHooks` expone puntos de inyección (`at_step`, `before_exchange`) para los tests y para el arnés de caos de INF-TMC-001.

El lock por repo se indexa por la ruta del almacén (única por repo y perfil) y vive en `timemachine` para que la captura, la purga y el mantenimiento lo compartan (pendiente, § 8).

## 4. Refs y HEAD — D2

- Solo `refs/heads/*` y `refs/stash` son representables; se rechazan nombres que no pasan `check-ref-format`, con un componente que empieza por `-` o con caracteres de control.
- `update`/`create`/`delete` con el valor anterior del previo (`0{40}` si debe no existir) y `verify` para lo que no cambia; `prepare` + `commit`. Si un agente movió una ref, la transacción falla entera.
- **`HEAD` va fuera de la transacción** (enmienda de ADR-TMC-002 § 3, ver allí): `symref-update` exige Git 2.46 (> 2.38); con `HEAD.lock` tomado, `update-ref` no puede mover la rama a la que apunta `HEAD` (comprobado); y `update-ref` rechaza `worktrees/<id>/HEAD` (comprobado con Git 2.50.1), así que el `HEAD` de un worktree enlazado tampoco cabe. Orden: se verifican todos los `HEAD`, corre la transacción y luego cada `HEAD` se escribe con `HEAD.lock` exclusivo, comparación bajo el lock, `fsync` y rename. No deja entrada de reflog: aviso `HeadWithoutReflog`.
- `refs/stash` mueve solo la cima y `update-ref` añade una entrada a su reflog: las entradas anteriores (`stash@{1..n}`) siguen recuperables (test). Aviso `StashTopOnly`.
- **`refs/stash` nunca se borra** (ajuste del PO, NFR-01): borrar la ref borraría su reflog, es decir, toda la pila. Si el destino no tiene stash y el repo sí, se conserva con aviso `StashKept`. Si el destino tiene stash y el repo no, se crea la ref, pero sin reflog nuevo `git stash list` no la muestra (pendiente menor, § 8).

## 5. Archivos (SEC-TMC-04, SEC-TMC-11) — D3, D4, D5

- **Rutas**: fd de la raíz y `openat(O_DIRECTORY|O_NOFOLLOW)` componente a componente, con `st_dev` igual al de la raíz (equivale a `NO_XDEV`). Un enlace, un archivo o otro dispositivo en el camino da `Blocked` y no se escribe nada.
- **Árbol hostil** (`tree_path`): se rechaza entero si hay ruta vacía, absoluta, `.`, `..`, NUL o `.git` en cualquier grafía (mayúsculas, `GIT~1`, puntos y espacios finales, sufijo de stream `:…`, ignorables de HFS+). En Unix `\` y `:` son legales y solo cuentan si forman `.git`. Las **colisiones** de mayúsculas o NFC/NFD solo se rechazan si el FS destino las pliega: se **sondea** en la raíz con un temporal (ajuste del Arquitecto; `README` y `readme` son válidos en ext4).
- **Reemplazo**: el contenido nuevo va a `.gitraptor-tm-<n>` (`O_EXCL`, modo 0666/0777 según la umask, `fsync`) y se intercambia con `renameat_with(EXCHANGE)` (`RENAME_EXCHANGE` en Linux, `RENAME_SWAP` en macOS). Lo desplazado se compara (tipo, incluido el bit ejecutable, y hash de blob) con el previo: si coincide, ya está en el almacén y se borra; si no, se deshace el intercambio y la ruta es `Overlap` con el contenido ajeno en su sitio. Si el previo dice "ausente" se usa `NOREPLACE`/`RENAME_EXCL`: algo presente es solape y se queda. Si la ruta ya tiene el destino, `Unchanged`.
- **Borrado**: rename a temporal, comparación y `unlink`; si difiere, vuelve a su ruta con `NOREPLACE` (si alguien la ocupó, el temporal se conserva y se reporta en `kept_at`). Las carpetas solo se borran vacías (`ENOTEMPTY` → se quedan, pueden tener ignorados).
- **Solo el diff entre el previo y el destino**: nunca se recorre el FS, así que los ignorados (que nunca están en un snapshot) no se tocan. Las exclusiones del `meta` (`<clave>:<ruta>`: anidados, submódulos, > 50 MB) nunca se escriben ni se borran y salen como aviso `Excluded`.
- **FS sin intercambio** (`EINVAL`, `ENOTSUP`, `ENOSYS`): la ruta queda como está y se reporta `NotGuaranteed` ("no restaurable con garantía"). Se prueba con `ApplyHooks::simulate_no_exchange`, que hace fallar con `EINVAL` todo rename con intercambio o exclusivo por el mismo camino de código que un FS real sin ellos (ajuste del PO). Refs e índice sí se aplican; el informe lista cada ruta no restaurada. En Windows el aplicador devuelve `Unsupported` (el almacén ya es solo Unix).
- **Temporales en el worktree** (`.gitraptor-tm-*`): existen solo durante la escritura de una ruta. Mientras dura la aplicación el `index.lock` está tomado, así que un `git add -A` de un agente falla y no puede commitearlos. **Riesgo anotado**: tras un crash, un temporal puede quedar como archivo sin seguimiento hasta que la recuperación lo limpie (pendiente, § 8); su contenido está en el almacén.

## 6. Índice, objetos y worktrees — D6, D7

- **Índice**: entradas de etapa 0 de `wt/<clave>/index` más las etapas 1–3 de los conflictos del `meta`, por `update-index -z --index-info` sobre `GIT_INDEX_FILE=<perfil>/tmp/index-<n>`, y `--skip-worktree` para las rutas marcadas. Los bytes se escriben en el `index.lock` tomado en el paso 3 y se renombran. Un índice sin entradas se instala borrando `index` (Git lo lee como vacío). Sin stat: el primer `git status` vuelve a comparar contenido. `intent-to-add` no tiene plumbing en 2.38: se omite con aviso `IntentToAddNotRestored` (el archivo sigue en el working tree).
- **Objetos**: `pack-objects --revs --stdout` en el almacén con `want` y `^have` por stdin, a un temporal privado; `index-pack --stdin --strict --keep` en el repo. El `.keep` impide que un `gc` de un agente tire el pack antes de que las refs lo alcancen.
- **Recreación de un worktree enlazado**: sin `git worktree add` (porcelana). Solo si la ruta es absoluta, no existe o está vacía, su padre existe y queda fuera del perfil, de la carpeta de Git y de todo `.git`; id `[A-Za-z0-9._-]` sin `.` inicial; rechazada con `extensions.relativeWorktrees`. `worktrees/<id>/` se crea en exclusiva (nunca se pisa uno existente) con `HEAD`, `commondir` y `gitdir`; el `.git` del worktree se escribe **al final** como punto de confirmación. Si el proceso muere antes, queda una carpeta de administración que `git worktree prune` limpia.

## 7. Frontera (comprobaciones estáticas)

| Test | Qué falla |
|---|---|
| `crates/git/tests/static_check.rs::repo_intact::process_spawn_only_in_invoke_module` (existente) | Un lanzamiento de proceso fuera de `invoke.rs` |
| `…::repo_intact_tm::gix_writes_only_in_store_writer` (existente) | Una escritura de gix fuera de `tm_write/store/` |
| `…::repo_intact_tm_write::write_profile_is_a_closed_plumbing_list` | Una variante fuera de `update-ref`, `update-index`, `pack-objects`, `index-pack`, o cualquier porcelana o comando de remoto (`push`, `fetch`, `clone`, `checkout`, `commit`, `worktree`…) |
| `…::write_profile_neutralizes_configurable_code` | Que falte `GIT_CONFIG_NOSYSTEM`, `GIT_CONFIG_GLOBAL`, `core.hooksPath`, `gpg.program`, `protocol.allow=never`, `--git-dir`, `--work-tree` o `core.fsmonitor=false` |
| `…::only_the_write_layer_runs_the_write_profile` | `run_write` fuera de `tm_write/cli.rs`, o `tm_write` usando la CLI de lectura |
| `…::file_system_writes_on_repos_only_in_the_write_layer` | `renameat`, `unlinkat`, `mkdirat`, `symlinkat` o `RenameFlags` fuera de `tm_write/` |
| `crates/core/tests/tm_boundary.rs::repo_intact::only_the_time_machine_uses_the_write_layer` (existente) | El motor, el daemon, las apps o `api`/`policy` importando `tm_write` |
| `…::repo_intact_fs::core_does_not_write_repos_with_raw_calls` | Llamadas de escritura directas en `crates/core`. **Excepción conocida**: la liberación de locks anotados de la recuperación (`oplog/recovery.rs`, TS-TMC-002), pendiente de moverse a `tm_write::lock` |

## 8. Fuera de este corte (pendiente)

| Pendiente | Dónde se resuelve |
|---|---|
| Muerte del daemon en cada paso 3–7 y limpieza de temporales `.gitraptor-tm-*` tras un crash (su contenido está en el almacén: sin pérdida) | INF-TMC-001 (usa `ApplyHooks::at_step`) y la recuperación de TS-TMC-002 |
| Llamar al aplicador desde la operación protegida, y compartir `repo_lock` con la captura, la purga y el mantenimiento | TS-TMC-004 y US-TMC-016 |
| Cuándo corren `repack` y `prune` (ya en la lista cerrada) y la prueba "captura en curso durante el mantenimiento" | US-TMC-016 |
| Mover la liberación de locks de la recuperación a `tm_write::lock` | Deuda de TS-TMC-002 |
| `intent-to-add` sin restaurar | Cuando el mínimo de Git lo permita; mientras, aviso tipado |
| "No restaurable con garantía" sobre un FS real sin intercambio (aquí se prueba por simulación de `EINVAL`) | INF-TMC-001 (p. ej. exFAT o NFS) |
| Stash creado por la restauración en un repo sin stash: sin reflog, `git stash list` no lo muestra | Menor; decidir con `--create-reflog` cuando se cierre la Dev Spec |
| Textos en/es de los avisos y motivos (aquí son códigos tipados, NFR-TMC-14) | Cliente (CLI/TUI) |
| Windows (`ReplaceFileW` con respaldo, `FILE_FLAG_OPEN_REPARSE_POINT`) y verificación manual con un editor que mantiene un archivo abierto | Hecho en la Enmienda 2026-10-08 (XP-12) |
| Linux (`renameat2`): compila, sin ejecutar aquí | **Pendiente: etapa de validación multiplataforma** |
| Aviso "ya empujado" (ADR-TMC-002 § 4) | US-TMC-014 |

## 9. Verificación

| Criterio de la TS | Test |
|---|---|
| Frontera: la comprobación estática falla si el motor importa la capa o si se añade una operación de remoto | § 7 |
| Sin código configurable: canario de SEC-TMC-02 (filtros, `core.fsmonitor`, hooks `reference-transaction`/`post-checkout`/`post-index-change`, `gpg.program`, pager, credenciales, trace2) más `core.worktree` hostil: 0 marcadores y 0 escrituras fuera | `crates/core/tests/tm_apply.rs::repo_intact::internal_writes_run_no_configurable_program` |
| Concurrencia: rama movida durante la aplicación → la transacción falla entera, sin cambios | `a_branch_moved_while_applying_fails_the_whole_transaction` (y `a_branch_moved_before_applying_rejects_without_changes`) |
| Concurrencia: escritura entre la comparación y el intercambio → solape y contenido conservado | `a_write_between_compare_and_exchange_is_overlap_and_kept` |
| Rutas hostiles: corpus y nombres de ref, sin escrituras fuera ni opciones inyectadas | `tree_path::tests::*`, `refs::tests::*`, `a_link_planted_while_applying_never_leads_outside`, `hostile_names_in_the_meta_are_rejected_before_git_runs` |
| Exactitud: working tree (contenido, bit ejecutable, enlace, cambio de tipo archivo/carpeta), índice (`ls-files -s`) y refs iguales al snapshot | `restores_working_tree_index_and_refs_exactly` |
| Objetos perdidos por `gc` vuelven desde el almacén; `fsck` limpio y sin `.keep` | `objects_the_repo_lost_come_back_from_the_store` |
| Locks: con un lock ajeno, no empieza y el lock sigue ahí | `a_foreign_lock_rejects_and_stays`, `lock::tests::*` |
| Precondiciones de US-TMC-015 (operación en curso) | `an_operation_in_progress_rejects` |
| Exclusiones nunca escritas ni borradas | `excluded_paths_are_never_written_or_removed` |
| Recreación de worktree sin checkout | `a_deleted_worktree_is_recreated_without_checkout` |
| Un escritor por repo | `a_second_application_on_the_same_repo_is_busy`, `repo_lock::tests::one_holder_per_repo` |
| "No restaurable con garantía" en un FS sin intercambio | `without_atomic_exchange_paths_are_reported_and_left_alone` |
| El stash nunca pierde entradas | `restoring_the_stash_keeps_every_older_entry`, `a_stash_absent_from_the_target_is_never_deleted` |
| `repack` y `prune` del almacén por el perfil de escritura | `crates/git/tests/tm_write_maintenance.rs` |
| Caos | Pendiente: INF-TMC-001 |

Verificado en macOS (Apple Silicon, Git 2.50.1): `cargo clippy --all-targets -- -D warnings` y `cargo test --workspace` en verde; `cargo clippy --target x86_64-pc-windows-msvc -p gitraptor-git --all-targets` limpio (`crates/core` no compila para Windows desde este Mac por el C de sqlite: lo cubre el CI).

## Enmienda 2026-10-08 — Windows (XP-12)

Porta a Windows el almacén de TS-TMC-001 y las escrituras del aplicador de esta Dev Spec, que hasta hoy devolvían `Unsupported` ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md), XP-12). No cambia nada en Unix. El FFI nuevo vive solo en `crates/winsys` (ADR-GRP-002, Enmienda 2026-10-05), detrás de una API segura.

| Id | Decisión |
|---|---|
| W1 | **Reemplazo por dos renombrados, no `ReplaceFileW`.** Windows no tiene intercambio atómico. El contenido nuevo va a `.gitraptor-tm-<n>` (creación exclusiva y `FlushFileBuffers`). Lo actual se renombra a otro temporal con `MoveFileExW(MOVEFILE_WRITE_THROUGH)`, **sin** `MOVEFILE_REPLACE_EXISTING`. Lo desplazado se compara con el previo. Si coincide, el nuevo entra con el mismo renombrado exclusivo y lo desplazado se borra (ya está en el almacén). Si difiere, vuelve a su ruta: es `Overlap`. Si alguien ocupa la ruta en el hueco, su contenido se queda (`Overlap`) y el desplazado se borra solo si es el previo. Se descarta `ReplaceFileW` porque sigue enlaces en el archivo reemplazado, mezcla atributos y tiene un estado intermedio (`ERROR_UNABLE_TO_MOVE_REPLACEMENT_2`) con la ruta vacía. `MoveFileExW` renombra la entrada misma y nunca sigue un reparse point. **Coste aceptado**: durante unos microsegundos la ruta no existe. Si el proceso muere en ese hueco, el contenido queda en un `.gitraptor-tm-*` (o es el previo, que ya está en el almacén, o es contenido ajeno conservado). No hay pérdida (NFR-01). |
| W2 | **Carpetas del camino fijadas.** Cada componente se abre con `FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS` y sin `FILE_SHARE_DELETE`, y el handle se mantiene durante la escritura de la ruta. Mientras está abierto, nadie puede renombrar ni borrar esa carpeta, así que tampoco cambiarla por una junction. Es el equivalente de `openat(O_NOFOLLOW)`. Una carpeta con atributo de reparse point (enlace o junction) o en otro volumen (número de serie distinto al de la raíz) da `Blocked`. |
| W3 | **Reparse points nunca se siguen.** Lo que hay en la ruta se observa con `FILE_FLAG_OPEN_REPARSE_POINT`. Un reparse point (enlace, junction, placeholder de la nube) es "otro contenido": no se lee, no coincide con nada y, si estaba, se conserva (`Overlap`). La captura (`open_nofollow`) abre igual y rechaza un reparse point. |
| W4 | **Tipos.** NTFS no tiene bit de ejecución: `File` y `Executable` son iguales al comparar, como Git con `core.fileMode=false`. Escribir un enlace (`Content::Symlink`) da `NotGuaranteed` porque crearlo exige un privilegio y depende de `core.symlinks`. Un archivo normal cuyos bytes son el destino del enlace esperado cuenta como ese enlace (así lo deja Git for Windows con `core.symlinks=false`). |
| W5 | **Archivo abierto por otro proceso** (un editor sin `FILE_SHARE_DELETE`, o el antivirus). El renombrado falla con violación de compartición o acceso denegado y la ruta queda intacta. Se reintenta con un plazo acotado: 6 intentos con espera creciente, unos 1,5 s en total. Si sigue bloqueado, la escritura falla con `WriteError::Locked(ruta)` y la operación queda interrumpida. `raptor undo` vuelve al snapshot previo (ADR-TMC-002, "fallo detectado a mitad"). Nunca se fuerza ni se borra un archivo abierto. |
| W6 | **Almacén.** Las carpetas privadas se crean con la DACL protegida de `gitraptor-winsys::acl::create_private_dir` y se verifican con `verify_private_dir` (dueño, sin acceso para otros y sin reparse point, SEC-TMC-01). Los archivos heredan esa DACL. La comparación "dentro de `tm/`" usa una sola forma de ruta en los dos lados (`paths::canonicalize`, regla de la segunda ronda). `fsync` de carpeta no existe en Windows: NTFS registra los metadatos en su diario, y la barrera es `FlushFileBuffers` (`sync_all`), como dice ADR-TMC-001 § 4. |
| W7 | **Locks de Git** (`GitLock`): en Windows la identidad del lock es `(número de serie del volumen, índice del archivo)` del handle (`gitraptor-winsys::file_id`). Un lock reemplazado por otro nunca se confirma ni se borra. |
| W8 | El aplicador deja de rechazar Windows en `check_preconditions`. Siguen pendientes en Windows el suelo de espacio libre (`below_floor` devuelve "suficiente": si el disco se llena, la escritura del almacén falla como sin espacio) y la recuperación de locks del oplog (XP-14 y XP-15). |

**Verificación (máquina real `gitraptor-win`, Windows 10 19045, NTFS, Git 2.56)**: `tm_store_capture`, `tm_store_safety`, `tm_apply`, `tm_write_maintenance` y `protected::backend::tests::the_lock_key_is_the_appliers` dejan de ser `cfg(unix)`. Tests nuevos `cfg(windows)` en `files.rs`: archivo abierto sin `FILE_SHARE_DELETE` → `Locked` sin pérdida, y se escribe en cuanto se cierra; un enlace o junction en el camino → `Blocked`; un reparse point en la ruta → `Overlap` sin seguirlo. E2E en Windows: edición, `git reset --hard` y `raptor undo` restauran. Resultados en el PR y en `xplat-pendientes.md`.

> **Decisión del orquestador (2026-10-08), validada por el Arquitecto**: W1–W8.
