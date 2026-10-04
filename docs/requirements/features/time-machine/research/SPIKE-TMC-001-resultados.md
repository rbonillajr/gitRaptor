---
id: SPIKE-TMC-001-RESULTADOS
title: "SPIKE-TMC-001 — Resultados: repo mediano de referencia y viabilidad del snapshot en menos de 200 ms"
type: research-brief
status: done
feature: time-machine
domain: GRP
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-TMC-006, ADR-TMC-001, ADR-TMC-004, ADR-GRP-001, ADR-GRP-011]
  stories: [SPIKE-TMC-001, US-TMC-001, US-TMC-004, US-TMC-020, INF-GRP-002]
  nfrs: [NFR-04, NFR-01, NFR-TMC-11, SEC-TMC-12]
tags: [time-machine, spike, rendimiento, nfr-04, d-tmc-21, repo-mediano, almacen, resultados]
---

# SPIKE-TMC-001 — Resultados

**Pregunta** ([SPIKE-TMC-001](../technical-stories/SPIKE-TMC-001-repo-mediano-overhead.md)): ¿qué es un "repo mediano" y, en ese repo, cumple un snapshot previo un p95 menor de 200 ms escrito en un almacén privado del perfil (ADR-TMC-001), con un coste de disco y de siembra aceptable?

## Veredicto

**Viable en macOS, pero solo con los escalones 2 y 3 de ADR-TMC-006 § 5 activados juntos.** El escalón 2 es la detección de cambios con el estado del motor y una caché de stat; el escalón 3 es escribir el almacén con gitoxide, con los blobs en paralelo. En el repo de referencia (perfil `M`, D-TMC-21), con el almacén sembrado y el delta de referencia (100 archivos, unos 20 MB nuevos), el **p95 es de 142–146 ms** en dos pasadas (objetivo: < 200 ms). Con 100 archivos de texto baja a 74 ms y el camino rápido cuesta 4 ms. El repo mayor `L` también cumple (127 ms), porque con este diseño el coste depende del delta y no del tamaño del repo (§ 5.7).

- La **línea base** (Git CLI con un proceso por paso, ADR-GRP-001 tal cual) **no cumple**: 171 ms con 1 archivo, 865 ms con 100 y 1.956 ms con el delta de referencia.
- El **escalón 1** (procesos persistentes con `git fast-import`) cumple con deltas pequeños, pero **no con el delta de referencia** (456 a 564 ms): un solo proceso de `fast-import` no pasa de unos 45 MB/s.
- Por tanto, se cumple la **vía de fracaso** prevista en el SPIKE: "si solo cumple el tercero, se activa ese escalón (preaprobado, TQ-4 → a)". El escalón 2 también hace falta: con `git status` como detección, el camino rápido cuesta 75 ms y la detección se come la mitad del presupuesto.
- **La siembra por enlace duro rompe la garantía 4 de ADR-TMC-001**: cuando el almacén escribe un objeto que ya está en el pack enlazado, Git cambia el `mtime` del pack *del usuario*. El clon APFS (`clonefile`) cuesta lo mismo, 0,01 s y casi nada de disco, y no tiene ese efecto. Ver § 5.4.
- **Repo mediano (D-TMC-21), aprobado por Rene Bonilla el 2026-10-04**: es el perfil `M` del generador: 10.000 archivos, 316 MB de working tree, 50.000 commits y 627 MiB de historial. Queda en torno al p70–p90 de una muestra de 18 repos públicos. `L` queda fuera de referencia.
- **Linux y Windows: sin verificar.** El procedimiento está en § 8.

**Reproducir** (un comando, unos 45 min en la máquina de referencia):

```sh
spikes/snapshot-overhead/run.sh          # perfil M, banco completo
```

Informes crudos de cada pasada y de la muestra pública: [`spikes/snapshot-overhead/results/2026-10-04-macos-m5/`](../../../../../spikes/snapshot-overhead/results/2026-10-04-macos-m5/).

## 1. Alcance y lo que no se verificó

| Experimento del SPIKE | Estado |
|---|---|
| Corpus de repos públicos y repo de referencia | Hecho, con una muestra de 18 repos (no es un estudio de percentiles representativo, § 3) |
| p95 por etapa con deltas de 1, 10, 100 y 1.000 archivos | Hecho en macOS (máquina de dogfooding) |
| Un proceso por paso frente a procesos persistentes; ganancia de gitoxide | Hecho: cuatro variantes (§ 2.2) |
| Siembra y disco: enlace duro frente a copia; crecimiento en una semana simulada | Hecho, más el clon APFS |
| Archivo de 1 GB sin seguimiento | Hecho (50 MB, 200 MB y 1 GB) |
| Archivos LFS reales | **No medido** con `git-lfs`. Para el almacén, un archivo LFS ya "smudged" es un archivo grande más (ADR-TMC-001 § 2): su coste es el de la fila de archivos grandes |
| Captura continua: CPU, disco y p95 **del motor** con 10 worktrees | Parcial: CPU, disco y latencia del snapshot con 10 worktrees capturando sí. El p95 del motor **no**, porque el motor (TS-GRP-002/003) aún no existe |
| Admisión y canal hasta el componente Time Machine | **No medido** (no hay daemon). Se mantiene su presupuesto de 10 ms |
| Windows y Linux | **No verificados**. Procedimiento en § 8 |
| Repo intacto | Hecho: guarda automática en cada ventana medida (§ 5.6) |

