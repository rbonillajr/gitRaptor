# SPIKE-TMC-001 — resultados (M)

- Hardware: Apple M5 · 10 núcleos · 32 GiB RAM · macOS 26.6.2 (25G83) · APFS · git 2.50.1 (Apple Git-155)
- Repo: M HEAD=1a8bd2364b3bf0767bc3cc0a728bb1a7adbc8f71 tracked_files=10001 wt_bytes=316373715 commits=50000 pack_kb=642516 gen_secs=58.2
- Iteraciones por escenario: 100 (1.000 archivos: 25)
- Carga de la máquina al empezar: { 9.49 7.06 5.90 }

## Micro-mediciones

- Lanzar `git --version`: p50 22.86 ms, p95 28.75 ms
- Lanzar `/Library/Developer/CommandLineTools/usr/bin/git --version`: p50 6.06 ms, p95 8.88 ms
- `git status --porcelain=v2 -uall` en el repo limpio: p50 39.7 ms, p95 44.6 ms
- SHA-1 de 20 MiB en el proceso: `sha1_smol` 19.1 ms · gitoxide (SHA-1 con detección de colisiones) 10.7 ms
- Fila de 200 B + `fsync` simple: p50 0.03 / p95 0.08 ms · con `F_FULLFSYNC`: p50 3.97 / p95 6.00 ms

## Siembra del almacén

| modo | tiempo (s) | espacio libre consumido (MiB) | `du` del almacén (MiB) |
|---|---|---|---|
| enlace duro | 0.00 | -1 | 641 |
| clon APFS (`cp -c`) | 0.01 | -14 | 627 |
| copia de bytes | 0.38 | 628 | 627 |
- Siembra por enlace duro y luego `hash-object -w` en el almacén de un archivo que ya está en el pack: el `.git` del usuario **cambia** (1 entradas: el `mtime` del pack del usuario)
- Siembra por clon APFS y luego `hash-object -w` en el almacén de un archivo que ya está en el pack: el `.git` del usuario no cambia

## Snapshot previo, 1 worktree, almacén sembrado (clon APFS)

