# SPIKE-TMC-001 — resultados (P50)

- Hardware: Apple M5 · 10 núcleos · 32 GiB RAM · macOS 26.6.2 (25G83) · APFS · git 2.50.1 (Apple Git-155)
- Repo: P50 HEAD=2d6f9bc30be790d0f65493cf8af3706fd8e8830b tracked_files=6001 wt_bytes=53307053 commits=20000 pack_kb=160339 gen_secs=15.8
- Iteraciones por escenario: 100 (1.000 archivos: 25)
- Carga de la máquina al empezar: { 5.24 5.05 5.23 }

## Micro-mediciones

- Lanzar `git --version`: p50 13.96 ms, p95 16.90 ms
- Lanzar `/Library/Developer/CommandLineTools/usr/bin/git --version`: p50 3.87 ms, p95 5.25 ms
- `git status --porcelain=v2 -uall` en el repo limpio: p50 18.7 ms, p95 20.2 ms
- SHA-1 de 20 MiB en el proceso: `sha1_smol` 17.8 ms · gitoxide (SHA-1 con detección de colisiones) 8.1 ms
- Fila de 200 B + `fsync` simple: p50 0.03 / p95 0.06 ms · con `F_FULLFSYNC`: p50 3.91 / p95 4.08 ms

## Snapshot previo, 1 worktree, almacén sembrado (clon APFS)

| variante | escenario | n | p95 total (ms) | p50 | máx | detección p95 | cola p95 | anclaje p95 | blobs p95 | árboles+commit p95 | ref+oplog p95 | MB nuevos (p50) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cli | primera captura (índice reflejado + árbol) | 1 | 264.9 | | | | | | | | | |
| cli | sin cambios (camino rápido) | 100 | **31.1** | 28.0 | 33.3 | 26.9 | 0.0 | 0.0 | 0.0 | 0.0 | 4.4 | 0.0 |
| cli | delta 1 archivo | 100 | **118.6** | 98.0 | 154.4 | 33.7 | 0.0 | 7.9 | 16.6 | 47.5 | 16.2 | 0.0 |
| cli | delta 10 archivos | 100 | **194.8** | 163.2 | 225.3 | 30.6 | 0.0 | 7.8 | 66.1 | 77.2 | 17.0 | 0.1 |
| cli | delta 100 archivos | 100 | **711.9** | 641.2 | 813.6 | 30.1 | 0.0 | 7.6 | 499.5 | 175.5 | 16.4 | 0.8 |
| cli | delta 100 archivos / ~20 MB | 100 | **686.5** | 652.1 | 783.7 | 44.0 | 0.0 | 7.9 | 570.4 | 60.2 | 16.5 | 6.4 |
| cli | delta 1.000 archivos | 25 | **5223.9** | 4566.0 | 5313.4 | 44.0 | 0.0 | 7.3 | 4971.0 | 228.4 | 17.5 | 7.4 |
| cli | (almacén tras el escenario: 978 MiB, sin contar la siembra compartida) | | | | | | | | | | | |
| fi-hint | primera captura (índice reflejado + árbol) | 1 | 67.5 | | | | | | | | | |
| fi-hint | sin cambios (camino rápido) | 100 | **4.2** | 3.8 | 4.7 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 4.1 | 0.0 |
| fi-hint | delta 1 archivo | 100 | **23.1** | 20.9 | 25.6 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 23.0 | 0.0 |
| fi-hint | delta 10 archivos | 100 | **27.0** | 25.2 | 30.9 | 0.1 | 0.0 | 0.0 | 0.3 | 0.0 | 26.7 | 0.1 |
| fi-hint | delta 100 archivos | 100 | **52.8** | 46.8 | 58.8 | 0.4 | 0.0 | 0.0 | 2.8 | 0.0 | 50.1 | 0.8 |
| fi-hint | delta 100 archivos / ~20 MB | 100 | **209.1** | 205.2 | 216.3 | 0.5 | 0.0 | 0.0 | 142.3 | 0.1 | 67.8 | 6.4 |
| fi-hint | delta 1.000 archivos | 25 | **216.5** | 209.4 | 216.6 | 5.0 | 0.0 | 0.0 | 156.8 | 0.3 | 58.4 | 7.4 |
| fi-hint | (almacén tras el escenario: 780 MiB, sin contar la siembra compartida) | | | | | | | | | | | |
| gix-hint | primera captura (índice reflejado + árbol) | 1 | 57.5 | | | | | | | | | |
| gix-hint | sin cambios (camino rápido) | 100 | **4.1** | 3.8 | 4.5 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 4.1 | 0.0 |
| gix-hint | delta 1 archivo | 100 | **7.7** | 6.5 | 8.7 | 0.0 | 0.0 | 0.0 | 1.1 | 2.5 | 4.8 | 0.0 |
| gix-hint | delta 10 archivos | 100 | **14.8** | 13.0 | 15.7 | 0.1 | 0.0 | 0.0 | 2.0 | 7.5 | 6.1 | 0.1 |
| gix-hint | delta 100 archivos | 100 | **59.2** | 54.1 | 64.2 | 0.6 | 0.0 | 0.0 | 14.9 | 33.7 | 15.8 | 0.8 |
| gix-hint | delta 100 archivos / ~20 MB | 100 | **39.5** | 35.9 | 47.9 | 0.9 | 0.0 | 0.0 | 22.9 | 7.0 | 12.2 | 6.4 |
| gix-hint | delta 1.000 archivos | 25 | **248.1** | 220.5 | 254.1 | 4.6 | 0.0 | 0.0 | 168.5 | 64.0 | 22.0 | 7.4 |
| gix-hint | (almacén tras el escenario: 1038 MiB, sin contar la siembra compartida) | | | | | | | | | | | |

## Repo intacto (NFR-01, garantía 4 de ADR-TMC-001)

- 18 ventanas medidas; en cada una se compara la huella de todo `.git` de todos los worktrees (ruta, tamaño, mtime, inodo y contenido salvo packs), `for-each-ref`, `log --all` y `stash list` antes y después.
- Resultado: **idéntico en todas las ventanas**
- Carga de la máquina al terminar: { 6.21 4.76 4.94 }