## 2. Método

### 2.1 Máquina y condiciones

- **Hardware**: Apple M5, 10 núcleos, 32 GiB de RAM, SSD interno con APFS, macOS 26.6.2 (25G83), Git 2.50.1 (Apple Git-155). Es la máquina de Rene, que ADR-GRP-011 § 4 usa como máquina de dogfooding.
- **Carga**: la máquina **no estaba en reposo**. Otros agentes de Orca trabajaban en paralelo (entre ellos el banco de SPIKE-GRP-002) y la carga media osciló entre 4 y 9,5. Las cifras son, por tanto, conservadoras. Además se repitieron las variantes clave en otra pasada (§ 5.1), con diferencias de pocos milisegundos.
- **Entorno de Git limpio**: sin configuración de sistema ni global (`GIT_CONFIG_NOSYSTEM`, `GIT_CONFIG_GLOBAL=/dev/null`), igual que exige NFR-TMC-12 a las escrituras internas.
- **Binario de Git**: en macOS, `/usr/bin/git` es un *shim* de `xcrun` que cuesta **23 ms por proceso** (p50). El binario real cuesta 6 ms. El prototipo resuelve el real una vez (`xcrun -f git`) y el producto debe hacer lo mismo (enmienda E6).
- **Repos temporales**: todo vive en un directorio de trabajo fuera de cualquier repo. El binario se niega a arrancar dentro de uno. Nada se escribió en este repo (NFR-01).

### 2.2 Prototipo

[`spikes/snapshot-overhead/`](../../../../../spikes/snapshot-overhead/) es un workspace de Cargo independiente, fuera de `crates/` y de `apps/`. Implementa el almacén de ADR-TMC-001:

- Repo bare en `<perfil>/tm/<id>/store.git`, carpeta 0700, sin remotos, sin hooks (`core.hooksPath` vacío), `gc.auto=0`, sin reflogs, `core.fsync=committed`.
- Cada snapshot es un commit del almacén con `wt/<k>/files` (contenido bruto: `--no-filters` o lectura directa), `wt/<k>/index` (espejo del índice del usuario, construido con un índice temporal **del perfil**) y `meta` (HEAD, ramas, stash). Sus padres son HEAD, ramas y stash, y tiene la ref `refs/tm/snap/<id>`.
- Una fila de oplog con barrera de durabilidad (`F_FULLFSYNC`) sustituye a la fila SQLite de ADR-TMC-003.
- **Árbol de archivos = árbol del índice del usuario + lo sucio**: los archivos limpios reutilizan el blob del índice y solo se leen y escriben las rutas cambiadas. Una caché de stat evita volver a hashear lo que no cambió desde la última captura.

Cuatro variantes siguen los escalones de ADR-TMC-006 § 5:

| Variante | Escalón | Detección | Escritura del almacén |
|---|---|---|---|
| `cli` | línea base | `git status --porcelain=v2 -uall` (sin refrescar el índice, `GIT_OPTIONAL_LOCKS=0`) + `for-each-ref` | `hash-object --stdin-paths`, `update-index --index-info` + `write-tree` sobre un índice del perfil, `commit-tree`, `update-ref`: unos 8 procesos por snapshot |
| `fi` | 1 | igual que `cli` | `git fast-import` persistente (blobs, árboles y commit en un pack por snapshot), más `cat-file --batch-check` y `update-ref --stdin` persistentes |
| `fi-hint` | 1 + 2 | rutas que entrega el motor + caché de stat | igual que `fi` |
| `gix-hint` | 2 + 3 | igual que `fi-hint` | gitoxide en el proceso: blobs en paralelo (8 hilos), `fsync` simple por objeto y una barrera `F_FULLFSYNC` final (el esquema de `core.fsyncMethod=batch`), árboles con el editor de gitoxide y la ref |

### 2.3 Escenarios

