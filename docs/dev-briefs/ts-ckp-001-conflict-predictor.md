# Brief · TS-CKP-001 PR-A: solape, pares y merge en seco en memoria (2026-10-21)

Plan: [DS-TS-CKP-001](../requirements/features/cockpit/dev-specs/TS-CKP-001-predictor-conflictos.md) (rama R-A2W), **acotado**. Lo preparó el loop principal: no rediseña, traduce la Dev Spec a firmas que las pruebas del contrato puedan usar. Todo lo no dicho aquí se rige por la Dev Spec (citada como `DS §`).

## Ruta ante el spec en rojo

`devspec-map classify` da `spec-red` (D1: `gaps_blocking: 2`, G2 y G5). Ruta: **acotar el alcance** a las tareas que ningún hueco bloquea, que es lo que la propia Dev Spec prevé (`blocked_tasks`, D3, D9).

- **Entra:** T001, T002, T003, T004, T005, T011, y de T010 solo la excepción de `static_check.rs` para `dry_merge/memory.rs` (G9), sin la cual la suite no compila en verde.
- **Queda fuera (bloqueado):** T006 y T007 (G5: TS-GRP-005 sin implementar), T009 (G2: forma del contrato), T010 (salvo lo dicho), T012 y T013 por arrastre. TS-CKP-001 queda `partially-implemented`.

## Deriva de citas (sin cambio de decisión)

1. `observe.rs:793` `bounded()` y `read_worktree` (`observe.rs:669`) siguen como dice T001. `all_changes` relee el repo: sigue igual (la retención la consume el predictor, no la paginación).
2. `reader.rs:488` `merge_base(&RefName, &RefName) -> Option<String>`: T002 añade `merge_bases(&str, &str)` aparte, sin tocar la existente.
3. `reader.rs:641` `permissions()`: `config.system`/`attributes.system` valen `cfg!(not(windows))`. `ignore_ambient_config` apaga además la config global y `HOME` (y con ellas el `safe.directory` del usuario), y es solo de tests: **no se usa**. T005 abre con un `pub(crate) fn RepoReader::open_for_dry_merge(path)` que parte de `permissions(false)` y apaga solo `config.system` y `attributes.system` (DS T005, `reader.rs:639`), con `bail_if_untrusted` e `ignore_replacements`.
4. `static_check.rs:64` `GIX_WRITE_PATTERNS` incluye `write_buf`/`write_stream`: el envoltorio de T005 los implementa. Excepción de G9 en este PR.
5. `ObserverHooks` (`watch/mod.rs:223`) y `MODULES` (`daemon/modules/mod.rs:51`) como dice la spec.

## Firmas públicas (las usan las pruebas del contrato)

Las pruebas del contrato son de integración (`crates/*/tests/ts_ckp_001_*.rs`), así que lo que prueban es `pub`.

