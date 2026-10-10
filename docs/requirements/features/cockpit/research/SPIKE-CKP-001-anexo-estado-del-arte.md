---
id: SPIKE-CKP-001-ANEXO
title: "Anexo de SPIKE-CKP-001: estado de gix-merge y git merge-tree (2026-10-09)"
type: research
status: done
feature: cockpit
domain: GRP
spike: SPIKE-CKP-001
created: 2026-10-09
updated: 2026-10-09
related:
  adrs: [ADR-CKP-001, ADR-GRP-001, ADR-GRP-009]
  stories: [TS-CKP-001]
  spikes: [SPIKE-CKP-001]
tags: [cockpit, spike, investigacion, gitoxide, gix-merge, merge-tree, estado-del-arte]
---

> Investigación de fuentes (código de los tags, release notes y crates.io) hecha el 2026-10-09 por un subagente `nassa-core:internet-researcher` para el [plan de SPIKE-CKP-001](./SPIKE-CKP-001-plan.md) (§ 1). No contiene mediciones: lo que afirma sobre el comportamiento en ejecución lo confirma o refuta el SPIKE.

# Anexo — Investigación: gix-merge vs `git merge-tree` para predecir conflictos sin escribir (2026-10-09)

## Método

- Versiones: API de crates.io (`/api/v1/crates/<c>/versions`), consultada el 2026-10-09.
- Código gitoxide: clon de `GitoxideLabs/gitoxide`, checkout del tag anotado `gix-v0.89.0` (commit `0115e902`, 2026-10-08). `gix/` y `gix-merge/` no cambian entre ese tag y `main` (`34f654e5`, 2026-10-09).
- Código Git: clon de `git/git` `master` (`6de20f60`, 2026-10-06). Las RelNotes son las del árbol. El tag `v2.56.0` se firmó el 2026-09-28T04:20:13Z (API de GitHub).
- context7: `/gitoxidelabs/gitoxide` existe pero no tiene versiones. Por eso se usó el código fuente del tag como fuente primaria.

Base de las URLs: `GX = https://github.com/GitoxideLabs/gitoxide/blob/gix-v0.89.0`, `GT = https://github.com/git/git/blob/master`.

---

## A) gitoxide

### A1. Versiones

| Crate | Última | Fecha | Anterior |
|---|---|---|---|
| gix | **0.89.0** | 2026-10-08 | 0.88.0 (2026-09-25) |
| gix-merge | **0.22.0** | 2026-10-08 | 0.21.0 (2026-09-25) |
| gix-path | 0.14.0 | 2026-10-08 | 0.13.0 (2026-09-25) |
| gix-odb | 0.86.0 | 2026-10-08 | 0.85.0 (2026-09-25) |

**Corrección al ADR:** 0.88.0/0.21.0 (2026-09-25) era correcto en su fecha, pero ya no es lo último. Lo último es gix 0.89.0 / gix-merge 0.22.0.

**Cambio importante de 0.22.0:** el CHANGELOG lo deja en "Unreleased", pero el código ya está en el tag (`remove_if_leaf` aparece 25 veces en `resolve.rs`). Corrige una **pérdida silenciosa de datos** en las fusiones de árboles. Si un lado borraba o renombraba el archivo `a/b` y llenaba el directorio nuevo `a/b/` solo mediante renames, el merge informaba éxito y perdía todo `a/b/`. Afectaba también a las bases virtuales. Fuente: `GX/gix-merge/CHANGELOG.md` y el commit `18f476cd` (2026-09-27). **Hay que fijar `gix >= 0.89.0`.**

### A2. API (`gix/src/repository/merge.rs` @0.89.0)