- **Repo**: el perfil `M` del generador determinista (§ 3). La misma semilla da los mismos commits (`HEAD=1a8bd2364b3bf0767bc3cc0a728bb1a7adbc8f71`). Se genera en 58 s.
- **Almacén sembrado** (por clon APFS, § 5.4) y una captura inicial. Después, por escenario, el banco edita N archivos ("el agente") y mide un snapshot previo. Hay 100 iteraciones tras 3 de calentamiento; 25 en el escenario de 1.000 archivos y 3 en el de archivos grandes.
- **Deltas**: 0 (camino rápido), 1, 10, 100 y 1.000 archivos de texto; 100 archivos con unos 20 MB nuevos (assets binarios reescritos, el delta de referencia de ADR-TMC-006 § 2); y un archivo sin seguimiento de 50 MB, 200 MB y 1 GB.
- **10 worktrees**: 9 worktrees hacen capturas por observación (10 archivos editados y una captura por segundo cada uno, **5 veces más** que el peor caso con `M` = 5 s; `wt1` empieza con una ráfaga de 1.000 archivos) mientras se mide el snapshot previo de `w0` con 100 archivos. Hay un escritor por almacén (ADR-TMC-004 § 2). Además se mide un snapshot previo cuyo ámbito son los 10 worktrees.
- **Semana simulada**: 2.400 capturas por observación de 5 archivos editados, y el mantenimiento con `git repack -d --geometric=2`.
- **Etapas**: las de ADR-TMC-006 § 2, cronometradas con reloj monótono. "Cola" es la espera del escritor del almacén. "Árboles + commit" incluye el commit del almacén.

## 3. Repo mediano de referencia (D-TMC-21)

### 3.1 Muestra pública

18 repos públicos medidos con la API de GitHub (solo lectura) el 2026-10-04: archivos y bytes del árbol de la rama por defecto, commits de esa rama y tamaño del repo que informa GitHub (aproximación al historial empaquetado).

| Repo | Archivos | Working tree (MB) | Commits | Historial (MiB) |
|---|---|---|---|---|
| sindresorhus/got | 132 | 2,2 | 1.742 | 4 |
| expressjs/express | 214 | 0,7 | 6.173 | 10 |
| pallets/flask | 236 | 1,9 | 5.557 | 12 |
| BurntSushi/ripgrep | 237 | 3,3 | 2.287 | 6 |
| tokio-rs/axum | 505 | 2,0 | 2.012 | 6 |
| vitejs/vite | 2.841 | 17,6 | 9.734 | 74 |
| rust-lang/cargo | 3.073 | 18,0 | 23.358 | 73 |
| fastapi/fastapi | 3.181 | 34,9 | 7.776 | 55 |
| rails/rails | 5.018 | 39,9 | 99.854 | 284 |
| django/django | 7.084 | 46,1 | 34.969 | 276 |
| facebook/react | 7.252 | 40,7 | 21.710 | 1.075 |
| prisma/prisma | 8.629 | 77,3 | 8.351 | 267 |
| astral-sh/ruff | 11.194 | 91,0 | 17.480 | 204 |
| denoland/deno | 14.922 | 93,7 | 17.377 | 247 |
| microsoft/vscode | 19.952 | 286,7 | 166.904 | 1.490 |
| kubernetes/kubernetes | 31.356 | 279,6 | 141.765 | 1.493 |
| vercel/next.js | 33.918 | 158,6 | 36.054 | 2.521 |
| microsoft/TypeScript | ≥ 51.359 (árbol truncado) | ≥ 184,5 | 39.503 | 2.822 |
| **p50 / p75 / p90** | **6.051 / 13.990 / 32.125** | **40 / 93 / 213** | **17.428 / 35.783 / 112.427** | **225 / 877 / 1.802** |

Es una muestra de conveniencia de repos conocidos, no un estudio representativo: sirve para situar la referencia, no para fijar percentiles de la población.

### 3.2 Decisión: perfil `M` (aprobado por Rene Bonilla, 2026-10-04)

| Perfil | Archivos | Working tree | Commits | Historial | Posición en la muestra | Uso |
|---|---|---|---|---|---|---|
| **`M` (referencia, D-TMC-21)** | **10.001** | **316 MB** (3 % binarios de ~0,8 MB; 3.000 archivos ignorados) | **50.000** | **627 MiB** | ~p65 archivos, >p90 working tree, ~p80 commits, ~p70 historial | Gate de NFR-04 (US-TMC-020) |
| `P50` (alternativa) | 6.000 | 50 MB | 20.000 | ver § 5.7 | ~p50 en todo | Comparación |
| `L` (fuera de referencia) | 40.000 | 1 GB | 150.000 | ver § 5.7 | >p90 | US-TMC-020, escenario 3 (tarda más, no se salta) |

**Decisión**: Rene Bonilla aprobó `M` como referencia el 2026-10-04 (registrado en D-TMC-21 del [context](../context.md)). Motivos de la recomendación: es conservador (un gate que pasa en `M` pasa en la mayoría de los repos de la muestra) y coincide con la hipótesis de ADR-TMC-006 § 3. Además, el generador lo reproduce en cualquier máquina y en CI en un minuto, sin descargar nada. Con `P50` el gate sería más fácil de cumplir y protegería menos (§ 5.7).

## 4. Micro-mediciones (macOS)

