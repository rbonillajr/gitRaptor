---
schema: nassa.approval-request.v1
plan: "docs/dev-briefs/ts-ckp-001-conflict-predictor.md"
---

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: no
- Generada: `2026-10-21T17:37:12.514Z`
- Sha de aprobación: `e11bcd7418d8b6df0cd126223950a6bf4a63f69d50fb6bacdbb44c0562d4cb90:f749eec43181df6ff7955b470809719e2b42ccd74e6094c5aecb76d0f79f0678:c2920ebff6029ee7611a37c41fefd49cba4313f235eabaf2a2e040401290342e`

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES 52b43f789483>>>
```text
docs/dev-briefs/ts-ckp-001-conflict-predictor.md
```
<<<FIN-DATOS-NO-CONFIABLES 52b43f789483>>>

planSha: `e11bcd7418d8b6df0cd126223950a6bf4a63f69d50fb6bacdbb44c0562d4cb90` (base de la cadena de re-planes)

Contenido:

<<<DATOS-NO-CONFIABLES c7cc9886f358>>>
````text
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
````
<<<FIN-DATOS-NO-CONFIABLES c7cc9886f358>>>

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 85e6b56d7552>>>
```text
docs/dev-briefs/ts-ckp-001-conflict-predictor.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 85e6b56d7552>>>

contractSha: `f749eec43181df6ff7955b470809719e2b42ccd74e6094c5aecb76d0f79f0678`

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 2ceb9fcde247>>>
```text
[1] id: CKP001-UNC-FULL
statement: T001: read_worktree retiene en `uncommitted` todas las rutas sin commitear (modificadas, preparadas y sin seguimiento) sin repetir, también por encima de MAX_WORKTREE_CHANGES, con partial=false
verify: cargo test -p gitraptor-core --test ts_ckp_001_uncommitted full:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/core/tests/ts_ckp_001_uncommitted.rs

[2] id: CKP001-UNC-CAP
statement: T001: UncommittedPaths::from_paths con más rutas que el tope guarda las primeras en orden de bytes y marca partial=true; MAX_UNCOMMITTED_FOR_OVERLAP vale 10.000
verify: cargo test -p gitraptor-core --test ts_ckp_001_uncommitted cap:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/core/tests/ts_ckp_001_uncommitted.rs

[3] id: CKP001-UNC-HOOK
statement: T001: el observador llama a worktree_recomputed con el conjunto completo al cerrar el recomputo de un worktree, y FanoutHooks reenvía worktree_recomputed y repo_tier_changed a cada hook
verify: cargo test -p gitraptor-core --test ts_ckp_001_uncommitted hooks:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/core/tests/ts_ckp_001_uncommitted.rs

[4] id: CKP001-PAIRS
statement: T002: pairs::plan forma cada worktree con trabajo propio contra la base (salvo el que tiene sacada la base) y cada pareja de worktrees con trabajo propio; un worktree sin trabajo propio no forma pares
verify: cargo test -p gitraptor-core --test ts_ckp_001_pairs pairs:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/core/tests/ts_ckp_001_pairs.rs

[5] id: CKP001-OLDBASE
statement: T002 guard 2.1: un worktree sobre una base más vieja no solapa por los commits que la base hizo después (el trabajo propio sale de merge-base..punta)
verify: cargo test -p gitraptor-core --test ts_ckp_001_pairs old_base:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/core/tests/ts_ckp_001_pairs.rs

[6] id: CKP001-OVERLAP
statement: T002: el solape W–base y W1–W2 es la intersección del trabajo propio (commiteado y sin commitear), y una entrada partial da un solape partial, nunca uno vacío
verify: cargo test -p gitraptor-core --test ts_ckp_001_pairs overlap:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/core/tests/ts_ckp_001_pairs.rs

[7] id: CKP001-BASE-STATES
statement: T002: base Unconfirmed deja los pares contra la base en Pending(BaseUnconfirmed) y los pares entre worktrees siguen; base Invalid o rama base sin ref da NotComputable(BaseMissing)
verify: cargo test -p gitraptor-core --test ts_ckp_001_pairs base_states:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/core/tests/ts_ckp_001_pairs.rs

[8] id: CKP001-STALE
statement: T003 guard 3.1: un resultado cuya triple ya cambió se descarta (complete devuelve false) y el par nunca queda Current con entradas viejas
verify: cargo test -p gitraptor-core --test ts_ckp_001_state stale:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/core/tests/ts_ckp_001_state.rs

[9] id: CKP001-CACHE
statement: T003: una triple igual a la de la caché no encola nada; una triple distinta encola el par y lo pasa a Recalculating si tenía resultado; un cambio solo de rutas sin commitear recalcula el solape sin encolar
verify: cargo test -p gitraptor-core --test ts_ckp_001_state cache:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/core/tests/ts_ckp_001_state.rs

[10] id: CKP001-QUEUE
statement: T003: PairQueue guarda como mucho un trabajo por par (el nuevo sustituye al anterior) y saca por prioridad ChangedVsBase, PresentSession, Rest, por orden de llegada dentro de cada una
verify: cargo test -p gitraptor-core --test ts_ckp_001_state queue:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/core/tests/ts_ckp_001_state.rs

