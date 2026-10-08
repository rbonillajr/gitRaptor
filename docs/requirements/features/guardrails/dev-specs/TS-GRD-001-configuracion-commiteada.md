---
id: DS-TS-GRD-001
title: "Dev Spec — Lectura commiteada de la configuración del equipo y de la rama principal"
type: dev-spec
status: implemented
feature: guardrails
domain: GRP
story: TS-GRD-001
created: 2026-10-04
updated: 2026-10-08
related:
  adrs: [ADR-GRP-007, ADR-GRD-004, ADR-GRD-003, ADR-GRP-009, ADR-GRP-006]
  nfrs: [NFR-01, NFR-03, NFR-GRD-04, NFR-GRD-15, SEC-11, SEC-GRD-13, SEC-GRD-17]
tags: [guardrails, configuracion, settings-json, suelo, rama-base, refs-reemplazo, schemars, crates-policy]
---

# Dev Spec — TS-GRD-001: lectura commiteada de la configuración del equipo

Blueprint compacto de [TS-GRD-001](../technical-stories/TS-GRD-001-configuracion-commiteada.md). Fuentes: ADR-GRP-007 (formato, niveles, validación, estado por fuente) y ADR-GRD-004 § 1 a § 4 (suelo, worktree, rama principal, rama base y suelo confirmados). Es el **cargador común**: lo reutilizan US-GRP-013 (perfil y local), US-GRP-016 (rama base del motor), US-GRD-007, US-GRD-011 y US-GRD-014.

## 1. Ubicación en el código

| Crate / archivo | Responsabilidad |
|---|---|
| `crates/git/src/committed.rs` | Lecturas de objetos commiteados: `committed_file(commit, ruta, máx)` → `Absent` / `NotRegular(Symlink\|Submodule\|WrongKind)` / `TooLarge` / `Blob{id, bytes}`; `blob_by_id`; `remote_names`; `symbolic_target` |
| `crates/git/src/reader.rs` | Al abrir, `objects.ignore_replacements = true` para **todas** las lecturas (SEC-GRD-17) |
| `crates/policy/src/settings/model.rs` | Tipos del documento (`Settings`, `engine`, `permissions`, `policies`, `timeMachine`) con `x-gitraptor-levels` por clave |
| `crates/policy/src/settings/schema.rs` | Schema generado con `schemars`, versionado en `crates/policy/schema/settings.schema.json` y embebido |
| `crates/policy/src/settings/strict.rs` | Parser JSON estricto con los límites de L-03 aplicados durante el parseo |
| `crates/policy/src/settings/document.rs` | `parse_document(bytes, nivel, fuente)` → `Parsed { status, settings, diagnostics }`. Entrada común a los tres niveles |
| `crates/policy/src/settings/diagnostic.rs` | Códigos de diagnóstico, puntero JSON saneado; nunca contenido |
| `crates/policy/src/team.rs` | `resolve_main_branch`, `TeamLoader::load`, `combine`, `affects_team_config` |
| `crates/core/src/profile/team_baseline.rs` | `RepoStore::{confirmed_team_baseline, set_confirmed_team_baseline}` sobre `store_meta` |

## 2. Dependencias nuevas

| Crate | Por qué | Licencia |
|---|---|---|
| `schemars` 1.2 | Schema desde los tipos (ADR-GRP-007) | MIT |
| `serde` / `serde_json` | Parseo y tipos | MIT / Apache-2.0 |

## 3. Documento y validación (ADR-GRP-007)

- **Límites (L-03)**, aplicados mientras se parsea, en bytes: 64 KiB (por la cabecera del blob, antes de cargarlo), profundidad 16, 256 claves o elementos, 1 KiB por cadena y por clave. Superar uno → `ignorado`.
- **`ignorado` (PQ-8)**: JSON inválido (con línea y columna), tipo o rango incorrecto (con puntero JSON), límites, entrada no regular (enlace, submódulo, árbol), objeto ilegible.
- **Clave desconocida** fuera de `permissions`/`policies`, o **clave fuera de nivel** (Q24): se quita esa clave, con diagnóstico; el resto aplica.
- **`parcial` (D12)**: clave desconocida u operación fuera del catálogo dentro de `permissions`, o cualquier clave de `policies` (`policy-not-supported`: "aún no se aplica", no "inválida").
- **Validador**: recorre el documento contra el schema embebido, con un **subconjunto cerrado** de palabras clave (`type`, `properties`, `items`, `enum`, `minimum`, `maximum`, `$ref`, `x-gitraptor-levels` y anotaciones). Un test falla si el schema generado usa otra. Los niveles admitidos salen solo del schema.
- **Diagnósticos**: código estable, fuente y línea/columna o puntero JSON. Nunca mensajes de `serde_json` (llevan valores). Los segmentos del puntero se truncan a 64 caracteres y se neutralizan Cc, Cf, Zl y Zp.