- `merge_resource_cache(worktree_roots) -> gix_merge::blob::Platform`
- `blob_merge_options()`
- `tree_merge_options() -> merge::tree::Options`. Lee `merge.renames` y `merge.renameLimit`, o usa `diff.*`. Por defecto deja `fail_on_conflict: None`, `marker_size_multiplier: 0`, `symlink_conflicts: None` y `tree_conflicts: None`.
- `merge_trees(ancestor, ours, theirs, labels, options) -> merge::tree::Outcome`
- `merge_commits(ours, theirs, labels, merge::commit::Options)` calcula la merge-base. Con varias bases aplica el merge recursivo de las bases. Devuelve `merge_base_tree_id`, `merge_bases` y `virtual_merge_bases`.
- `virtual_merge_base(...)` y `virtual_merge_base_with_graph(...)`. Ojo: tienen un `// TODO: test` en el código.
- La doc de `merge_trees` dice: "No change to the worktree or index is made, but objects may be written to the object database … consider enabling object memory". `Repository::with_object_memory()` (`GX/gix/src/repository/cache.rs:48`) guarda los objetos en memoria.
- `write_buf` hace hash y `exists()`, y si el objeto ya existe no escribe nada (`GX/gix/src/repository/impls.rs:118`). **No refresca el mtime.**
- `gix_merge::tree::Outcome` (`GX/gix-merge/src/tree/mod.rs`): campos `tree: Editor` (sin escribir), `conflicts: Vec<Conflict>` y `failed_on_first_unresolved_conflict: bool`. Métodos `has_unresolved_conflicts(TreatAsUnresolved)` e `index_changed_after_applying_conflicts`.
- `TreatAsUnresolved { content_merge: Markers|ForcedResolution, tree_merge: Undecidable|EvasiveRenames|ForcedResolution }`. Presets: `git()` (por defecto), `undecidable()` y `forced_resolution()`.
- `Conflict` tiene `resolution: Result<Resolution, ResolutionFailure>`, `ours`, `theirs`, `entries()` (3 etapas, con `mode` e `id`), `is_unresolved(how)` y `content_merge() -> Option<ContentMerge{merged_blob_id, resolution}>`.
- `Resolution` tiene estas variantes:
  - `SourceLocationAffectedByRename`
  - `OursModifiedTheirsRenamedAndChangedThenRename`
  - `OursModifiedTheirsModifiedThenBlobContentMerge`
  - `Forced(ResolutionFailure)`
- `ResolutionFailure` tiene estas variantes:
  - `SubmoduleMerge`, `SubmoduleAddAdd`
  - `OursRenamedTheirsRenamedToSameLocation`, `OursRenamedTheirsRenamedDifferently`
  - `OursModifiedTheirsDirectoryThenOursRenamed`, `OursDirectoryTheirsNonDirectoryTheirsRenamed`
  - `OursAddedTheirsAddedTypeMismatch`, `OursModifiedTheirsRenamedTypeMismatch`
  - `OursDeletedTheirsRenamed`, `OursModifiedTheirsDeleted`
  - `Unknown`
- **Hunks y rangos de líneas:** no hay API pública que los dé. El blob fusionado con marcadores se escribe con el callback `write_blob_to_odb`, que en `merge_trees` es `self.write_buf` (en memoria si se usa `with_object_memory`), y se accede vía `ContentMerge::merged_blob_id`. Para sacar rangos hay que parsear los marcadores. `text::Merge` tiene `hunks` como campo privado (`GX/gix-merge/src/blob/builtin_driver/text/mod.rs:135`). `ConflictStyle` admite Merge, Diff3 y ZealousDiff3. Los marcadores miden el tamaño base + `marker_size_multiplier*2`.

### A3. Opciones, drivers y atributos

- `gix_merge::tree::Options`: `rewrites: Option<Rewrites>` (`limit` por defecto 1000, `percentage` 0.5), `blob_merge`, `blob_merge_command_ctx: gix_command::Context`, `fail_on_conflict` (para en el primer conflicto no resuelto), `marker_size_multiplier: u8`, `symlink_conflicts` y `tree_conflicts: Option<ResolveWith{Ancestor, Ours}>`.
- `merge_resource_cache` lee los drivers de config con `config.merge_drivers()` (secciones `[merge "<x>"]`). Usa `merge.default`. Arma un `gix_filter::Pipeline` con `command_context()` (los filtros `filter.<x>` pueden lanzar procesos). Usa la pila de atributos `attributes_only(&index, Source::IdMapping)` si `worktree_roots` no está fijado, y `WorktreeThenIdMapping` si lo está. El índice sale de `index_or_load_from_head_or_empty()`, así que `.gitattributes` se lee del **índice o, si no hay índice, del árbol HEAD**, no del worktree, en la ruta por defecto de `merge_trees`. También cuentan los atributos globales, del sistema e `info/attributes` según `Permissions.attributes`. El tamaño máximo es `large_file_threshold_bytes = core.bigFileThreshold`: por encima, el blob se trata como binario o `TooLarge`.
- **Drivers vacíos:** `merge_trees` y `merge_commits` crean su propio `merge_resource_cache` y no lo dejan inyectar. Para tener cero drivers hay que llamar directamente a `gix_merge::tree(base, ours, theirs, labels, objects, write_blob, diff_state, diff_cache, &mut blob_merge, options)` (`GX/gix-merge/src/tree/function/resolve.rs:97`). El `blob_merge` se construye con `gix_merge::blob::Platform::new(filter, mode, attr_stack, Vec::new(), options)` (`GX/gix-merge/src/blob/platform/mod.rs:81`).

