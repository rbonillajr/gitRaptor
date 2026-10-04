# Prototipo SPIKE-TMC-001: overhead del snapshot previo

Prototipo **aislado** del almacén de snapshots de ADR-TMC-001 y del reparto por etapa de ADR-TMC-006. No es código de producto: es un workspace de Cargo independiente (no es miembro del workspace de GitRaptor) y no depende de `crates/` ni de `apps/`.

Resultados y veredicto: [`docs/requirements/features/time-machine/research/SPIKE-TMC-001-resultados.md`](../../docs/requirements/features/time-machine/research/SPIKE-TMC-001-resultados.md).

## Reproducir

```sh
spikes/snapshot-overhead/run.sh                 # repo de referencia (perfil M), banco completo (~45 min)
spikes/snapshot-overhead/run.sh S --iters 20 --week 50   # pasada rápida de humo (~5 min)
SPIKE_ROOT=/ruta/scratch spikes/snapshot-overhead/run.sh M --variants gix-hint,cli
```

Opciones: `--profile S|P50|M|L`, `--variants cli,fi,fi-hint,gix-hint`, `--iters N`, `--week N` (capturas de la semana simulada; `0` la omite), `--no-multi`, `--no-big`, `--no-seed`. `SPIKE_GIT` fuerza el binario de Git.

El informe se escribe en `<SPIKE_ROOT>/results-<perfil>.md`. Todo (repo generado, 10 worktrees, almacenes y perfiles) vive en un directorio temporal **fuera de cualquier repo Git**: el binario se niega a arrancar si `--root` está dentro de un repo (NFR-01).

Requisitos: Rust (toolchain del repo), Git ≥ 2.38 y unos 5 GB libres para el perfil M con 10 worktrees.

Informes crudos de las pasadas del spike (macOS, Apple M5): [`results/2026-10-04-macos-m5/`](results/2026-10-04-macos-m5/). La muestra pública (`public-sample-github-2026-10-04.tsv`) tiene estas columnas: repo, archivos, bytes del árbol, árbol truncado, commits de la rama por defecto y tamaño del repo en KB según GitHub.

## Qué hace

- `src/repogen.rs`: generador determinista de repos sintéticos con `git fast-import`. Con la misma semilla produce los mismos commits (el informe anota el `HEAD`). Perfiles: `S` (humo), `P50` (alternativa en la mediana de la muestra pública), `M` (propuesta de repo mediano) y `L` (mayor que la referencia).
- `src/snap.rs`: almacén bare privado en el perfil (`store.git`, sin remotos, sin hooks, `gc.auto=0`, sin reflogs, carpeta 0700) y captura de un snapshot con la forma de ADR-TMC-001 § 1: commit del almacén con `wt/<k>/files`, `wt/<k>/index` y `meta`, cuyos padres son HEAD y las ramas, más la ref `refs/tm/snap/<id>` y una fila de oplog con barrera de durabilidad. Cuatro variantes, que siguen los escalones de ADR-TMC-006 § 5:
  - `cli`: Git CLI, un proceso por paso (línea base).
  - `fi`: escalón 1, con `git fast-import`, `cat-file --batch-check` y `update-ref --stdin` persistentes.
  - `fi-hint`: escalones 1 y 2. Las rutas cambiadas llegan como "pista" del motor y se comprueban con una caché de stat, en lugar de un `git status` completo.
  - `gix-hint`: escalones 2 y 3. Escritura del almacén en el proceso con gitoxide y blobs en paralelo.
- `src/bench.rs`: escenarios (deltas de 0, 1, 10, 100 y 1.000 archivos; 100 archivos con 20 MB; archivos grandes sin seguimiento; 10 worktrees con capturas de fondo; ámbito de 10 worktrees; semana simulada; siembra) y la **guarda de integridad**. En cada ventana medida compara la huella de todo el `.git` del usuario y de sus worktrees (contenido, tamaño, mtime e inodo), `for-each-ref`, `log --all` y `stash list`. El banco falla si cambia algo.

## Límites conocidos

- Sin daemon ni canal: la etapa "admisión" de ADR-TMC-006 no se mide.
- La variante `*-hint` supone que el motor entrega las rutas cambiadas (ADR-GRP-010). El motor aún no existe, así que el banco le pasa las rutas que acaba de editar.
- La fila del oplog es un archivo con `F_FULLFSYNC`, no SQLite.
- Solo se ejecutó en macOS. Linux y Windows tienen el procedimiento en el documento de resultados.