**Clave del mínimo** (fijada por esta TS, decisión del Arquitecto en ADR-GRP-007): `permissions.disableSafeMinimum: boolean`, por defecto `false`. Solo tiene efecto desde un suelo **confirmado y legible** (no `parcial`, no `ignorado`).

**`permissions`**: `allow`, `ask` y `deny` como listas de operaciones del catálogo de BR-VAL-002: `commit`, `push`, `force-push`, `reset-hard`, `branch-delete`, `rebase`, `merge`, `worktree-add`, `worktree-remove`. **`policies`**: objeto abierto, sin claves soportadas todavía (las añaden US-GRD-008, US-GRD-009 y US-GRD-015).

## 4. Nivel de equipo (ADR-GRD-004)

1. **Rama principal** (decisión 2): remoto `origin`, o el único; `refs/remotes/<r>/HEAD` → principal, si no `main`; copia: `refs/remotes/<r>/<principal>`, si no `refs/heads/<principal>`, si no ninguna.
2. **Fuentes**: suelo = blob de la copia; worktree = blob del `HEAD` del worktree del lector (`HEAD` sin nacer → `ausente`). Nunca el working tree.
3. **Combinación (D6)**: decisión por operación = máx(denegar > pedir > permitir) sobre el suelo efectivo y los endurecedores; sin regla = permitir. Los endurecedores no aportan `allow` ni `disableSafeMinimum`. El mínimo deniega `force-push` salvo que el suelo efectivo, confirmado y legible, lo desactive. Las razones son todas las fuentes que producen el máximo.
4. **Suelo efectivo (D7)**:
   - Sin confirmación → no hay suelo efectivo; el suelo resuelto solo endurece (fase 2 de § 3.5).
   - Mismo blob que el confirmado → el suelo resuelto.
   - Distinto: se comparan las **decisiones normalizadas** (ya con el mínimo) de cada uno como suelo. Si el resuelto relaja algo (incluido estar `ignorado` o `ausente` frente a un confirmado con reglas) → el confirmado es el suelo, el resuelto solo endurece y aparece `floor-relax-pending`. Si no relaja → el resuelto rige ya.
   - Blob confirmado ausente del odb → `confirmed-floor-missing` y el resuelto solo endurece.
5. **Rama base**: `TeamConfig::base_branch()` → `BaseBranch { name, status }`, la misma para el motor y Guardrails:

| Situación | `base_branch()` | `guarded_base_branches()` (solo Guardrails) | Diagnóstico |
|---|---|---|---|
| Sin confirmar, resuelta válida | resuelta, `Unconfirmed` | {main, principal, resuelta} | `base-unconfirmed` |
| Sin confirmar, `baseBranch` inválida (Q42) | ninguna, `Invalid` | {main, principal} | `base-unconfirmed`, `invalid-base-branch` |
| Confirmada = resuelta | confirmada | {confirmada} | — |
| Confirmada ≠ resuelta | confirmada | {confirmada, resuelta} | `base-change-pending` |
| Confirmada, `baseBranch` inválida | confirmada | {confirmada, main, principal} | `base-change-pending-invalid` |
| Suelo `ignorado` (fail-safe § 3.6) | confirmada | {main, principal, confirmada} | — |

6. **Caché (L-01)**: LRU acotada (64) de documentos parseados, clave (id de blob, nivel), segura entre hilos.
7. **Recarga**: `affects_team_config(ruta en el directorio Git)` es verdadero para `config`, `packed-refs`, `HEAD`, `refs/remotes/**`, `refs/heads/**` y `worktrees/<id>/HEAD`. El daemon vuelve a llamar a `load` cuando el observador reporta una de esas rutas.

## 5. Decisiones