| Medida | p50 | p95 |
|---|---|---|
| Lanzar `git` vía `/usr/bin/git` (shim de `xcrun`) | 22,9 ms | 28,8 ms |
| Lanzar el binario real (`xcrun -f git`) | 6,1 ms | 8,9 ms |
| `git status --porcelain=v2 -uall` en `M` limpio, sin poder refrescar el índice | 39,7 ms | 44,6 ms |
| Fila de 200 B + `fsync` simple | 0,03 ms | 0,08 ms |
| Fila de 200 B + `F_FULLFSYNC` (durabilidad real en macOS) | 3,97 ms | 6,00 ms |
| SHA-1 de 20 MiB en el proceso (gitoxide, con detección de colisiones) | 10,7 ms | — |

Consecuencias directas:

- **Cada objeto suelto que Git escribe con `core.fsync` paga un `F_FULLFSYNC` (unos 4 ms)**. `core.fsyncMethod=batch` solo se aplica a `add`, `update-index --add`, `stash` y `unpack-objects`, no a `hash-object` ni a `write-tree`. Con la línea base, 100 blobs cuestan unos 570 ms solo por eso.
- **`fast-import` desempaqueta en objetos sueltos** los packs de menos de 100 objetos (`fastimport.unpackLimit`), así que cada snapshot pagaba esos `fsync`. Con `fastimport.unpackLimit=0` el coste fijo baja de unos 40 ms a unos 21 ms (§ 5.1), a cambio de un pack por snapshot que el mantenimiento consolida.

## 5. Resultados (perfil `M`, macOS)

### 5.1 Snapshot previo, 1 worktree, almacén sembrado

p95 en ms (n = 100; 25 con 1.000 archivos). En negrita, lo que supera su presupuesto de ADR-TMC-006 § 2: 200 ms el total (50 ms en el camino rápido) y el límite de cada etapa en la cabecera.

| Variante | Delta | **p95 total** | p50 | Detección (≤ 50) | Anclaje (≤ 10) | Blobs (≤ 60) | Árboles + commit (≤ 30) | Ref + oplog (≤ 20) |
|---|---|---|---|---|---|---|---|---|
| `cli` | 0 (camino rápido) | **75,0** | 52,1 | **71,0** | 0 | 0 | 0 | 5,2 |
| `cli` | 1 | 170,8 | 120,8 | **55,9** | **11,8** | 20,1 | **62,7** | 19,1 |
| `cli` | 10 | **251,9** | 211,4 | **64,6** | **12,6** | **65,1** | **104,5** | **21,9** |
| `cli` | 100 | **865,0** | 748,7 | **53,9** | **10,4** | **567,8** | **242,0** | **20,3** |
| `cli` | 100 / 20 MB | **1.955,9** | 1.553,7 | **82,0** | **129,8** | **1.185,3** | **424,5** | **126,8** |
| `cli` | 1.000 | **7.186,4** | 5.396,5 | **54,2** | **39,3** | **6.190,8** | **897,9** | **160,0** |
| `fi` | 0 | 42,9 | 37,0 | 37,6 | 0 | 0 | 0 | 5,1 |
| `fi` | 1 | 76,6 | 71,9 | 39,1 | 0 | 0 | 0 | **38,2** |
| `fi` | 10 | 114,7 | 102,7 | 41,4 | 0 | 0,3 | 0 | **75,2** |
| `fi` | 100 | 87,6 | 82,9 | 36,6 | 0 | 2,6 | 0 | **50,0** |
| `fi` | 100 / 20 MB | **563,8** | 494,6 | **80,1** | 0 | **382,1** | 0,1 | **109,1** |
| `fi` | 1.000 | **285,6** | 272,1 | 48,4 | 0 | **166,4** | 0,3 | **83,5** |
| `fi-hint` | 0 | 4,1 | 4,0 | 0,1 | 0 | 0 | 0 | 4,1 |
| `fi-hint` | 1 | 41,1 | 35,6 | 0,0 | 0 | 0 | 0 | **41,0** |
| `fi-hint` | 10 | 78,2 | 66,0 | 0,1 | 0 | 0,3 | 0 | **77,9** |
| `fi-hint` | 100 | 53,6 | 49,5 | 0,3 | 0 | 2,4 | 0 | **51,2** |
| `fi-hint` | 100 / 20 MB | **455,5** | 422,7 | 0,6 | 0 | **341,8** | 0,1 | **115,4** |
| `fi-hint` | 1.000 | **243,5** | 232,5 | 4,8 | 0 | **167,1** | 0,4 | **83,7** |
| **`gix-hint`** | 0 | 4,1 | 3,1 | 0,0 | 0 | 0 | 0 | 4,1 |
| **`gix-hint`** | 1 | 7,8 | 6,7 | 0,0 | 0 | 0,9 | 2,6 | 4,8 |
| **`gix-hint`** | 10 | 16,2 | 13,8 | 0,1 | 0 | 2,4 | 8,6 | 6,4 |
| **`gix-hint`** | 100 | 73,5 | 66,4 | 0,6 | 0 | 15,5 | **45,9** | 18,3 |
| **`gix-hint`** | 100 / 20 MB | 142,1 ✅ | 133,0 | 0,9 | 0 | **91,8** | **37,3** | 19,1 |
| `gix-hint` | 1.000 | **366,8** | 332,9 | 4,7 | 0 | **206,2** | **176,7** | **20,6** |