```rust
// gitraptor_core::observe
pub const MAX_UNCOMMITTED_FOR_OVERLAP: usize = 10_000;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UncommittedPaths { pub paths: Arc<BTreeSet<String>>, pub partial: bool }
impl UncommittedPaths { pub fn from_paths(paths: impl IntoIterator<Item = String>, max: usize) -> Self } // sin repetir, primeras `max` en orden de bytes
pub struct WorktreeRead { /* … */ pub uncommitted: UncommittedPaths }          // vacío si no se pudo leer
// gitraptor_core::watch — por defecto vacíos; FanoutHooks los reenvía
fn worktree_recomputed(&self, _repo_id: &str, _root: &Path, _paths: &UncommittedPaths) {}
fn repo_tier_changed(&self, _repo_id: &str, _tier: Tier) {}

// gitraptor_git
impl RepoReader { pub fn merge_bases(&self, a: &str, b: &str) -> Result<Vec<String>, ReadError> } // vacío sin historia común
pub mod dry_merge;  // tipos de DS § Contratos compartidos, todos `pub`, `Debug, Clone, PartialEq, Eq`
impl Default for DryMergeLimits {}                    // valores de DS § Valores numéricos
impl DryMergeOutcome { pub fn has_kind_needing_escalation(&self) -> bool }
pub fn merge_in_memory(repo: &Path, input: &DryMergeInput, limits: &DryMergeLimits) -> Result<DryMergeOutcome, DryMergeError>;
pub fn hunk_ranges(merged: &[u8], ours: &[u8], theirs: &[u8], limits: &DryMergeLimits) -> (Vec<LineRange>, Vec<LineRange>, bool);

// gitraptor_core::predict — tipos de DS § Contratos compartidos, `pub`
pub struct BaseInput { pub status: BaseStatusView, pub tip: Option<String>, pub checked_out_at: Option<PathBuf> } // tip None: rama sin ref
pub struct WorktreeInput { pub root: PathBuf, pub tip: Option<String>, pub uncommitted: UncommittedPaths, pub in_progress: bool, pub session_present: bool }
pub mod pairs   { pub fn plan(reader: &RepoReader, repo_id: &str, base: &BaseInput, wts: &[WorktreeInput]) -> Result<Vec<PairPrediction>, ReadError>; }
pub mod overlap { pub fn intersect(a: &OwnWork, b: &OwnWork) -> Overlap; pub struct OwnWork { pub paths: BTreeSet<String>, pub partial: bool } }
pub mod state {
    pub struct RepoPairs;   // pares de un repo con su caché por triple
    impl RepoPairs {
        pub fn new(repo_id: &str) -> Self;
        pub fn apply(&mut self, plan: Vec<PairPrediction>, now_utc_ms: i64) -> Vec<(PairKey, Triple)>; // lo que hay que encolar
        pub fn complete(&mut self, key: &PairKey, triple: &Triple, result: Result<DryMergeOutcome, DryMergeError>, now_utc_ms: i64) -> bool; // false: triple vieja, descartado
        pub fn suspend(&mut self, reason: PendingReason);   // D4/D5: conserva último resultado y hora
        pub fn get(&self, key: &PairKey) -> Option<&PairPrediction>;
        pub fn pairs(&self) -> impl Iterator<Item = &PairPrediction>;
    }
}
pub mod queue {
    #[derive(PartialOrd, Ord, ...)] pub enum Priority { ChangedVsBase, PresentSession, Rest }
    pub struct PairQueue; impl PairQueue { pub fn push(&mut self, key: PairKey, triple: Triple, p: Priority); pub fn pop(&mut self) -> Option<(PairKey, Triple)>; pub fn len(&self) -> usize; }
}
```

## Supuestos

- `plan` da el estado inicial de cada par: `Calculating` (triple calculable), `Pending(BaseUnconfirmed)`, `Pending(OperationInProgress)` o `NotComputable(BaseMissing)`; el solape va calculado en todos. `apply` decide `Recalculating` (había resultado) frente a `Calculating`, y no encola ni los `Pending` ni los `NotComputable`, ni una triple igual a la de la caché.
- `complete` con `Err`: estado `NotComputable` con el motivo de DS § Forma del error (`TooLarge` no llega: lo absorbe `merge_in_memory`).
- `Triple.merge_base`: la primera merge-base por orden de bytes cuando hay varias (clave de caché; el merge virtual lo arma `gix_merge::commit`).
- `PairKey.a` es el worktree de menor ruta en orden de bytes en los pares entre worktrees.
- Prioridad estable: dentro de una misma prioridad, por orden de llegada. El cálculo inicial usa `PresentSession` para los pares con sesión presente y `Rest` para los demás.
- El módulo `predict` se registra en `MODULES` (DS T003 paso 8) y calcula pares y solape en el acto; sin T007 la cola no se vacía (ningún trabajo sale). Está acotada por construcción: un trabajo por par, 55 por repo.
- `merge_in_memory` usa los re-exports de `gix` (`gix::merge`, `gix::diff`, `gix::filter`, `gix::worktree`, `gix::index`, `gix::objs`, `gix::attrs`) con la feature `merge`. Añadir un `gix-*` directo solo si un tipo no está re-exportado, con la versión que ya está en `Cargo.lock`.
- El generador R-SYN (`gitraptor_testkit::syn_corpus`) porta `mk-syn.sh` a Rust con el Git CLI de la máquina en un directorio temporal, como el resto de `testkit`, y expone `pub fn generate(dir: &Path) -> Vec<SynCase>` con `SynCase { name, repo, ours, theirs, demo: bool }` (ids hex).