- **Decisión del orquestador (2026-10-04), validada por el Arquitecto**: los reemplazos se desactivan en el handle del odb (`ignore_replacements`) y no con `core.useReplaceRefs`. Motivo comprobado con un test: gix 0.88 lee esa clave invertida, y `core.useReplaceRefs=false` en el config del repo **activa** los reemplazos.
- **Decisión del orquestador (2026-10-04), validada por el Arquitecto**: las claves duplicadas en un objeto y un BOM inicial dan `ignorado`. Un editor y el cargador podrían leer valores distintos.
- **Decisión del orquestador (2026-10-04), validada por el Arquitecto y el PO**: la persistencia de lo confirmado se implementa ya como getter/setter tipados sobre `store_meta`, sin migración. El cargador recibe lo confirmado como entrada y no confirma nunca. Los únicos escritores son los comandos de confirmación (US-GRD-001, US-GRD-014, US-GRD-007).
- **Decisión del orquestador (2026-10-04), validada por el PO**: suscribir `affects_team_config` al observador queda para TS-GRP-003 (daemon). Esta TS expone el predicado y la recarga, probados con un evento sintético.
- **Decisión del orquestador (2026-10-04), validada por el Arquitecto**: ⚠️ ASSUMPTION de presupuesto: carga en caliente ≤ 5 ms p95, para dejar margen a la evaluación dentro de los 30 ms de NFR-GRD-04. Medido en macOS (release): en caliente p50 ≈ 0,50 ms y p95 ≈ 0,52 ms; en frío (abrir el repo y cargar) ≈ 1,0 ms. El test de tiempo es `#[ignore]` para no dar fallos intermitentes en CI.

## 6. Plan de tests

| Criterio de la TS | Test |
|---|---|
| Sin commitear (edición y conflicto) | `team_config.rs::uncommitted_edit_or_conflict_does_not_change_the_team_level`, `a_real_merge_conflict_in_the_worktree_is_not_read` |
| Por worktree (D6), checkout antiguo, rama huérfana | `each_worktree_adds_its_own_hardening_and_never_relaxes`, `an_old_checkout_or_an_orphan_branch_keeps_the_floor_rules`, `a_hardening_on_the_main_branch_applies_in_every_worktree` |
| Suelo forjado (D7, J3) | `a_forged_lax_floor_keeps_the_most_restrictive_combination`, `a_forged_floor_without_settings_or_with_broken_settings_is_a_relaxation`, `without_confirmation_the_floor_only_hardens`, `a_confirmed_floor_lost_to_gc_leaves_the_floor_hardening_only` |
| Cambio de rama base | `a_base_change_keeps_the_confirmed_branch_and_guards_both`, `an_invalid_base_branch_never_reaches_git`, `unconfirmed_base_is_the_resolved_one_marked_and_guards_the_union` |
| Objetos de reemplazo y grafts | `replacement_objects_and_grafts_do_not_change_the_document`; en `crates/git`: `replacement_objects_do_not_change_the_read` (blob, árbol, commit, `core.useReplaceRefs`), `replacement_environment_variables_are_ignored`, `grafts_do_not_change_the_read` |
| Rama principal | `main_branch_resolution_order`, `a_bare_repository_reads_the_floor_the_same_way` |
| Commit local sin push | `a_local_commit_without_push_does_not_change_the_floor` |
| Dos consumidores | `engine_and_guardrails_get_the_same_confirmed_base_and_source_state` |
| Entradas hostiles | `hostile_entries_give_an_ignored_floor_and_the_fail_safe_union`, `a_partial_floor_forces_the_minimum`; unitarios de `strict.rs` y `document.rs` |
| Sin escrituras ni red | `loading_writes_nothing` (huella del repo y de los worktrees antes y después), `gix_has_no_network_feature` |
| Presupuesto | `budget_warm_load_p95` (`#[ignore]`, release) |
| Recarga y caché | `a_ref_change_reloads_the_floor`, `the_cache_is_bounded_and_keyed_by_blob`, `team.rs::reload_trigger_paths` |
| Deriva del schema | `schema.rs::schema_has_not_drifted`, `schema_uses_only_supported_keywords` |
| Persistencia de lo confirmado | `crates/core/tests/profile_team_baseline.rs` |

## 7. Fuera de alcance (y a quién pertenece)

- Lectura de los archivos de perfil y local y su precedencia sobre `engine`: US-GRP-013 y ADR-GRP-008, con `parse_document`.
- Semántica de `policies` (sus claves): US-GRD-008, US-GRD-009 y US-GRD-015. La evaluación: ADR-GRD-003 (US-GRD-007).
- Comandos de confirmación (escritores de lo confirmado): US-GRD-001, US-GRD-014 y US-GRD-007.
- Reacción de Guardrails ante un suelo ilegible: US-GRD-011.
- Suscripción al observador y verificación manual por CLI: TS-GRP-003 y US-GRP-016 / US-GRD-014.
- Arnés INF-GRP-001: no está en `main`. Se sustituye por una huella propia del repo; la ausencia de red se cumple por diseño (gix sin transporte de red), comprobado por un test del manifiesto.
- Windows y Linux: sin verificar desde este Mac.

## Estado de la implementación (2026-10-08)

Implementado en: PR #27.

- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