[11] id: CKP001-OPINPROG
statement: T003: un worktree con operación en curso deja sus pares en Pending(OperationInProgress) sin encolar nada
verify: cargo test -p gitraptor-core --test ts_ckp_001_state op_in_progress:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/core/tests/ts_ckp_001_state.rs

[12] id: CKP001-DORMANT
statement: T003/D4: suspend(Dormant) deja cada par Pending(Dormant) con su último resultado y su hora; al volver a aplicar un plan se recalculan todos como en el cálculo inicial
verify: cargo test -p gitraptor-core --test ts_ckp_001_state dormant:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/core/tests/ts_ckp_001_state.rs

[13] id: CKP001-MODULE
statement: T003: el módulo predict está registrado con una línea en MODULES
verify: grep -qE '\("predict", predict::start\)' crates/core/src/daemon/modules/mod.rs
tests:
  (ninguno)

[14] id: CKP001-MERGE-BASES
statement: T002: RepoReader::merge_bases(a, b) con ids hex devuelve todas las merge-bases (dos en una historia cruzada), una en una historia simple y ninguna sin historia común
verify: cargo test -p gitraptor-git --test ts_ckp_001_merge_bases merge_bases:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/git/tests/ts_ckp_001_merge_bases.rs

[15] id: CKP001-HUNK-MARKER
statement: T004 guard 4.1: una línea que imita un marcador más corto o más largo que marker_size (32) es contenido, no un bloque
verify: cargo test -p gitraptor-git --test ts_ckp_001_hunks marker:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/git/tests/ts_ckp_001_hunks.rs

[16] id: CKP001-HUNK-JOIN
statement: T004 guard 4.2: dos bloques separados por 3 líneas o menos en los dos lados se publican como uno; separados por 4, como dos
verify: cargo test -p gitraptor-git --test ts_ckp_001_hunks join:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/git/tests/ts_ckp_001_hunks.rs

[17] id: CKP001-HUNK-SIDES
statement: T004: los rangos (inicio en 1 y longitud) están en las coordenadas de cada lado aunque las partes resueltas vengan del otro lado, y pasado max_hunks_per_file se corta y se marca truncado
verify: cargo test -p gitraptor-git --test ts_ckp_001_hunks sides:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/git/tests/ts_ckp_001_hunks.rs

[18] id: CKP001-DIRRENAME
statement: T004 guard 4.3: un archivo movido por un renombrado de directorio y un conflicto directorio/archivo sin etapa en el índice aparecen en la lista de conflictos
verify: cargo test -p gitraptor-git --test ts_ckp_001_dry_merge dir_rename:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/git/tests/ts_ckp_001_dry_merge.rs

[19] id: CKP001-KINDS
statement: T004: cada archivo en conflicto lleva su ConflictKind (Content, AddAdd, ModifyDelete, Rename, DirectoryFile), has_kind_needing_escalation es cierto solo con ModifyDelete o Rename, y pasado max_files se corta con truncated
verify: cargo test -p gitraptor-git --test ts_ckp_001_dry_merge kinds:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/git/tests/ts_ckp_001_dry_merge.rs

[20] id: CKP001-NODRIVER
statement: T005 guard 5.1: con merge.<x>.driver configurado y .gitattributes merge=<x> en disco, índice y árbol, ningún driver se ejecuta (el marcador del canario no aparece) y el conflicto se informa
verify: cargo test -p gitraptor-git --test ts_ckp_001_dry_merge no_driver:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/git/tests/ts_ckp_001_dry_merge.rs

[21] id: CKP001-BLOBCAP
statement: T005 guard 5.2: un blob por encima de max_blob_bytes (tamaño leído de la cabecera) deja el archivo como Binary sin hunks y sin error; uno por debajo da sus hunks
verify: cargo test -p gitraptor-git --test ts_ckp_001_dry_merge blob_cap:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/git/tests/ts_ckp_001_dry_merge.rs

[22] id: CKP001-MERGERULE
statement: T005 guard 5.3: una regla merge= en el .gitattributes del árbol de cualquiera de las dos puntas marca has_merge_rule; un .gitattributes solo en el disco o el índice no marca, y la regla no resuelve el conflicto
verify: cargo test -p gitraptor-git --test ts_ckp_001_dry_merge merge_rule:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/git/tests/ts_ckp_001_dry_merge.rs

[23] id: CKP001-ERRORS
statement: T005: un objeto ausente da DryMergeError::MissingObject, un repo superficial sin la merge-base da Shallow y un max_written_bytes pequeño da LimitExceeded
verify: cargo test -p gitraptor-git --test ts_ckp_001_dry_merge errors:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/git/tests/ts_ckp_001_dry_merge.rs