En `fi` y `fi-hint` el reparto entre etapas es aproximado: `fast-import` procesa los árboles de forma asíncrona y su trabajo aparece en "ref + oplog" (el `checkpoint`). El total sí es exacto. En las variantes `*-hint`, "detección" solo mide el stat de las rutas que entrega el motor, no lo que cueste al motor saberlas (ver E2).

**Repetición** con `fastimport.unpackLimit=0` (otra pasada, carga media de 4–5):

| Variante | Delta | **p95 total** | p50 | Detección | Blobs | Árboles + commit | Ref + oplog |
|---|---|---|---|---|---|---|---|
| `fi-hint` | 0 / 1 / 10 | 4,0 / 24,6 / 27,8 | 3,1 / 21,0 / 25,1 | ≤ 0,1 | ≤ 0,3 | 0 | 4,0 / 24,5 / 27,5 |
| `fi-hint` | 100 | 55,4 | 51,3 | 0,3 | 2,6 | 0 | **52,9** |
| `fi-hint` | 100 / 20 MB | **468,4** | 419,1 | 0,5 | **359,7** | 0,1 | **110,1** |
| `fi-hint` | 1.000 | **256,6** | 236,8 | 4,9 | **173,5** | 0,3 | **90,4** |
| `gix-hint` | 0 / 1 / 10 | 4,1 / 7,1 / 15,9 | 3,9 / 5,9 / 14,3 | ≤ 0,1 | ≤ 1,9 | ≤ 6,9 | 4,0 / 4,7 / 7,8 |
| `gix-hint` | 100 | 73,5 | 64,8 | 0,6 | 14,4 | **41,6** | **20,9** |
| `gix-hint` | 100 / 20 MB | 146,1 ✅ | 131,8 | 0,7 | **83,9** | **35,0** | **29,8** |
| `gix-hint` | 1.000 | **356,5** | 335,6 | 4,7 | **212,1** | **142,7** | **24,8** |

La segunda pasada confirma la primera: `gix-hint` cumple el delta de referencia con unos 55 ms de margen (142 y 146 ms). `fast-import` sin desempaquetar abarata los deltas pequeños (de 41 a 25 ms con 1 archivo), pero no resuelve el de 20 MB. Ref + oplog de `gix-hint` llegó a 30 ms en esta pasada, por encima de sus 20 ms: son tres barreras de durabilidad (objetos, ref y fila), unos 4 ms cada una sin carga.

### 5.2 Archivos grandes sin seguimiento (snapshot previo garantizado)

p95 de 3 ejecuciones, en ms. El previo los incluye siempre (US-TMC-020, escenario 3).

| Tamaño | `cli` | `fi` / `fi-hint` | `gix-hint` |
|---|---|---|---|
| 50 MB | 1.493,7 | 1.922,5 / 1.936,7 | **400,6** |
| 200 MB | 3.764,6 | 7.357,3 / 7.270,8 | **1.438,1** |
| 1 GB | 16.807,7 | 37.824,2 / 38.258,9 | **7.091,7** |

Con gitoxide el coste es lineal, de unos 145 MB/s para un solo archivo (un hilo, zlib nivel 1 y SHA-1). Un archivo de 1 GB retrasa la operación unos 7 s: lo admite el escenario 3 de US-TMC-020, pero la CLI y la TUI deberían mostrar progreso.

### 5.3 Diez worktrees activos

p95 en ms. El fondo hace unas 9 capturas/s (5 veces más que `M` = 5 s).

| Variante | Previo de `w0` (100 archivos) con 9 worktrees capturando | … de ello, cola p95 | Capturas de fondo p95 (p50) | Previo con ámbito de 10 worktrees (10 archivos c/u) | CPU media (proceso + hijos) |
|---|---|---|---|---|---|
| `cli` | **1.668,8** | 933,8 | 1.649,0 (964,5) | **1.341,3** (detección 179) | 83 % de un núcleo |
| `fi` | 151,6 | 61,5 | 206,2 (129,0) | **221,2** (detección 167) | 92 % |
| `fi-hint` | 125,5 | 74,3 | 192,4 (93,5) | 72,5 | 8 % |
| **`gix-hint`** | **79,8** | 6,9 | 70,9 (22,8) | 125,2 | 52 % |