| variante | escenario | n | p95 total (ms) | p50 | máx | detección p95 | cola p95 | anclaje p95 | blobs p95 | árboles+commit p95 | ref+oplog p95 | MB nuevos (p50) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cli | primera captura (índice reflejado + árbol) | 1 | 453.9 | | | | | | | | | |
| cli | sin cambios (camino rápido) | 100 | **75.0** | 52.1 | 83.0 | 71.0 | 0.0 | 0.0 | 0.0 | 0.0 | 5.2 | 0.0 |
| cli | delta 1 archivo | 100 | **170.8** | 120.8 | 197.9 | 55.9 | 0.0 | 11.8 | 20.1 | 62.7 | 19.1 | 0.0 |
| cli | delta 10 archivos | 100 | **251.9** | 211.4 | 305.5 | 64.6 | 0.0 | 12.6 | 65.1 | 104.5 | 21.9 | 0.1 |
| cli | delta 100 archivos | 100 | **865.0** | 748.7 | 942.0 | 53.9 | 0.0 | 10.4 | 567.8 | 242.0 | 20.3 | 0.7 |
| cli | delta 100 archivos / ~20 MB | 100 | **1955.9** | 1553.7 | 2045.1 | 82.0 | 0.0 | 129.8 | 1185.3 | 424.5 | 126.8 | 20.6 |
| cli | delta 1.000 archivos | 25 | **7186.4** | 5396.5 | 7556.2 | 54.2 | 0.0 | 39.3 | 6190.8 | 897.9 | 160.0 | 7.1 |
| cli | archivo sin seguimiento de 50 MB | 3 | **1493.7** | 1423.5 | 1493.7 | 41.8 | 0.0 | 34.0 | 989.3 | 310.9 | 125.2 | 50.0 |
| cli | archivo sin seguimiento de 200 MB | 3 | **3764.6** | 3764.0 | 3764.6 | 39.7 | 0.0 | 35.8 | 3289.4 | 288.1 | 119.4 | 200.0 |
| cli | archivo sin seguimiento de 1 GB | 3 | **16807.7** | 16798.6 | 16807.7 | 54.9 | 0.0 | 32.1 | 16298.0 | 322.4 | 161.5 | 1024.0 |
| cli | (almacén tras el escenario: 6353 MiB, sin contar la siembra compartida) | | | | | | | | | | | |
| fi | primera captura (índice reflejado + árbol) | 1 | 144.9 | | | | | | | | | |
| fi | sin cambios (camino rápido) | 100 | **42.9** | 37.0 | 48.1 | 37.6 | 0.0 | 0.0 | 0.0 | 0.0 | 5.1 | 0.0 |
| fi | delta 1 archivo | 100 | **76.6** | 71.9 | 104.5 | 39.1 | 0.0 | 0.0 | 0.0 | 0.0 | 38.2 | 0.0 |
| fi | delta 10 archivos | 100 | **114.7** | 102.7 | 149.5 | 41.4 | 0.0 | 0.0 | 0.3 | 0.0 | 75.2 | 0.1 |
| fi | delta 100 archivos | 100 | **87.6** | 82.9 | 110.1 | 36.6 | 0.0 | 0.0 | 2.6 | 0.0 | 50.0 | 0.7 |
| fi | delta 100 archivos / ~20 MB | 100 | **563.8** | 494.6 | 611.5 | 80.1 | 0.0 | 0.0 | 382.1 | 0.1 | 109.1 | 20.6 |
| fi | delta 1.000 archivos | 25 | **285.6** | 272.1 | 286.0 | 48.4 | 0.0 | 0.0 | 166.4 | 0.3 | 83.5 | 7.1 |
| fi | archivo sin seguimiento de 50 MB | 3 | **1922.5** | 1913.6 | 1922.5 | 43.0 | 0.0 | 0.0 | 835.0 | 0.0 | 1060.3 | 50.0 |
| fi | archivo sin seguimiento de 200 MB | 3 | **7357.3** | 7315.0 | 7357.3 | 35.9 | 0.0 | 0.0 | 3336.4 | 0.0 | 3985.7 | 200.0 |
| fi | archivo sin seguimiento de 1 GB | 3 | **37824.2** | 37552.6 | 37824.2 | 49.8 | 0.0 | 0.0 | 17459.1 | 0.0 | 20315.3 | 1024.0 |
| fi | (almacén tras el escenario: 6099 MiB, sin contar la siembra compartida) | | | | | | | | | | | |
| fi-hint | primera captura (índice reflejado + árbol) | 1 | 110.8 | | | | | | | | | |
| fi-hint | sin cambios (camino rápido) | 100 | **4.1** | 4.0 | 4.9 | 0.1 | 0.0 | 0.0 | 0.0 | 0.0 | 4.1 | 0.0 |
| fi-hint | delta 1 archivo | 100 | **41.1** | 35.6 | 51.9 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 41.0 | 0.0 |
| fi-hint | delta 10 archivos | 100 | **78.2** | 66.0 | 100.2 | 0.1 | 0.0 | 0.0 | 0.3 | 0.0 | 77.9 | 0.1 |
| fi-hint | delta 100 archivos | 100 | **53.6** | 49.5 | 64.3 | 0.3 | 0.0 | 0.0 | 2.4 | 0.0 | 51.2 | 0.7 |
| fi-hint | delta 100 archivos / ~20 MB | 100 | **455.5** | 422.7 | 461.2 | 0.6 | 0.0 | 0.0 | 341.8 | 0.1 | 115.4 | 20.6 |
| fi-hint | delta 1.000 archivos | 25 | **243.5** | 232.5 | 353.6 | 4.8 | 0.0 | 0.0 | 167.1 | 0.4 | 83.7 | 7.1 |
| fi-hint | archivo sin seguimiento de 50 MB | 3 | **1936.7** | 1933.9 | 1936.7 | 2.0 | 0.0 | 0.0 | 884.9 | 0.1 | 1096.1 | 50.0 |
| fi-hint | archivo sin seguimiento de 200 MB | 3 | **7270.8** | 7252.1 | 7270.8 | 1.9 | 0.0 | 0.0 | 3293.9 | 0.2 | 3979.3 | 200.0 |
| fi-hint | archivo sin seguimiento de 1 GB | 3 | **38258.9** | 37368.5 | 38258.9 | 2.8 | 0.0 | 0.0 | 17379.7 | 0.1 | 20876.3 | 1024.0 |
| fi-hint | (almacén tras el escenario: 6100 MiB, sin contar la siembra compartida) | | | | | | | | | | | |
| gix-hint | primera captura (índice reflejado + árbol) | 1 | 86.6 | | | | | | | | | |
| gix-hint | sin cambios (camino rápido) | 100 | **4.1** | 3.1 | 4.8 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 4.1 | 0.0 |
| gix-hint | delta 1 archivo | 100 | **7.8** | 6.7 | 8.6 | 0.0 | 0.0 | 0.0 | 0.9 | 2.6 | 4.8 | 0.0 |
| gix-hint | delta 10 archivos | 100 | **16.2** | 13.8 | 18.8 | 0.1 | 0.0 | 0.0 | 2.4 | 8.6 | 6.4 | 0.1 |
| gix-hint | delta 100 archivos | 100 | **73.5** | 66.4 | 82.8 | 0.6 | 0.0 | 0.0 | 15.5 | 45.9 | 18.3 | 0.7 |
| gix-hint | delta 100 archivos / ~20 MB | 100 | **142.1** | 133.0 | 160.3 | 0.9 | 0.0 | 0.0 | 91.8 | 37.3 | 19.1 | 20.6 |
| gix-hint | delta 1.000 archivos | 25 | **366.8** | 332.9 | 390.2 | 4.7 | 0.0 | 0.0 | 206.2 | 176.7 | 20.6 | 7.1 |
| gix-hint | archivo sin seguimiento de 50 MB | 3 | **400.6** | 395.8 | 400.6 | 2.1 | 0.0 | 0.0 | 384.5 | 9.6 | 4.6 | 50.0 |
| gix-hint | archivo sin seguimiento de 200 MB | 3 | **1438.1** | 1419.4 | 1438.1 | 2.1 | 0.0 | 0.0 | 1421.4 | 10.1 | 5.6 | 200.0 |
| gix-hint | archivo sin seguimiento de 1 GB | 3 | **7091.7** | 7084.2 | 7091.7 | 2.0 | 0.0 | 0.0 | 7075.0 | 9.5 | 10.0 | 1024.0 |
| gix-hint | (almacén tras el escenario: 6741 MiB, sin contar la siembra compartida) | | | | | | | | | | | |