[24] id: CKP001-VIRTUAL
statement: T005: con dos merge-bases el resultado usa MergeBaseUsed::Virtual con las dos, y los marcadores de la base virtual no cuentan como bloques
verify: cargo test -p gitraptor-git --test ts_ckp_001_dry_merge virtual_base:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/git/tests/ts_ckp_001_dry_merge.rs

[25] id: CKP001-INTACT
statement: T005/NFR-01: merge_in_memory deja el repo observado intacto (huella de gitraptor_testkit con mtime de packs, objetos sueltos y directorios de objects/)
verify: cargo test -p gitraptor-git --test ts_ckp_001_dry_merge repo_intact:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/git/tests/ts_ckp_001_dry_merge.rs

[26] id: CKP001-NO-HIGHLEVEL
statement: T005/L-04: crates/git/src/dry_merge no llama a merge_trees, merge_commits, merge_resource_cache, tree_merge_options ni Repository::attributes*
verify: test -f crates/git/src/dry_merge/memory.rs && ! grep -rqE 'merge_trees|merge_commits|merge_resource_cache|tree_merge_options|\.attributes(_only)?\(' crates/git/src/dry_merge
tests:
  (ninguno)

[27] id: CKP001-G9
statement: G9: la excepción de gix_writes_only_in_store_writer cubre solo crates/git/src/dry_merge/memory.rs y solo si contiene with_object_memory
verify: grep -q 'with_object_memory' crates/git/src/dry_merge/memory.rs && grep -q 'dry_merge/memory.rs' crates/git/tests/static_check.rs && grep -q 'with_object_memory' crates/git/tests/static_check.rs
tests:
  (ninguno)

[28] id: CKP001-FIDELITY
statement: T011: sobre el corpus R-SYN (31 casos con la demo del BRD § 13 y un caso de historia cruzada), los archivos en conflicto coinciden al 100 % con git merge-tree en un clon temporal, los rangos de hunks son idénticos y la demo no tiene falsos negativos
verify: cargo test -p gitraptor-git --test dry_merge_fidelity r_syn:: 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\. [1-9][0-9]* passed'
tests:
  - crates/git/tests/dry_merge_fidelity.rs

[29] id: CKP001-STATUS
statement: PC-6: TS-CKP-001 queda partially-implemented en su ficha y en el índice de release-status.md (docs-lint comprueba en CI que el archivo coincide con el generador)
verify: grep -q '^status: partially-implemented' docs/requirements/features/cockpit/technical-stories/TS-CKP-001-predictor-conflictos.md && grep -E 'TS-CKP-001' docs/requirements/release-status.md | grep -q 'partially-implemented'
tests:
  (ninguno)
```
<<<FIN-DATOS-NO-CONFIABLES 2ceb9fcde247>>>

## Tests de criterio

testsSha: `c2920ebff6029ee7611a37c41fefd49cba4313f235eabaf2a2e040401290342e`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES dd37e4f1f3f2>>>
```text
5a31515aa56f1240bd8d454cfc612c7b333646149b662426f7dbf4df47eedfb0  crates/core/tests/ts_ckp_001_pairs.rs
3fc07685e5396ed9e0e59bd22c558152d7936255740657a91a38244bcb9379ad  crates/core/tests/ts_ckp_001_state.rs
7d826721c72a0a92d05e4c9259a361f5d2b875a1a8c562aebeed22626bc53987  crates/core/tests/ts_ckp_001_uncommitted.rs
a2eb3aaabd47f81d4978532b33981eccca73d5dc8f930d5df0f6e6cee8d493a7  crates/git/tests/dry_merge_fidelity.rs
633ee91a293816b4baf7cb424d27e1df622a08073998798c85eae12ec8ab0d8a  crates/git/tests/ts_ckp_001_dry_merge.rs
cba4dbaf27824bc4ed81252c6fdea2ab6ac1e92fcb06c2edeef1037d5499c388  crates/git/tests/ts_ckp_001_hunks.rs
d9bc5e98c385181b037ea445ea0fea519f1a557f05dfa5a9b5ae181b357ffcf7  crates/git/tests/ts_ckp_001_merge_bases.rs
```
<<<FIN-DATOS-NO-CONFIABLES dd37e4f1f3f2>>>

## Crítico

- Rondas (plan): 1
  - ronda 1: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
- Último veredicto: APROBABLE
- Excepción: no

## Diferidos

Ninguno: el crítico dio el plan por aprobable.

## Firmas

- planSha (contexto, no firmado): `e11bcd7418d8b6df0cd126223950a6bf4a63f69d50fb6bacdbb44c0562d4cb90`
- contractSha: `f749eec43181df6ff7955b470809719e2b42ccd74e6094c5aecb76d0f79f0678`
- testsSha: `c2920ebff6029ee7611a37c41fefd49cba4313f235eabaf2a2e040401290342e`
- approvalSha: `e11bcd7418d8b6df0cd126223950a6bf4a63f69d50fb6bacdbb44c0562d4cb90:f749eec43181df6ff7955b470809719e2b42ccd74e6094c5aecb76d0f79f0678:c2920ebff6029ee7611a37c41fefd49cba4313f235eabaf2a2e040401290342e`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