- **El escritor único por almacén hace esperar al snapshot previo detrás de las capturas por observación**. Con `cli` la cola llega a 934 ms. Con `gix-hint` son 7 ms, pero el riesgo de diseño sigue ahí: hace falta prioridad para el previo garantizado (enmienda E8).
- Un ámbito de 10 worktrees con `git status` cuesta 167–179 ms solo en detección, aun ejecutando los 10 `status` en paralelo. Con el escalón 2 baja a 3,5 ms.
- La CPU incluye el trabajo del propio banco (editar archivos y el primer plano), así que es una cota superior. Para `gix-hint`, una captura de fondo cuesta una mediana de 23 ms. Con `M` = 5 s y 10 worktrees en actividad continua (2 capturas/s) eso es **un ~5 % de un núcleo**.

### 5.4 Siembra y disco

| Modo | Tiempo | Espacio libre consumido | ¿Cambia el `.git` del usuario? |
|---|---|---|---|
| Enlace duro a los packs | 0,00 s | ~0 | **Sí**: al escribir en el almacén un objeto que ya está en el pack, Git lo "refresca" (`freshen_packed_object`) cambiando el `mtime` del pack, que es el mismo inodo que el del usuario |
| **Clon APFS (`cp -c`, `clonefile`)** | 0,01 s | ~0 (copia en escritura) | No |
| Copia de bytes | 0,38 s | 628 MiB (= historial) | No |

El banco lo demuestra con una prueba dedicada y lo detectó también por su cuenta en la primera pasada (en las ventanas con varios worktrees). Además del `mtime`, un enlace duro impide el 0600 de ADR-TMC-001 § 4: hacer `chmod` al archivo del almacén se lo haría también al del usuario. En la copia, 628 MiB en 0,38 s corresponde a un SSD interno; en un disco más lento o en otro volumen escalará con el historial.

### 5.5 Crecimiento del almacén (semana simulada)

2.400 capturas por observación de 5 archivos de texto editados, con 82 MiB de contenido nuevo (sin contar archivos grandes):

| Formato | Tiempo total | Crecimiento | Tras `repack -d --geometric=2` |
|---|---|---|---|
| Objetos sueltos (`gix-hint`; `cli` es igual) | 100 s | +280 MiB | **+47 MiB** (16 s de repack) |
| Un pack por snapshot (`fi`) | 306 s | +273 MiB | +48 MiB (16 s) |

Sin mantenimiento, cada captura deja unos 25 árboles nuevos como objetos sueltos. El mantenimiento de ADR-TMC-007 debe consolidar a diario o por umbral. Con este ritmo, un mes de retención (30 días) ocupa unos 200 MiB tras consolidar, muy por debajo de la cuota de 20 GB de SEC-TMC-12. Lo que de verdad consume cuota son los archivos grandes de los previos garantizados: el banco, que reescribió 100 veces 20 MB y tres veces 1 GB, dejó el almacén en unos 6–7 GiB.

### 5.6 Repo intacto (NFR-01, garantía 4 de ADR-TMC-001)

En cada ventana medida (34 en el banco completo) se compara la huella de todo el `.git` del usuario y de sus worktrees: ruta, tamaño, `mtime`, inodo y contenido de todo salvo los packs. También se comparan `for-each-ref`, `log --all` y `stash list`. **Resultado: idéntico en todas las ventanas** con la siembra por clon. Con enlace duro, la guarda falló (§ 5.4). Para que la detección no escriba el índice del usuario hacen falta `GIT_OPTIONAL_LOCKS=0`, `core.fsmonitor=false` y `core.untrackedCache=false`.

### 5.7 Sensibilidad al tamaño del repo (`P50` y `L`)

Pasadas reducidas: 1 worktree, `cli`, `fi-hint` (con `fastimport.unpackLimit=0`) y `gix-hint`; n = 100 en `P50` y n = 50 en `L`. p95 en ms.

| Perfil | Archivos / working tree / commits / historial | `cli` camino rápido | `cli` 1 arch. | `cli` 100 arch. | `fi-hint` 100 / 20 MB | `gix-hint` 100 arch. | `gix-hint` 100 / 20 MB | `gix-hint` 1.000 arch. |
|---|---|---|---|---|---|---|---|---|
| `P50` | 6.001 / 53 MB / 20.000 / 157 MiB | 31,1 | 118,6 | 711,9 | 209,1 \* | 59,2 | 39,5 \* | 248,1 |
| **`M`** | 10.001 / 316 MB / 50.000 / 627 MiB | 75,0 | 170,8 | 865,0 | 468,4 | 73,5 | **142,1 – 146,1** | 356,5 – 366,8 |
| `L` | 40.001 / 1.090 MB / 150.000 / 1.945 MiB | 104,2 | 225,9 | 841,5 | 439,8 | 72,0 | 126,7 | 500,8 |

\* En `P50` el escenario "20 MB" solo llega a 6,4 MB nuevos: el perfil tiene unos 10 MB de binarios en total.

Lectura:

- **Con los escalones 2 y 3, el coste depende del delta y casi nada del tamaño del repo**. `L` cumple el delta de referencia (127 ms) igual que `M`. Lo único que crece con el repo es la construcción de árboles con 1.000 archivos dispersos: 64 ms en `P50` y 312 ms en `L`.
- **Con la línea base, el coste fijo crece con el repo**: la detección con `git status` pasa de 27 a 71 y 99 ms (p95), y el camino rápido de 31 a 75 y 104 ms.
- Por eso, **elegir `P50` o `M` como referencia cambia poco el gate** con el diseño recomendado: unos 14 ms con 100 archivos. Con un diseño basado en `git status` cambiaría mucho. Es un argumento más para `M`: es conservador sin coste real.

## 6. Veredicto por hipótesis

| Hipótesis del SPIKE | Veredicto | Evidencia |
|---|---|---|
| Repo mediano ≈ 10.000 archivos, 300 MB y 50.000 commits | **Confirmada y aprobada** (D-TMC-21, Rene Bonilla, 2026-10-04), con 627 MiB de historial | § 3: conservador frente a la muestra (~p65–p90) |
| Con el almacén sembrado y un delta de hasta 100 archivos y 20 MB, se cumple el reparto por etapa de ADR-TMC-006 § 2, también en Windows | **Parcial**. El **total** se cumple solo con los escalones 2 y 3 (142 ms). El **reparto no**: blobs 92 ms (> 60) y árboles 37–46 ms (> 30). Windows y Linux sin verificar | § 5.1 |
| El camino rápido cuesta menos de 50 ms | **Solo con el escalón 2**: 4 ms. Con `git status` cuesta 75 ms en `M` (43 ms con `fi`) | § 5.1 |
| La siembra por enlace duro tarda segundos y casi no ocupa disco; la copia escala con el historial | **Coste confirmado, pero el enlace duro se descarta por seguridad**: incumple la garantía 4. El clon APFS cumple lo mismo sin el efecto. La copia escala con el historial (628 MiB) | § 5.4 |
| La captura por observación con `Q` = 1 s, `M` = 5 s y 10 worktrees no lleva el p95 del motor por encima de 300 ms | **No verificable** (el motor no existe). Indicador favorable con los escalones 2 y 3: unos 5 % de CPU en el caso `M`; un previo a 80 ms con 9 worktrees capturando 5 veces más rápido. Con `cli` no sería aceptable | § 5.3 |

**Valores medidos para la captura por observación**: `Q` = 1 s y `M` = 5 s se mantienen (unos 23 ms por captura). El límite de **50 MB se mantiene**: con gitoxide, un archivo de 50 MB cuesta unos 0,4 s por captura, y un archivo grande que cambia en cada `M` costaría unos 8 % de un núcleo; subirlo multiplica ese coste. **Cuotas de SEC-TMC-12**: 20 GB por repo y espacio libre máx(5 GB, 5 %) son holgados para el texto. Lo que puede agotarlas son los archivos grandes de los previos garantizados.

## 7. Enmiendas recomendadas

Los ADR no se tocan desde el spike. **Aplicadas el 2026-10-04** en las secciones "Enmienda (2026-10-04, SPIKE-TMC-001)" de ADR-TMC-001, 002, 004, 006 y 007, en los NFR de la feature, en TS-TMC-001 y en US-TMC-020, con ajustes del Arquitecto. Los más relevantes: el reparto final de E3 es detección ≤ 5, árboles + commit ≤ 45 y ref + oplog ≤ 25; E6 no ejecuta `xcrun`; E9 fija `untrackedCache=false`. Lo que toca a motor-local queda como nota pendiente en el overview de la feature, § 7.2.