## 10 worktrees activos

Primer plano: snapshot previo de `w0` con un delta de 100 archivos. Fondo: 9 worktrees con capturas por observación (10 archivos cada `Q` = 1 s; `wt1` empieza con una ráfaga de 1.000 archivos). Un escritor por almacén.

| variante | escenario | n | p95 total (ms) | p50 | máx | detección p95 | cola p95 | anclaje p95 | blobs p95 | árboles+commit p95 | ref+oplog p95 | MB nuevos (p50) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cli | previo w0 (100 arch.) con 9 worktrees capturando | 100 | **1668.8** | 1492.1 | 1768.2 | 63.6 | 933.8 | 9.2 | 474.2 | 213.8 | 17.1 | 0.7 |
| cli | capturas por observación de fondo | 751 | **1649.0** | 964.5 | 5917.9 | 56.5 | 1448.7 | 8.9 | 61.5 | 104.0 | 17.1 | 0.1 |
| cli | previo con ámbito de 10 worktrees (10 arch. c/u) | 50 | **1341.3** | 1275.8 | 1417.2 | 179.0 | 0.0 | 8.3 | 543.8 | 623.3 | 16.9 | 0.9 |
| fi | previo w0 (100 arch.) con 9 worktrees capturando | 100 | **151.6** | 118.5 | 178.3 | 49.6 | 61.5 | 0.0 | 3.7 | 0.0 | 54.1 | 0.7 |
| fi | capturas por observación de fondo | 243 | **206.2** | 129.0 | 827.7 | 50.4 | 73.0 | 0.0 | 0.5 | 0.1 | 93.6 | 0.1 |
| fi | previo con ámbito de 10 worktrees (10 arch. c/u) | 50 | **221.2** | 211.3 | 227.4 | 166.9 | 0.0 | 0.0 | 2.7 | 0.0 | 54.6 | 0.9 |
| fi-hint | previo w0 (100 arch.) con 9 worktrees capturando | 100 | **125.5** | 57.3 | 221.2 | 0.8 | 74.3 | 0.0 | 3.1 | 0.0 | 53.8 | 0.7 |
| fi-hint | capturas por observación de fondo | 216 | **192.4** | 93.5 | 710.2 | 2.0 | 117.1 | 0.0 | 0.5 | 0.1 | 88.3 | 0.1 |
| fi-hint | previo con ámbito de 10 worktrees (10 arch. c/u) | 50 | **72.5** | 63.9 | 82.6 | 2.6 | 0.0 | 0.0 | 29.1 | 0.2 | 59.9 | 0.9 |
| gix-hint | previo w0 (100 arch.) con 9 worktrees capturando | 100 | **79.8** | 64.4 | 105.2 | 0.7 | 6.9 | 0.0 | 22.1 | 41.0 | 19.2 | 0.7 |
| gix-hint | capturas por observación de fondo | 217 | **70.9** | 22.8 | 413.9 | 1.9 | 43.7 | 0.0 | 12.5 | 18.4 | 9.4 | 0.1 |
| gix-hint | previo con ámbito de 10 worktrees (10 arch. c/u) | 50 | **125.2** | 115.1 | 150.4 | 3.5 | 0.0 | 0.0 | 15.5 | 100.6 | 28.6 | 0.9 |