- **Guards de la DS como tests de integración.** La DS los nombra como tests `--lib`; aquí viven en `crates/*/tests/ts_ckp_001_*.rs` para que el contrato los congele en su propio archivo: ⛔2.1 → `ts_ckp_001_pairs::old_base::a_worktree_on_an_older_base_does_not_overlap_by_base_commits`; ⛔3.1 → `ts_ckp_001_state::stale::a_result_of_stale_inputs_is_never_current`; ⛔4.1 → `ts_ckp_001_hunks::marker::a_line_that_mimics_a_short_marker_is_content`; ⛔4.2 → `ts_ckp_001_hunks::join::blocks_three_lines_apart_are_joined`; ⛔4.3 → `ts_ckp_001_dry_merge::dir_rename::a_directory_rename_conflict_is_listed`; ⛔5.1 → `ts_ckp_001_dry_merge::no_driver::a_merge_attribute_runs_no_driver`; ⛔5.2 → `ts_ckp_001_dry_merge::blob_cap::a_blob_over_the_cap_is_binary_without_loading`; ⛔5.3 → `ts_ckp_001_dry_merge::merge_rule::a_union_rule_in_either_tip_marks_the_file`.
- El «sin cargar» de ⛔5.2 (el tamaño sale de la cabecera antes de cargar) no tiene código de salida propio: el test comprueba el resultado (Binary, sin hunks, sin error) y la revisión comprueba que el envoltorio lee la cabecera antes que el objeto.
- `crates/core/tests/observe.rs:279` construye `WorktreeRead` con un literal: el experto le añade `uncommitted: Default::default()` (no es prueba del contrato).
- Esta corrida marca TS-CKP-001 y DS-TS-CKP-001 como `partially-implemented` (con lo que falta), su fila del backlog y del índice, y regenera `release-status.md` (tramo 3).

- El tope de rutas de `changed_paths` en `pairs::plan` es `MAX_UNCOMMITTED_FOR_OVERLAP`; la regla «`total` mayor que `max` da solape `partial`» (DS T002) no tiene prueba del contrato (haría falta un repo de más de 10.000 rutas): la cubre un test unitario del experto en `predict/pairs.rs` y la revisión.
- Las pruebas del contrato fijan además: `PairQueue::default()`, los tipos y funciones del merge en seco bajo `gitraptor_git::dry_merge::` (sin re-export en la raíz) y `RepoReader::open(path, &ReaderOptions::default())`.

- **Suite del contrato acotada** a `-p gitraptor-git -p gitraptor-core -p gitraptor-testkit -p gitraptor-mcp`. Medida completa en la preparación, la base dio 6 rojos en `gitraptor-cli` (carga media 74 con otros workers); los 6 pasan solos (`cargo nextest run -p gitraptor-cli` con su filtro: 6/6). Los tests de daemon de `apps/cli` (que arrancan el módulo `predict`) se certifican solo en el `cargo test --workspace` previo al PR (AGENTS.md) y en CI; la capa estática (`cargo clippy --workspace --all-targets`) sí compila todo el workspace en la corrida.

### Inventario de reutilización