| # | Documento | Enmienda |
|---|---|---|
| E1 | ADR-TMC-006 § 5 | **Activar el escalón 3** (gitoxide solo en el almacén; preaprobado, TQ-4 → a) y anotar la medición: los escalones 1 y 2 no bastan para el delta de referencia (456–564 ms frente a 142 ms). |
| E2 | ADR-TMC-006 § 5 y ADR-TMC-004 | **El escalón 2 pasa a ser parte del diseño base, no un recurso de emergencia**: la detección usa las rutas que el motor ya publicó (ADR-GRP-010) más una caché de stat, y el recorrido completo (`status`) queda como verificación periódica fuera de la ruta crítica. El motor debe exponer, por worktree, "rutas cambiadas desde la marca X". Interfaz pendiente con TS-GRP-002/003. |
| E3 | ADR-TMC-006 § 2 | **Reparto medido** (p95 de `gix-hint`, delta de referencia): detección ~1 ms, anclaje ~0, blobs 92, árboles + commit 37, ref + oplog 19. Propuesta: admisión ≤ 10, detección ≤ 15, anclaje ≤ 5, **blobs ≤ 90**, **árboles + commit ≤ 40**, ref + oplog ≤ 20 (suma 180, margen 20). Delta de referencia sin cambios (100 archivos / 20 MB). |
| E4 | ADR-TMC-006 § 3 | Repo de referencia = perfil `M` del generador (§ 3.2), **aprobado por Rene el 2026-10-04** (D-TMC-21): sustituir la hipótesis por las cifras. Reutilizar el generador en INF-GRP-002 y en el banco de US-TMC-020. |
| E5 | ADR-TMC-001 § 3 y ADR-TMC-001 § 4 | **Siembra por clon con copia en escritura, no por enlace duro**: `clonefile` (APFS), `FICLONE` (Btrfs, XFS) y block cloning (ReFS); si no hay, copia. Motivo: el enlace duro cambia el `mtime` del pack del usuario (garantía 4) e impide el 0600. Cambia la mitigación de disco "enlaces duros" de las consecuencias. |
| E6 | ADR-TMC-002 (capa de escritura) / ADR-GRP-001 | Resolver una vez la ruta real del binario de Git (en macOS, `xcrun -f git`) y no lanzar `/usr/bin/git`: el shim cuesta unos 17 ms extra por proceso. Afecta también a las operaciones del motor y del ejecutor. |
| E7 | ADR-TMC-001 § 4 | Configuración del almacén: `pack.compression=1`, `core.bigFileThreshold=128k` (evita deltas caros sobre binarios) y, si se usa `fast-import`, `fastimport.unpackLimit=0`. Durabilidad con objetos sueltos: `fsync` simple por objeto y **una** barrera `F_FULLFSYNC` antes de la ref (el esquema `batch`). Con `core.fsync` por objeto y `F_FULLFSYNC`, Git CLI paga unos 4 ms por objeto. |
| E8 | ADR-TMC-004 § 2 | **Prioridad del snapshot previo sobre la captura por observación** en el escritor único del almacén: la captura de fondo cede (o se trocea) cuando hay un previo en cola. Con la línea base, la cola llegó a 934 ms. |
| E9 | ADR-TMC-001 § 2 / ADR-GRP-009 | Toda lectura del repo del usuario para capturar va con `GIT_OPTIONAL_LOCKS=0`, `core.fsmonitor=false` y sin `untrackedCache`. Consecuencia que conviene anotar: si el índice del usuario está desactualizado (entradas *racy* o tocadas), la Time Machine no puede refrescarlo y `status` re-hashea esos archivos en cada captura. El escalón 2 lo evita. |
| E10 | ADR-TMC-007 | Mantenimiento: `repack -d --geometric=2` diario o por umbral (una semana de capturas: 280 → 47 MiB, 16 s en segundo plano). |
| E11 | US-TMC-020 / UX | Mostrar progreso en un previo garantizado con archivos grandes (1 GB tarda unos 7 s con gitoxide). |
| E12 | SPIKE-TMC-001 | El SPIKE nombra su entregable `research/SPIKE-TMC-001-repo-mediano-overhead.md`; el brief de la tarea pidió `research/SPIKE-TMC-001-resultados.md`, que es este documento. Ajustar la referencia. |

## 8. Linux y Windows (sin verificar)

No se ejecutó en Linux ni en Windows desde esta máquina. El prototipo usa APIs de Unix para la medición (`libc::getrusage`, `fsync`, modos de archivo) y `cp -c` para la siembra, así que **en Windows no compila tal cual**.

**Linux** (x64 y arm64; mejor en un runner dedicado):

1. `spikes/snapshot-overhead/run.sh M` en ext4 y en Btrfs o XFS.
2. Antes, cambiar la siembra por clon a `cp --reflink=always` (Btrfs, XFS) y copia en ext4.
3. Qué vigilar: el coste de `fsync` en ext4 (sin `F_FULLFSYNC`, `fsync` sí vacía la caché del disco), el coste de lanzar procesos (se espera menor que en macOS) y la diferencia de disco entre reflink y copia.

**Windows** (NTFS y ReFS / Dev Drive):

1. Portar las piezas de Unix: `getrusage` a `GetProcessTimes`, el `fsync` simple a `FlushFileBuffers` y la siembra a block cloning en ReFS o copia en NTFS.
2. Ejecutar `run.sh` desde Git Bash.
3. Qué vigilar: el coste de lanzar procesos (el mayor riesgo para `cli` y `fi`), el antivirus en tiempo real sobre `objects/` (excluir `tm/` como pide SEC-TMC-06), el coste de crear miles de objetos sueltos en NTFS (puede inclinar la balanza hacia escribir packs) y `core.symlinks`.

El gate de CI de US-TMC-020 (ADR-TMC-006 § 4) es el lugar natural para cerrar esto en los tres SO.

## 9. Pendiente

- ~~Enmiendas E1–E12 en sus ADR e historias~~: aplicadas el 2026-10-04 (ver § 7). Quedan las Dev Specs y las notas para motor-local.
- Medir en Linux y Windows (§ 8) y en los runners de CI.
- Medir el p95 del motor con la Time Machine activa cuando existan TS-GRP-002/003 (hipótesis 5).
- Medir la admisión y el canal (10 ms) cuando exista el daemon.
- Probar archivos LFS reales con `git-lfs`, solo para confirmar que no difieren de un archivo grande.
