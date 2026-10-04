# SPIKE-TMC-001 — resultados (L)

- Hardware: Apple M5 · 10 núcleos · 32 GiB RAM · macOS 26.6.2 (25G83) · APFS · git 2.50.1 (Apple Git-155)
- Repo: L HEAD=589faa7a1122bfa35c8bc89deb48a34b37c99821 tracked_files=40001 wt_bytes=1090452372 commits=150000 pack_kb=1991818 gen_secs=141.3
- Iteraciones por escenario: 50 (1.000 archivos: 12)
- Carga de la máquina al empezar: { 3.73 4.34 4.75 }

## Micro-mediciones

- Lanzar `git --version`: p50 35.09 ms, p95 39.56 ms
- Lanzar `/Library/Developer/CommandLineTools/usr/bin/git --version`: p50 4.00 ms, p95 5.81 ms
- `git status --porcelain=v2 -uall` en el repo limpio: p50 87.5 ms, p95 94.1 ms
- SHA-1 de 20 MiB en el proceso: `sha1_smol` 18.8 ms · gitoxide (SHA-1 con detección de colisiones) 8.1 ms
- Fila de 200 B + `fsync` simple: p50 0.03 / p95 0.05 ms · con `F_FULLFSYNC`: p50 3.90 / p95 4.06 ms

## Snapshot previo, 1 worktree, almacén sembrado (clon APFS)

| variante | escenario | n | p95 total (ms) | p50 | máx | detección p95 | cola p95 | anclaje p95 | blobs p95 | árboles+commit p95 | ref+oplog p95 | MB nuevos (p50) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cli | primera captura (índice reflejado + árbol) | 1 | 1357.4 | | | | | | | | | |
| cli | sin cambios (camino rápido) | 50 | **104.2** | 96.9 | 106.1 | 99.0 | 0.0 | 0.0 | 0.0 | 0.0 | 5.2 | 0.0 |
| cli | delta 1 archivo | 50 | **225.9** | 209.7 | 234.9 | 103.8 | 0.0 | 11.4 | 17.4 | 80.0 | 16.2 | 0.0 |
| cli | delta 10 archivos | 50 | **279.5** | 269.7 | 282.6 | 104.8 | 0.0 | 10.1 | 56.5 | 98.9 | 16.6 | 0.1 |
| cli | delta 100 archivos | 50 | **841.5** | 807.2 | 863.1 | 130.0 | 0.0 | 10.8 | 460.2 | 245.9 | 20.0 | 0.6 |
| cli | delta 100 archivos / ~20 MB | 50 | **1730.4** | 1522.8 | 1792.6 | 155.4 | 0.0 | 68.9 | 1094.6 | 341.6 | 76.0 | 21.2 |
| cli | delta 1.000 archivos | 12 | **7628.9** | 5793.7 | 7628.9 | 126.6 | 0.0 | 24.3 | 6118.8 | 1300.6 | 106.2 | 7.2 |
| cli | (almacén tras el escenario: 1364 MiB, sin contar la siembra compartida) | | | | | | | | | | | |
| fi-hint | primera captura (índice reflejado + árbol) | 1 | 211.5 | | | | | | | | | |
| fi-hint | sin cambios (camino rápido) | 50 | **4.2** | 3.9 | 4.7 | 0.1 | 0.0 | 0.0 | 0.0 | 0.0 | 4.1 | 0.0 |
| fi-hint | delta 1 archivo | 50 | **24.8** | 21.8 | 26.8 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 24.7 | 0.0 |
| fi-hint | delta 10 archivos | 50 | **26.9** | 25.0 | 34.8 | 0.1 | 0.0 | 0.0 | 0.3 | 0.0 | 26.6 | 0.1 |
| fi-hint | delta 100 archivos | 50 | **49.9** | 46.9 | 52.1 | 0.3 | 0.0 | 0.0 | 2.2 | 0.0 | 47.7 | 0.6 |
| fi-hint | delta 100 archivos / ~20 MB | 50 | **439.8** | 427.4 | 447.3 | 0.4 | 0.0 | 0.0 | 331.1 | 0.1 | 111.1 | 21.2 |
| fi-hint | delta 1.000 archivos | 12 | **274.0** | 263.1 | 274.0 | 5.1 | 0.0 | 0.0 | 172.7 | 0.3 | 99.2 | 7.2 |
| fi-hint | (almacén tras el escenario: 1188 MiB, sin contar la siembra compartida) | | | | | | | | | | | |
| gix-hint | primera captura (índice reflejado + árbol) | 1 | 195.8 | | | | | | | | | |
| gix-hint | sin cambios (camino rápido) | 50 | **4.1** | 3.9 | 6.1 | 0.1 | 0.0 | 0.0 | 0.0 | 0.0 | 4.1 | 0.0 |
| gix-hint | delta 1 archivo | 50 | **7.0** | 6.3 | 8.9 | 0.1 | 0.0 | 0.0 | 1.0 | 2.1 | 4.6 | 0.0 |
| gix-hint | delta 10 archivos | 50 | **15.3** | 12.9 | 18.7 | 0.1 | 0.0 | 0.0 | 1.9 | 6.9 | 6.0 | 0.1 |
| gix-hint | delta 100 archivos | 50 | **72.0** | 62.2 | 82.4 | 0.6 | 0.0 | 0.0 | 16.6 | 47.2 | 17.4 | 0.6 |
| gix-hint | delta 100 archivos / ~20 MB | 50 | **126.7** | 119.0 | 141.4 | 0.7 | 0.0 | 0.0 | 80.0 | 34.8 | 18.0 | 21.2 |
| gix-hint | delta 1.000 archivos | 12 | **500.8** | 451.8 | 500.8 | 4.3 | 0.0 | 0.0 | 171.4 | 312.2 | 23.4 | 7.2 |
| gix-hint | (almacén tras el escenario: 1433 MiB, sin contar la siembra compartida) | | | | | | | | | | | |

## Repo intacto (NFR-01, garantía 4 de ADR-TMC-001)

- 18 ventanas medidas; en cada una se compara la huella de todo `.git` de todos los worktrees (ruta, tamaño, mtime, inodo y contenido salvo packs), `for-each-ref`, `log --all` y `stash list` antes y después.
- Resultado: **idéntico en todas las ventanas**
- Carga de la máquina al terminar: { 5.39 5.03 4.93 }