- **Reutiliza:** `gitraptor_testkit::fingerprint::{Scope, Snapshot::take, diff}` y `control::check` (huella con mtime, `crates/testkit/src/fingerprint.rs:85`, `control.rs:186`) para CKP001-INTACT; `gitraptor_testkit::Fixture` para los repos; `RepoReader::changed_paths` (`changed_paths.rs:66`) para el trabajo propio; `observe::changes` (`observe.rs:759`) como fuente de `UncommittedPaths`; `FanoutHooks` (`watch/mod.rs:243`).
- **Genera:** ninguno aplica.
- **Crea nuevo:** `dry_merge/*` y el envoltorio de objetos acotado (consulta: `capability-catalog query "bounded object find"` no da nada; el único precedente es el del SPIKE, `spikes/conflict-prediction/src/a2.rs`, que se porta); `testkit::syn_corpus` (no existe; porta `mk-syn.sh`); `predict/*`.

## Decisiones abiertas

Ninguna: las decisiones de producto y arquitectura son D1–D14 de la Dev Spec. La única de esta corrida (acotar el alcance) es la que la Dev Spec ya traza.

## File & Project Topology

### Tramo 1 · `crates/git` (merge en seco y merge-bases) — criterios: CKP001-HUNK-MARKER CKP001-HUNK-JOIN CKP001-HUNK-SIDES CKP001-DIRRENAME CKP001-KINDS CKP001-NODRIVER CKP001-BLOBCAP CKP001-MERGERULE CKP001-ERRORS CKP001-VIRTUAL CKP001-INTACT CKP001-MERGE-BASES CKP001-NO-HIGHLEVEL CKP001-G9 CKP001-FIDELITY
- `crates/git/Cargo.toml` (feature `merge`), `Cargo.lock`
- `crates/git/src/lib.rs`, `crates/git/src/reader.rs`
- `crates/git/src/dry_merge/{mod,conflicts,hunks,memory,merge_attr}.rs`
- `crates/git/tests/static_check.rs` (excepción G9: solo `dry_merge/memory.rs` y solo si contiene `with_object_memory`)
- `crates/testkit/src/syn_corpus.rs`, `crates/testkit/src/lib.rs`

### Tramo 2 · `crates/core` (rutas sin commitear, pares, solape, estados, cola, módulo) — criterios: CKP001-UNC-FULL CKP001-UNC-CAP CKP001-UNC-HOOK CKP001-PAIRS CKP001-OLDBASE CKP001-OVERLAP CKP001-BASE-STATES CKP001-STALE CKP001-CACHE CKP001-QUEUE CKP001-OPINPROG CKP001-DORMANT CKP001-MODULE
- `crates/core/src/observe.rs`, `crates/core/src/watch/mod.rs`, `crates/core/src/lib.rs`
- `crates/core/src/predict/{mod,pairs,overlap,state,queue}.rs`
- `crates/core/src/daemon/modules/{mod,predict}.rs`
- `crates/core/tests/observe.rs` (literal de `WorktreeRead`)

### Tramo 3 · documentos de estado — criterios: CKP001-STATUS
- `docs/requirements/features/cockpit/technical-stories/TS-CKP-001-predictor-conflictos.md`, `docs/requirements/features/cockpit/technical-stories.md`
- `docs/requirements/features/cockpit/dev-specs/TS-CKP-001-predictor-conflictos.md` (solo frontmatter y "Implementado en")
- `docs/requirements/backlog.md`, `docs/requirements/release-status.md` (regenerado)

El tramo 2 depende de los tipos del tramo 1 (`DryMergeOutcome`, `merge_bases`): van en serie.

## Not Built (deferred)

| Tarea | Por qué | Dueño |
|---|---|---|
| T006 trabajador, T007 pool | G5 (TS-GRP-005 sin implementar) | Orquestador |
| T009 publicación | G2 (forma del contrato) | Worker del canal (TS-GRP-004) |
| T010 (salvo G9), T012, T013 | Por arrastre de T006/T009 | Siguiente PR de TS-CKP-001 |
| Ejecutar `real_git_corpus` (R-REAL) | El test se escribe con `#[ignore]`; se corre a mano al subir `gix` | Orquestador |
