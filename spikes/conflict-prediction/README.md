# SPIKE-CKP-001 — prototipo del predictor de conflictos (`ckp-spike`)

Prototipo aislado del [plan de SPIKE-CKP-001](../../docs/requirements/features/cockpit/research/SPIKE-CKP-001-plan.md). **No es código del producto**: es un workspace de Cargo propio (no es miembro del de GitRaptor) y depende de `gitraptor-testkit` por ruta solo para la huella de INF-GRP-001 (`fingerprint`) y el generador del repo de referencia (`repogen`). Los resultados y la decisión están en el [Research Brief](../../docs/requirements/features/cockpit/research/SPIKE-CKP-001-resultados.md).

Todos los repos viven en `CKP_ROOT` (por defecto `/private/tmp/ckp-spike`), un directorio fuera de cualquier repo y marcado como raíz del testkit. Nunca se toca este repo ni el perfil real (NFR-01).

## Alternativas implementadas

| Id | Dónde | Qué hace |
|---|---|---|
| A1 | `src/a1.rs` | Solo control (P1): `Repository::merge_commits` de alto nivel. Sirve para demostrar que el canario salta con él |
| A2 | `src/a2.rs` | `gix_merge::commit` directo, con plataformas propias: sin drivers de merge ni de diff, pila de atributos vacía, filtros sin drivers, `rewrites.limit` fijo, marcadores de 32 caracteres y la memoria de objetos de `gix`. Todo acceso a objetos pasa por `Guarded`: tope de tamaño leído de la cabecera, plazo y tope de bytes escritos |
| A2-W | `src/worker.rs` | A2 en un proceso trabajador de vida larga (el propio binario, `ckp-spike worker`): `RLIMIT_CPU`, temporizador por par, vigilancia de la memoria residente con `proc_pidinfo` y relanzamiento |
| B | `src/gitcli.rs` | `git merge-tree --write-tree` con `--git-dir` en un almacén *bare* del perfil con `objects/info/alternates` hacia el repo; argv fijo, entorno vacío, configuración aislada |
| Referencia | `src/gitcli.rs` | `git merge-tree --write-tree` en un clon `--mirror --no-local` desechable, con el mismo límite de renombrados |

Además: `src/hunks.rs` (rangos de líneas por lado desde el blob fusionado), `src/prefilter.rs` (H6) y `src/bench.rs` (E8 a E11).

## Uso

```sh
cargo build --release
./target/release/ckp-spike batch a2  <repo> <pares.tsv> [--hunks]      # una línea TSV por par
./target/release/ckp-spike batch ref <repo> <pares.tsv> --scratch <dir> # referencia merge-tree
./target/release/ckp-spike intact <dir> -- <comando…>                  # huella antes y después
./target/release/ckp-spike bench recompute <flota> --engine a2w        # E8
```

`pares.tsv`: `<ours>\t<theirs>` por línea (ramas o ids). Salida: `ours, theirs, ok|err, ms, n, rutas, tipos, hunks (JSON), error`.

## Suites (macOS)

| Suite | Experimentos | Evidencia |
|---|---|---|
| `setup-git.sh` | Compila Git 2.38.5 y 2.56.0 desde los tags de un clon de `git/git` en `.tools/` (la 2.50.1 es la de Apple) | — |
| `suites/e1-intact.sh [runs]` | E1: huella de la flota (directorio común, `.git/worktrees/*`, cada working tree; con mtime) en 4 escenarios, 2 flotas (clon completo y clon con `alternates`) | `results/…/e1-intact.tsv` y `.log` |
| `suites/e2-e4-canary.sh` | E2 (canario y PATH trampa), E3 (*partial clone* con `uploadpack` trampa), E4 (`refs/replace`, `info/grafts`, `merge=<x>` por las tres vías) | `results/…/e2-e4-canary.tsv` |
| `suites/e6-e7-fidelity.sh [N] [muestra]` | E6, E7 y H6 sobre R-SYN y los últimos N merges de dos padres de `git/git`, contra las 3 versiones de Git; comprobación P4 con `git merge` | `results/…/e6-e7-fidelity.txt` |
| `suites/e8-e9-perf.sh <etiqueta>` | E8 y E9 (parte en proceso) sobre una flota nueva de R-H | `results/…/e8-e9-perf-<etiqueta>.txt` |
| `suites/e10-hostile.sh [muestras]` | E10 sobre R-HOS | `results/…/e10-hostile.txt` |
| `measure.sh` | **Medición final (fase b), con la máquina en reposo**: todo lo anterior más H9 (gix 0.88) y el p95 del motor de INF-GRP-002 con y sin ráfaga del predictor | `results/final-<fecha>/` |

`gix088/` compila las mismas fuentes contra `gix` 0.88 / `gix-merge` 0.21 (H9), con la feature `gix088`.

Prueba de la barrera de API (E5): `cargo test` incluye `tests/api_barrier.rs`, que falla si `src/a2.rs` llama a `merge_trees`, `merge_commits` o `merge_resource_cache`.

## Linux y Windows

**Pendiente: etapa de validación multiplataforma.** El código de A2 es portable; el de A2-W usa `proc_pidinfo` (macOS) para la memoria residente del hijo: en Linux hay que leer `/proc/<pid>/statm` y en Windows `GetProcessMemoryInfo`. Las suites son bash y asumen el `sed` de BSD (`sed -i ''`). El procedimiento es el mismo: `setup-git.sh`, las suites en orden y `measure.sh`.