### A4. Interrupción, límites, replace, grafts y lazy fetch

- **No hay `should_interrupt`.** Ninguna aparición de `interrupt` ni `AtomicBool` en `gix-merge/src`. Las únicas salidas tempranas son `fail_on_conflict` y `rewrites.limit`.
- Límite de memoria: `gitoxide.objects.allocLimit` o `GIT_ALLOC_LIMIT`. En repos de confianza reducida se aplica por defecto `allocLimitIfReducedTrust` (`GX/gix/src/open/repository.rs`). El tope por blob es `core.bigFileThreshold`.
- Replace refs: se controlan con `core.useReplaceRefs`; `GIT_NO_REPLACE_OBJECTS` se mapea a esa misma clave (`GX/gix/src/config/tree/sections/core.rs:98`), y `gitoxide.objects.replaceRefBase` fija la base. **Observación (sin probar en ejecución):** `replacement_objects_refs_prefix` evalúa `is_disabled = core.useReplaceRefs.unwrap_or(true)` (`open/repository.rs:608`). Leído así, gix **no** aplica `refs/replace` por defecto, y `useReplaceRefs=true` también los desactiva. Parece una inversión de la semántica.
- `info/grafts`: no aparece en `gix/` ni en `gix-odb/`. Lo único es el manejo de shallow en `revision/walk.rs`. No hay soporte.
- Lazy fetch: `gix-odb` no tiene código de promisor. `promisor` solo sale en la doc de `extensions.partialClone` (`repository/mod.rs:29`). gix **no** descarga objetos ausentes: devuelve error de objeto no encontrado.

### A5. Paridad con merge-ort y madurez

- `GX/gix-merge/tests/fixtures/tree-baseline.sh` genera baselines con `git merge-tree -z --write-tree` (unos 90 casos `baseline`, en los dos sentidos y con varios conflict styles). Las desviaciones están declaradas: `rename-within-rename*-deviates` (gix compone renames de directorio anidados y Git no).
- `GX/gix-merge/tests/merge/tree/cartesian-baseline.txt` da 210/210 en árbol exacto, conflicto no resuelto y payload frente a `git merge-tree`. El modelo es acotado: 1 base, renames exactos, sin atributos, symlinks, submódulos ni bases recursivas.
- `GX/crate-status.md`: sin soporte para submódulos (cuentan como conflicto), opciones de newline/whitespace (`-Xignore-space-change`), sparse index ("refuse") ni `merge=binary` sin cargar en memoria. Reconoce: "rewrite so that the whole logic can be proven to be correct - it's too insane now".
- Bases múltiples: `[x] commits - with handling of multiple merge bases by recursive merge-base merge`.
- El README coloca `gix-merge` en "Initial Development → usable". La API es pre-1.0 y cambia en cada minor.
- Issues abiertos con "merge" en el título: solo #3036 (`is_ancestor()`, 2026-10-02), según `gh api search/issues`.

### A6. Procesos hijo (`git`)

- `Permissions.config.git_binary` y `Permissions.attributes.git_binary` vienen en `false` por defecto, también en `all()` (`GX/gix/src/open/permissions.rs`). Con `true` se ejecuta `git config -lz --show-origin --no-includes` (`GX/gix-path/src/env/git/mod.rs:202`). `Options::git_installation_config_path(path)` evita esa ejecución.
- **Windows:** `gix_path::env::system_config()` comparte la invocación de `git` (`GX/gix-path/src/env/mod.rs:51`), y `system_prefix()` puede lanzar `git --exec-path`. Para garantizar cero procesos: `Permissions.config.system=false` y `attributes.system=false` (o `Permissions::isolated()`).
- Otros caminos que lanzan procesos: merge drivers (`blob_merge_command_ctx`), filtros `filter.<x>.clean/process` del `gix_filter::Pipeline` (se usan con `Mode::ToGit` / renormalize) y `core.sshCommand` (no aplica).

