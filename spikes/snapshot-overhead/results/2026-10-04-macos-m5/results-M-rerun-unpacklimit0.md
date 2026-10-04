# SPIKE-TMC-001 — resultados (M)

- Hardware: Apple M5 · 10 núcleos · 32 GiB RAM · macOS 26.6.2 (25G83) · APFS · git 2.50.1 (Apple Git-155)
- Repo: M HEAD=1a8bd2364b3bf0767bc3cc0a728bb1a7adbc8f71 tracked_files=10001 wt_bytes=316373715 commits=50000 pack_kb=642516 gen_secs=58.2
- Iteraciones por escenario: 100 (1.000 archivos: 25)
- Carga de la máquina al empezar: { 4.22 4.97 5.25 }

## Micro-mediciones

- Lanzar `git --version`: p50 12.51 ms, p95 14.25 ms
- Lanzar `/Library/Developer/CommandLineTools/usr/bin/git --version`: p50 3.88 ms, p95 5.11 ms
- `git status --porcelain=v2 -uall` en el repo limpio: p50 26.7 ms, p95 28.6 ms
- SHA-1 de 20 MiB en el proceso: `sha1_smol` 17.8 ms · gitoxide (SHA-1 con detección de colisiones) 8.1 ms
- Fila de 200 B + `fsync` simple: p50 0.03 / p95 0.06 ms · con `F_FULLFSYNC`: p50 3.92 / p95 6.00 ms

## Snapshot previo, 1 worktree, almacén sembrado (clon APFS)

| variante | escenario | n | p95 total (ms) | p50 | máx | detección p95 | cola p95 | anclaje p95 | blobs p95 | árboles+commit p95 | ref+oplog p95 | MB nuevos (p50) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| fi-hint | primera captura (índice reflejado + árbol) | 1 | 144.4 | | | | | | | | | |
| fi-hint | sin cambios (camino rápido) | 100 | **4.0** | 3.1 | 4.2 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 4.0 | 0.0 |
| fi-hint | delta 1 archivo | 100 | **24.6** | 21.0 | 34.8 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 24.5 | 0.0 |
| fi-hint | delta 10 archivos | 100 | **27.8** | 25.1 | 33.0 | 0.1 | 0.0 | 0.0 | 0.3 | 0.0 | 27.5 | 0.1 |
| fi-hint | delta 100 archivos | 100 | **55.4** | 51.3 | 61.7 | 0.3 | 0.0 | 0.0 | 2.6 | 0.0 | 52.9 | 0.7 |
| fi-hint | delta 100 archivos / ~20 MB | 100 | **468.4** | 419.1 | 495.0 | 0.5 | 0.0 | 0.0 | 359.7 | 0.1 | 110.1 | 20.6 |
| fi-hint | delta 1.000 archivos | 25 | **256.6** | 236.8 | 277.6 | 4.9 | 0.0 | 0.0 | 173.5 | 0.3 | 90.4 | 7.1 |
| fi-hint | (almacén tras el escenario: 2217 MiB, sin contar la siembra compartida) | | | | | | | | | | | |
| gix-hint | primera captura (índice reflejado + árbol) | 1 | 83.9 | | | | | | | | | |
| gix-hint | sin cambios (camino rápido) | 100 | **4.1** | 3.9 | 7.3 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 4.0 | 0.0 |
| gix-hint | delta 1 archivo | 100 | **7.1** | 5.9 | 8.2 | 0.0 | 0.0 | 0.0 | 1.0 | 2.1 | 4.7 | 0.0 |
| gix-hint | delta 10 archivos | 100 | **15.9** | 14.3 | 18.8 | 0.1 | 0.0 | 0.0 | 1.9 | 6.9 | 7.8 | 0.1 |
| gix-hint | delta 100 archivos | 100 | **73.5** | 64.8 | 91.0 | 0.6 | 0.0 | 0.0 | 14.4 | 41.6 | 20.9 | 0.7 |
| gix-hint | delta 100 archivos / ~20 MB | 100 | **146.1** | 131.8 | 164.3 | 0.7 | 0.0 | 0.0 | 83.9 | 35.0 | 29.8 | 20.6 |
| gix-hint | delta 1.000 archivos | 25 | **356.5** | 335.6 | 375.6 | 4.7 | 0.0 | 0.0 | 212.1 | 142.7 | 24.8 | 7.1 |
| gix-hint | (almacén tras el escenario: 2635 MiB, sin contar la siembra compartida) | | | | | | | | | | | |

## Repo intacto (NFR-01, garantía 4 de ADR-TMC-001)

- 12 ventanas medidas; en cada una se compara la huella de todo `.git` de todos los worktrees (ruta, tamaño, mtime, inodo y contenido salvo packs), `for-each-ref`, `log --all` y `stash list` antes y después.
- Resultado: **idéntico en todas las ventanas**
- Carga de la máquina al terminar: { 5.86 5.16 5.27 }
