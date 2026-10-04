# Prototipo SPIKE-GRP-002: viabilidad del observador de cambios

Prototipo **aislado** para responder a [SPIKE-GRP-002](../../docs/requirements/features/motor-local/technical-stories/SPIKE-GRP-002-viabilidad-observador.md). Los resultados y su análisis están en [`SPIKE-GRP-002-resultados.md`](../../docs/requirements/features/motor-local/research/SPIKE-GRP-002-resultados.md).

- No es código del motor ni entra en `crates/` ni en `apps/`: es un workspace de Cargo propio (`[workspace]` vacío en su `Cargo.toml`), fuera del workspace del producto.
- Solo trabaja con repos sintéticos que genera en un directorio temporal y borra al terminar (NFR-01). Nunca abre este repo.
- Reproduce el diseño de ADR-GRP-010: un watcher `notify` compartido, una ventana fija de debounce por worktree, recomputo incremental con una caché de stat en memoria, persistencia en SQLite (WAL + `synchronous=FULL` + `fullfsync`) antes de publicar, publicación por un socket local, sondeo de respaldo y reconciliación. Toma las marcas de tiempo de ADR-GRP-011 § 2 (`t0`, `t_recv`, `t_flush`, `t_computed`, `t_persisted`, `t_client_recv`).

## Ejecutar (un solo comando)

Desde la raíz del repo:

```sh
cargo run --release --manifest-path spikes/watcher-viability/Cargo.toml -- --out spikes/watcher-viability/results
```

Requisitos: Rust 1.99 o superior y Git 2.38 o superior en el `PATH`. La corrida completa (repo de 100K commits, 10 worktrees y 200 muestras por escenario) tarda unos 15 a 25 minutos. Escribe `results/results-<so>.json` (`macos`, `linux` o `windows`). Las dos corridas de macOS que respaldan el documento de resultados están guardadas como `results/results-macos-run1.json` y `results/results-macos-run2.json`. La primera es anterior al experimento `handles`.

| Opción | Efecto |
|---|---|
| `--quick` | Repo de 20K commits, 40 muestras y ráfagas más cortas, para probar el arnés en unos 3 minutos |
| `--samples N` | Muestras por escenario de latencia (los de alta y baja de worktree usan N/4, con un mínimo de 20) |
| `--commits N` | Commits del repo sintético |
| `--only a,b` | Solo esos experimentos |
| `--out DIR` | Carpeta de resultados (por defecto `results/`) |
| `--keep` | No borra los repos temporales al terminar |

## Experimentos

| Nombre | Qué mide | Pregunta del SPIKE |
|---|---|---|
| `intact` | Huella (ruta, tamaño, mtime, inodo y hash de los metadatos de Git) antes y después de observar, reconciliar y sondear | Repo intacto |
| `latency` | p50/p95/p99/máx. por etapa y total al modificar un archivo, `git add`, commit, checkout y crear y borrar un worktree | Latencia por etapa e interpretación de p95 |
| `debounce` | Ventanas fijas de 0, 25, 50, 75, 100 y 150 ms frente a una deslizante de 75 ms, durante una ráfaga de 2.000 archivos en 2 s | Debounce de 75 ms |
| `timer` | Retraso real de `recv_timeout` (la primitiva de la ventana) | Debounce de 75 ms |
| `scale` | Ráfaga de 10K archivos en un worktree midiendo los otros nueve; descriptores, memoria, CPU en reposo; ráfaga de 10K en un directorio ignorado | Escala |
| `polling` | Coste del sondeo de respaldo (huella barata) y de un ciclo del modo degradado (estado completo por stat) | Sondeo |
| `gaps` | Eventos descartados (desbordamiento o suspensión simulados), watcher reiniciado y recreación del stream de FSEvents | Huecos |
| `coalescing` | Eventos que entrega FSEvents con 200 escrituras al mismo archivo | macOS |
| `persistence` | Una transacción SQLite por lote (1 a 1.000 filas) con `fullfsync` activado y desactivado | `fsync` dentro del presupuesto |
| `ahead_behind` | `git rev-list --left-right --count` en el repo de 100K commits, con y sin `commit-graph` | Escala de historia |
| `handles` | `git worktree remove`, borrado y renombrado de la raíz y de archivos con el watcher activo | Windows |

## Reproducir en Linux y Windows

Los dos están **sin verificar**: el prototipo solo se ha ejecutado en macOS. El procedimiento es el mismo comando en una máquina de cada SO, con Git 2.38 o superior y Rust 1.99 o superior.

**Linux** (inotify):

1. Anota `cat /proc/sys/fs/inotify/max_user_watches` y `max_user_instances`.
2. Ejecuta la corrida completa. Guarda `results/results-linux.json`.
3. Agotamiento de watches: en una VM o un contenedor desechable, baja el límite (`sudo sysctl fs.inotify.max_user_watches=2000`) y repite `--only latency,scale`. El prototipo usa el modo recursivo de `notify`, que registra un watch por directorio **incluidos los ignorados y `.git/objects`**. Para medir el diseño de ADR-GRP-010 hay que registrar solo los no ignorados, así que el recuento de watches de esta corrida es una cota superior. Lo que se espera ver: un error de registro (`notify::ErrorKind::MaxFilesWatch`) y el worktree en modo degradado, sin que se caigan los demás. El prototipo **no implementa** el paso a degradado por worktree: lo reporta como `error` en el JSON. Esa parte queda para el desarrollo de US-GRP-003 y US-GRP-004.
4. Desbordamiento forzado: baja `fs.inotify.max_queued_events` (p. ej. a 256) en la VM y repite `--only scale`. El contador `rescans` debe ser mayor que cero (`IN_Q_OVERFLOW`) y `gaps` debe seguir dando 100% tras reconciliar.

**Windows** (ReadDirectoryChangesW):

1. Ejecuta la corrida completa desde un directorio local (no de red). El canal de publicación usa TCP de loopback en lugar del socket Unix, y la CPU y la memoria del proceso se reportan como `NaN` porque esas lecturas solo existen en Unix.
2. Revisa `handles`: `failures` debe ser 0 en `worktree_remove`, `root_delete`, `root_rename`, `file_rename` y `file_delete`. Si alguno falla, repite con el editor y el antivirus cerrados para descartar otros procesos.
3. `gaps.fsevents_stream_restart` mide en Windows la recreación del watcher de `notify` al añadir y quitar rutas. El nombre del campo viene de macOS.

## Límites del prototipo

- **Estado del worktree**: lo calcula una implementación propia (índice leído con `gix`, comparación por stat y hash del contenido cuando el stat difiere, árbol de `HEAD` con `gix`), no `gix status`. Es una aproximación de lo que hará `crates/git` (ADR-GRP-009).
- **Ahead/behind**: usa `git rev-list --left-right --count` (en la allowlist de ADR-GRP-009 § 3), con el coste de lanzar un proceso. El producto puede usar `gix`.
- **Publicación**: suscriptor en el mismo proceso por un socket Unix. No hay un cliente en otro proceso ni la TUI, así que no mide `t_render`.
- **Monitorización simple**: el sondeo de respaldo, la reconciliación y la validación del enlace `gitdir` siguen ADR-GRP-010, pero no implementan la marca de hueco con inicio y fin, el tope de watches por repo ni el modo degradado.