- `cli`: 751 capturas de fondo en 183 s; CPU total del proceso y sus hijos 151.3 s = **83 % de un núcleo** en promedio; almacén +263 MiB
- `fi`: 243 capturas de fondo en 32 s; CPU total del proceso y sus hijos 29.7 s = **92 % de un núcleo** en promedio; almacén +85 MiB
- `fi-hint`: 216 capturas de fondo en 28 s; CPU total del proceso y sus hijos 2.1 s = **8 % de un núcleo** en promedio; almacén +80 MiB
- `gix-hint`: 217 capturas de fondo en 26 s; CPU total del proceso y sus hijos 13.5 s = **52 % de un núcleo** en promedio; almacén +173 MiB

## Semana simulada (2400 capturas de 5 archivos editados)

- `fi`: 306 s; contenido nuevo 82 MiB; almacén +273 MiB (2 archivos en `objects/pack`); `repack -d --geometric=2` 16.4 s → +48 MiB
- `gix-hint`: 100 s; contenido nuevo 82 MiB; almacén +280 MiB (2 archivos en `objects/pack`); `repack -d --geometric=2` 15.8 s → +47 MiB

## Repo intacto (NFR-01, garantía 4 de ADR-TMC-001)

- 34 ventanas medidas; en cada una se compara la huella de todo `.git` de todos los worktrees (ruta, tamaño, mtime, inodo y contenido salvo packs), `for-each-ref`, `log --all` y `stash list` antes y después.
- Resultado: **idéntico en todas las ventanas**
- Carga de la máquina al terminar: { 6.38 5.75 5.55 }