---

## B) `git merge-tree`

### B1. Historia (RelNotes)

| Función | Versión | Fuente |
|---|---|---|
| `--write-tree` (modo que fusiona dos commits) | 2.38.0 | `GT/Documentation/RelNotes/2.38.0.adoc:16` |
| `--stdin` | **2.39.0** (no 2.42) | `2.39.0.adoc:23` |
| `--merge-base` | 2.40.0 | `2.40.0.adoc:6` |
| Lee config básica (forges desactivan replace refs) | 2.41.0 | `2.41.0.adoc:368` |
| `-X` / `--strategy-option` | **2.43.0** (no 2.45) | `2.43.0.adoc:98` |
| `--merge-base` con trees (no solo commits) | 2.45.0 | `2.45.0.adoc:31` |
| Arreglo de deadlock en `--stdin` | 2.49.0 | `2.49.0.adoc:246` |
| `--quiet` ("resolves cleanly without actually creating a result") | **2.50.0** (no 2.46) | `2.50.0.adoc:82` |

`-z`, `--name-only` y `--messages` están documentados en `GT/Documentation/git-merge-tree.adoc`. Su introducción en 2.38 no está verificada en las RelNotes.

### B2. ¿Escribe objetos?

- `--write-tree` escribe blobs fusionados (`odb_write_object`, `merge-ort.c:2275`) y árboles (`write_tree`, `:3874`, `:4591`).
- `--quiet` activa `mergeability_only` (`builtin/merge-tree.c:602`). Solo graba árboles y blobs si `!mergeability_only || call_depth` (`merge-ort.c:4039`, `4342`, `4509`). **Con bases múltiples (`call_depth>0`) sigue escribiendo** los objetos de la base virtual. La doc dice "avoid writing **most** objects". `--quiet` es incompatible con `--messages`, `--name-only`, `--stdin` y `-z`.
- **Freshen:** `odb_write_object` hace primero `odb_freshen_object`, que recorre **todas** las `odb->sources`, alternates incluidos, y toca el mtime si el objeto ya existe (`GT/odb.c:815` y `:987`).

### B3. Atributos, drivers, replace y lazy fetch

- Ejecuta drivers externos: `ll_merge` → `ll_ext_merge` → `run_command` (`GT/merge-ll.c:205,257`).
- Atributos: en un repo no bare, dirección CHECKIN: primero el `.gitattributes` del **worktree** y después el índice (`attr.c:851`). En un repo bare no se leen archivos del árbol salvo con `--attr-source`, `GIT_ATTR_SOURCE` o `attr.tree` (2.43, `2.43.0.adoc:104`). El uso de HEAD por defecto en bare se revirtió en 2.46 (`2.46.0.adoc:279`, `2.45.3.adoc:31`).
- Replace: `--no-replace-objects` / `GIT_NO_REPLACE_OBJECTS` (`git.c`) y `core.useReplaceRefs` (merge-tree lee config desde 2.41).
- Lazy fetch: `prefetch_for_content_merges` → `promisor_remote_get_direct` (`merge-ort.c:4450,4494`). Se corta con `--no-lazy-fetch` / `GIT_NO_LAZY_FETCH=1` (2.45.0, `2.45.0.adoc:120`), que se comprueba en `promisor-remote.c:34`.

### B4. Versión estable

**Git 2.56.0**, tag del 2026-09-28. No hay 2.56.x ni 2.57 en `ls-remote`. En `master` existe `Documentation/RelNotes/2.98.0.adoc` (borrador del ciclo siguiente: "antepenultimate release to prepare for … Git 3.0"). No está publicado.

---

## No verificado

- El efecto real de `core.useReplaceRefs` en gix (solo lectura de código; hace falta un test).
- La versión exacta de `-z`, `--name-only` y `--messages`.
- `protocol.allow=never` como corte del lazy fetch (no se buscó).
- Si `--quiet` en 2.56.0 es idéntico a `master` (se leyó el código de `master`; la RelNote es la de 2.50).
- Si gix-merge implementa la **detección de renames de directorio por inferencia** de merge-ort (archivo nuevo dentro de un directorio renombrado por el otro lado) en todos los casos. El código y los baselines la tratan, pero con diferencias de índice documentadas ("directory-old … Git reports a file location conflict").
- La lista completa de issues abiertos de gix-merge (la búsqueda fue solo por título).
