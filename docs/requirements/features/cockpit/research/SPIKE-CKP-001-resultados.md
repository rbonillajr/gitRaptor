---
id: SPIKE-CKP-001-RES
title: "Resultados de SPIKE-CKP-001: merge en seco con gix-merge en un proceso trabajador, sin escribir en el repo (macOS)"
type: research
status: done
feature: cockpit
domain: GRP
spike: SPIKE-CKP-001
created: 2026-10-20
updated: 2026-10-20
related:
  adrs: [ADR-CKP-001, ADR-GRP-009, ADR-GRP-011, ADR-GRP-015]
  stories: [TS-CKP-001, INF-GRP-001, INF-GRP-002, US-CKP-006, US-CKP-010, US-MCP-016]
  spikes: [SPIKE-CKP-001]
tags: [cockpit, spike, prediccion-conflictos, merge-en-seco, gitoxide, gix-merge, merge-tree, proceso-trabajador, rendimiento, fidelidad, repo-intacto, m-02, m-06, l-04]
---

# Resultados de SPIKE-CKP-001 (macOS)

> **Alcance de esta entrega**: macOS 26 (Apple M5, 10 núcleos, 32 GB, APFS), `gix` 0.89.0 / `gix-merge` 0.22.0 (y 0.88.0 / 0.21.0 para H9), Git **2.38.5** (mínima, NFR-07), **2.50.1** (Apple) y **2.56.0** (última estable). El prototipo y la evidencia están en [`spikes/conflict-prediction/`](../../../../../spikes/conflict-prediction/). Cada afirmación remite a un archivo de `results/indicative-2026-10-20/`. **Linux y Windows quedan sin verificar** (§ 10).
>
> **Mediciones de esta entrega: fase (a), indicativas.** Se tomaron con la máquina cargada por otros workers (carga media entre 3,5 y 12, anotada en cada archivo). La **fase (b)**, con la máquina en reposo, la lanza el coordinador con [`measure.sh`](../../../../../spikes/conflict-prediction/measure.sh) y escribe en `results/final-<fecha>/`. **La decisión no espera a la fase (b)** (plan, regla 5): los criterios eliminatorios no dependen de la carga, y las cifras de rendimiento cumplen con margen de 50 veces aunque la máquina estaba cargada (§ 5). La fase (b) confirma las cifras y añade el p95 del motor con la ráfaga del predictor (E9), que no se midió en la fase (a).

## 1. Veredicto

| Pregunta | Resultado en macOS |
|---|---|
| **Mecanismo** | **A2**: `gix_merge::commit` con plataformas propias (sin drivers, pila de atributos vacía, filtros sin drivers). **B queda eliminada** por el criterio 1: refresca el mtime y el ctime de objetos sueltos del repo en todas las versiones de Git (§ 3.1). A1 solo sirvió de control: ejecuta el driver de merge del repo (§ 3.2) |
| **Dónde corre** | En un **proceso trabajador** (A2-W). Dentro del daemon no se puede acotar el tiempo: un merge de texto de un solo blob de 8 MB tarda 36 s y no hay punto de corte (§ 6). Tampoco la memoria: un par con 10.000 archivos en conflicto llega a 882 MB de RSS con todas las cotas previas puestas |
| **Salida del árbol** | **S4 con A2-W → rama R-A2W de la Dev Spec.** A2 pasa los eliminatorios 1 a 4, pero se queda en el 93,4 % de paridad en `git/git` (umbral provisional: 95 %), y B no pasa el criterio 1. Las diferencias medidas pasan a límites declarados (§ 4.4). El PO corrigió el umbral con los datos (≥ 90 % de paridad y ≤ 2 % de falsos negativos en `git/git`) y mantuvo el KPI del 70 % (§ 13) |
| **Frescura (≤ 5 s p95)** | **Cumple con margen**: el recálculo de los 10 pares de un worktree tras un commit tarda **96 ms de p95** con A2-W y 100 ms con A2, con 10 worktrees y el repo de 100K commits, en una máquina con carga 4-6. Los 55 pares iniciales, 719 ms de p95 |
| **Fidelidad** | R-SYN (31 casos): **100 %**, con 0 falsos negativos en la demo del BRD § 13. `git/git` (1.989 pares de merges reales): **84,4 %** con `rewrites.limit` = 1.000 y **93,4 %** escalando a 100.000 los pares con conflictos de borrado o renombrado. Hay 3 pares con falso negativo (1,4 % de los pares con conflicto real) |
| **Hunks (H7)** | 85 % de rangos idénticos y **96,5 %** si se unen los bloques separados por ≤ 3 líneas, como hace Git. Los marcadores de 32 caracteres no confunden las líneas que imitan un marcador; los de 7, sí |
| **Prefiltro (H6)** | **Se descarta**: tiene un falso negativo (renombrado de directorio) y en la flota cuesta más de lo que ahorra (p95 de 191 ms con prefiltro frente a 100 ms sin él) |
| **Versión de `gix` (H9)** | 0.88 y 0.89 dan **el mismo conjunto de conflictos y los mismos hunks** en los 1.989 pares reales y en R-SYN. La pérdida de F2 afecta al árbol fusionado, no al conjunto de conflictos de los casos construidos. Se mantiene 0.89.0 como mínima (ya está en `main`, G3 cerrado) |

## 2. Prototipo y método

`spikes/conflict-prediction/` es un workspace de Cargo propio (no es miembro del de GitRaptor) con el binario `ckp-spike`. Usa por ruta `gitraptor-testkit` para la huella de INF-GRP-001 y el generador del repo H de INF-GRP-002. La regla de copiar `fingerprint.rs` del plan (§ 6) no hizo falta: el testkit compila como dependencia por ruta.

- **A2** (`src/a2.rs`): merge-base (virtual si hay varias) y `gix_merge::commit` con `blob::Platform::new(<pipeline sin drivers>, ToGit, <pila vacía>, Vec::new(), …)` y un `diff::blob::Platform` igual. Todo acceso a objetos pasa por un envoltorio (`Guarded`) con tres cotas: el tope de tamaño de blob leído de la cabecera antes de cargarlo, un plazo comprobado en cada acceso (P6) y un tope de bytes escritos en la memoria de objetos. El repo se abre como `RepoReader::open`, con `ignore_replacements = true` y `with_object_memory()`. La lista de archivos en conflicto sale de aplicar los conflictos a un índice construido desde el árbol fusionado, en memoria. Los conflictos que no dejan etapa en ese índice (F2: un directorio ocupa el lugar del archivo) y los archivos movidos por un renombrado de directorio (`merge.directoryRenames=conflict`, el valor por defecto de `merge-ort`) se añaden aparte. Sin eso, la paridad en R-SYN bajaba del 100 % al 90 %.
- **A2-W** (`src/worker.rs`): el mismo binario como `ckp-spike worker`, de vida larga, que recibe pares por stdin y responde una línea JSON por par. El cliente le aplica `RLIMIT_CPU` y `nice`, un temporizador de 2 s por par y la vigilancia de su memoria residente (`proc_pidinfo`, cada 20 ms), y lo mata y relanza si se pasa.
- **B** (`src/gitcli.rs`): `git --git-dir=<almacén del perfil> merge-tree --write-tree -z`, con `objects/info/alternates` hacia el repo, argv fijo, entorno vacío, configuración aislada, `protocol.allow=never` y `--no-lazy-fetch` en las versiones ≥ 2.45. Ojo: `--no-lazy-fetch` es una opción **global** (`git --no-lazy-fetch merge-tree …`), no de `merge-tree`, que la rechaza con salida 129.
- **Referencia**: `git merge-tree --write-tree` en un clon `--mirror --no-local` desechable, con la misma configuración aislada, `merge.renameLimit=1000` y `conflict-marker-size=32`. Las tres versiones de Git dan exactamente el mismo resultado en los dos corpus. **P4**: en 99 pares de `git/git` con conflicto, `git merge` en un working tree da el mismo conjunto que `merge-tree` (`e6-p4-git-merge.txt`).
- **Repos**: R-H es el perfil `H` de `repogen` con la semilla y el commit extra del banco del motor (100.000 commits, 6.001 archivos), más una flota de 10 worktrees con 3 commits por agente, en la que los agentes `i` e `i+1` editan la misma línea de un archivo (`suites/lib/mk-fleet.sh`). R-SYN tiene 31 casos (`mk-syn.sh`). R-REAL son los últimos 2.000 merges de dos padres de `git/git` (1.989 pares distintos). R-CAN es el canario de `canary.rs` ampliado. R-PC es un *partial clone* `blob:none` de R-SYN con 68 objetos ausentes. R-HOS tiene 5 pares hostiles (`mk-hostile.py`).

## 3. Criterios eliminatorios (E1 a E5)

### 3.1 E1 — Repo intacto (`e1-intact.tsv`, `e1-intact.log`)

Huella de INF-GRP-001 (ruta, tipo, tamaño, contenido, mtime, permisos, inodo y ctime de cada archivo y directorio) de la flota entera: el directorio común, `.git/worktrees/*` y los 11 working trees (unas 70.000 entradas). Se tomó antes y después de cada ejecución, en dos flotas (un clon completo y un clon `--shared` cuyo `alternates` apunta a R-H, con los objetos de R-H también en la huella), en cuatro escenarios (los 55 pares iniciales, un commit en un worktree, la base que se mueve y un par que el usuario ya fusionó) y en 3 corridas.

| Alternativa | Diferencias imputables (3 corridas × 2 flotas × 4 escenarios) |
|---|---|
| A2 | **0** |
| A2-W | **0** |
| B, Git 2.38.5 | 2 por corrida: `mtime` y `ctime` de 2 objetos sueltos del repo |
| B, Git 2.56.0 | 3 por corrida |
| B, Git 2.50.1 | 4 en los 55 pares iniciales y 1 tras un commit |

**H1 confirmada; H2 confirmada.** B escribe objetos en su almacén, pero antes Git "refresca" los objetos que ya existen en cualquier fuente, alternates incluidos (F11), y les cambia el mtime y el ctime dentro del repo del usuario. Pasa en las tres versiones y en todas las corridas. Con objetos empaquetados, el refresco tocaría el `.pack` (no se vio aquí porque los objetos refrescados eran sueltos, de los commits de los agentes). **B no pasa el criterio 1 y queda eliminada.** Por eso su rendimiento y su fidelidad no se midieron aparte (P3), aunque en la flota dio el mismo conjunto de conflictos que A2 en los 55 pares, en 1.043 ms frente a 393 ms.

### 3.2 E2 — Cero ejecución (`e2-e4-canary.tsv`)

R-CAN arma como script que deja un marcador cada programa configurable: `merge.evil.driver` con `merge.default=evil`, filtros `clean`/`smudge`/`process`, `textconv`, `diff.external`, `gpg.program`, el pager, `credential.helper`, `core.fsmonitor` y cuatro hooks. Además pone `merge=evil filter=evil` en el árbol, en el índice, en el disco y en `info/attributes`. Un directorio trampa va primero en el `PATH` y caza cualquier programa lanzado por nombre (`git`, `sh`, `env`, `ssh`, `gpg`, `less`, `git-upload-pack`, `git-lfs`).

| Alternativa | Par con conflicto | Par limpio | Marcadores | Huella del repo |
|---|---|---|---|---|
| A1 (control) | sin conflicto: el driver "resolvió" | — | **`merge-driver`** en los dos pares | 1 cambio |
| A2 | `a.txt` | — | ninguno | 0 |
| A2-W | `a.txt` | — | ninguno | 0 |
| B (3 versiones) | `a.txt` | — | ninguno | 0 |

El canario salta con A1 y no con A2 (P1 confirmado). Con A2-W, el único proceso hijo es el trabajador (el propio binario, lanzado por ruta absoluta). Con B, `git merge-tree` con argv fijo. **No se usó el tracer del kernel** (`eslogger` necesita `sudo` y Full Disk Access en esta máquina): la auditoría de `exec` se cubre con el canario y la trampa del `PATH`, y queda como pendiente de la fase (b) si hay permisos.

### 3.3 E3 — Sin descargas (M-02)

En R-PC, con `remote.origin.uploadpack` apuntando a un script marcador, las cinco variantes (A2, A2-W y B en las tres versiones) devuelven **error** en el par cuyos blobs faltan, con 0 marcadores y 0 cambios en la huella del clon. A2 da `An object with id … could not be found`, que el predictor clasifica como "no calculable (objeto ausente)". **H2, parte de *partial clone***: con B no hay intento de descarga ni siquiera en 2.38, porque el almacén del perfil no es un repo *promisor* (la configuración del *promisor* vive en el repo y el almacén no la lee). Así que `protocol.allow=never` y `--no-lazy-fetch` no llegan a probarse con B; da igual, porque B ya quedó eliminada.

### 3.4 E4 — Mismos objetos y cero atributos (L-04)

R-CAN tiene `refs/replace/<theirs>` → un commit que hace limpio el par, e `info/grafts` que deja a `ours` sin padres (y por tanto sin merge-base). Las tres variantes dan el conflicto de los objetos originales (`a.txt`) con `core.useReplaceRefs` sin definir, `true` y `false`. **La posible inversión de F8 no afecta**: con `ignore_replacements = true` el resultado es el de los objetos originales en los tres casos. El `merge=evil` de las cuatro vías se trata como el driver de texto interno y no ejecuta nada (E2).

### 3.5 E5 — Barrera de API

`spikes/conflict-prediction/tests/api_barrier.rs` falla si `src/a2.rs` llama a `merge_trees`, `merge_commits`, `merge_resource_cache` o `tree_merge_options`, y comprueba que la barrera sí detecta esas llamadas en el control A1. En verde.

## 4. Fidelidad (E6), hunks (E7), prefiltro (H6) y versiones (H9)

### 4.1 R-SYN (`e6-e7-fidelity.txt`)

Los casos son: misma línea, líneas contiguas, solape sin conflicto, añadido/añadido (distinto e igual), modificado/borrado, renombrado con modificación (limpio y con conflicto), renombrado/renombrado, renombrado de directorio, directorio/archivo, binario, submódulo, punteros LFS, CRLF, `merge=union`, `-merge`, F2 (dos variantes), líneas que imitan marcadores, varios hunks con cambios automáticos entre ellos, symlink, modo frente a contenido, historia cruzada (dos variantes) y los 6 pares de la demo del BRD § 13. **Paridad 21/21 en los pares con conflicto (100 %)** contra las tres versiones de Git, y 13/13 archivos con los mismos rangos de hunks (`e6-syn-31-cases.txt`; `e6-e7-fidelity.txt` es la misma corrida con 30 casos, antes de añadir `f2-rename-conflict`). La demo da el conflicto en `src/cart.rs` entre `agent-1` y `agent-2` y nada en los otros 5 pares: **0 falsos negativos (Q-CKP-25)**.

Dos casos coinciden con la referencia pero por una razón que conviene saber. La referencia corre en un clon *bare*, que lee los atributos del árbol de `HEAD` (`main`, sin `.gitattributes`). Por eso `merge=union` y `-merge` dan en ella lo mismo que en A2, que nunca lee atributos. En un working tree con esos atributos, `git merge` daría limpio en `union-attr` y conflicto en `nomerge-attr`. Es un **límite declarado** de L-04 (ADR-CKP-001 § 7): el predictor ignora los atributos a propósito.

### 4.2 R-REAL: 1.989 pares de `git/git`

| Variante de A2 | Pares con conflicto en algún motor | Paridad | Pares con falso negativo | Pares con falso positivo | p95 / p99 / máx. por par |
|---|---|---|---|---|---|
| `rewrites.limit` = 1.000 | 244 | **206 (84,4 %)** | 3 | 27 | 132 / 342 / 448 ms |
| `rewrites.limit` = 100.000 | 226 | 211 (93,4 %) | 3 | 9 | 326 / 1.196 / 10.067 ms |
| **Escalado**: 1.000 y, solo si sale un conflicto de borrado o renombrado, 100.000 (51 pares) | 226 | **211 (93,4 %)** | 3 | 9 | **152 / 727 / 1.726 ms** |

Fuentes: `e6-e7-fidelity.txt` y `e6-rename-limit-escalation.txt`. La carga estaba entre 5 y 8.

**De dónde salen las diferencias:**

1. **Falsos positivos de modificado/borrado (41 archivos en 27 pares con el límite de 1.000).** Un lado modifica `Documentation/x.txt` y el otro la renombra a `x.adoc`, en el commit de `git/git` que renombró cientos de archivos. `merge-ort` encuentra el renombrado porque solo considera las fuentes **relevantes** (las que el otro lado modificó). `gix-merge` considera todas, pasa del límite y se rinde: avisa "modificado/borrado" donde Git fusiona limpio. Subir el límite cierra 18 de los 27 pares, pero cuesta: un par llega a 10 s. **El escalado da la misma paridad con un p99 de 727 ms.** Quedan 9 pares de falso positivo (`gitk` y `git-gui.sh` de los merges de subárbol, entre otros).
2. **Falsos negativos de contenido (4 archivos en 4 pares; en 3 de ellos el par queda sin ningún ⚡).** `gix-merge` da por fusionado sin conflicto un archivo en el que los dos lados insertan bloques distintos en el mismo punto, y Git lo marca como conflicto (por ejemplo, `ci/run-build-and-tests.sh`, en el par `bd5c2827`/`ffe8005b`). Es una diferencia del merge de texto de `gix-merge`, y no se corrige con ninguna opción. Se repite con Myers en lugar de histogram.
3. **Falsos positivos de contenido (2 archivos)**: el caso inverso, un conflicto que Git resuelve solo.

**Lectura para el producto.** Los falsos positivos son conservadores: avisan de un conflicto que Git habría resuelto. Los falsos negativos son lo que daña el KPI: 3 pares de 217 con conflicto real (1,4 %) según la referencia. `git/git` es un corpus exigente, con merges de ramas largas, renombrados masivos y subárboles. La flota de agentes de R-H y los 31 casos de R-SYN dan 100 %.

### 4.3 Hunks (E7, H7)

- Con el método de la Dev Spec (D7: el blob fusionado con marcadores, llevado a las coordenadas de cada lado con un diff de alineación, `src/hunks.rs`), los rangos son **idénticos en el 85,0 %** de los 287 archivos (288 en la corrida con escalado, de ahí el 85,1 %) con conflicto de contenido de R-REAL. En R-SYN, en el 100 %, incluido el caso con cambios automáticos entre bloques, cuyas coordenadas de cada lado difieren.
- Casi toda la diferencia (38 de 43 archivos) es **cómo se parten los bloques**. Git (`XDL_MERGE_ZEALOUS_ALNUM`) une dos bloques de conflicto separados por pocas líneas, y `gix-merge` los deja separados. **Uniendo los bloques separados por ≤ 3 líneas en los dos lados, la coincidencia sube al 96,5 %** (97,6 % con ≤ 10). **H7 se confirma con esa unión**, que pasa a formar parte del extractor.
- **Marcadores**: con el tamaño por defecto (7), un archivo con líneas que imitan un marcador produce un bloque falso (`[1,0,1,0]`). Con 32 caracteres, ninguno. Se fija `marker_size` = 32 en las opciones de texto. `marker_size_multiplier` queda en su valor por defecto, porque es lo que distingue los marcadores de una base virtual.

### 4.4 Límites declarados que añade el spike (ADR-CKP-001 § 7)

1. Atributos de merge (`merge=union`, `-merge`, drivers): se ignoran por L-04. Un archivo con `merge=union` puede aparecer como conflicto previsto aunque Git lo fusione.
2. Renombrados que `merge-ort` encuentra por relevancia y `gix-merge` no, dentro del límite fijo: aparecen como "modificado/borrado" previsto (falso positivo).
3. Inserciones distintas en el mismo punto de un archivo: `gix-merge` puede darlas por fusionadas (falso negativo, 1,4 % de los pares con conflicto en `git/git`).
4. Los bloques de hunks cercanos se publican unidos (≤ 3 líneas), así que un bloque publicado puede abarcar líneas que no están en conflicto.

### 4.5 Prefiltro (H6): se descarta

- R-SYN: se salta 4 de 30 pares y **tiene un falso negativo**, `dir-rename`. Un lado renombra `d/` → `e/` y el otro añade `d/new.txt`. Los conjuntos de archivos no se cortan, pero `merge-ort` lo marca como conflicto de ubicación.
- R-REAL: se salta el 37,9 %, sin falsos negativos.
- Flota R-H: se salta el 10 % y el recálculo pasa de **100 ms a 191 ms de p95**, porque el prefiltro calcula la merge-base y dos diffs de árbol por par, lo mismo que hace el merge.

H6 queda refutada en sus dos partes: no está libre de falsos negativos y no ahorra. **El predictor no lleva prefiltro.** El solape (nivel 1) sigue, porque es otra cosa: rutas sin commitear.

### 4.6 Versiones de `gix` (H9)

`gix088/` compila las mismas fuentes contra `gix` 0.88.0 / `gix-merge` 0.21.0. El resultado es **idéntico** al de 0.89 en los 1.989 pares de R-REAL (conjuntos de archivos y hunks; `h9-gix088-vs-gix089.txt`) y en R-SYN, incluidos los dos casos construidos con la forma de F2. **H9 queda refutada en lo que predecía** (un falso negativo con 0.88). La pérdida de F2 está en el árbol fusionado, que el predictor no publica. La versión mínima sigue siendo 0.89.0, porque el extractor usa ese árbol para la lista de conflictos y porque `main` ya está en 0.89 (#251).

## 5. Rendimiento (E8) y coste (E9, parte en proceso)

Flota nueva de R-H con 10 worktrees, `e8-e9-perf-indicative.txt`. Son 200 muestras por escenario, sin contar las 10 primeras (ADR-GRP-011 § 4), con la carga anotada (entre 3,5 y 6,8 durante esta parte).

| Escenario | A2 (en proceso) | A2-W (trabajador) |
|---|---|---|
| Par con delta de 1 archivo por lado (p50 / p95) | 4,3 / 5,4 ms | 4,3 / 4,6 ms |
| 10 archivos | 5,2 / 5,6 ms | 5,2 / 5,6 ms |
| 100 archivos | 15,9 / 19,4 ms | 16,0 / 17,6 ms |
| 1.000 archivos | 154,7 / 187,8 ms | 147,9 / 166,0 ms |
| **Recálculo tras un commit** (10 pares, de fin del commit a último par calculado), p50 / p95 / p99 | 73,8 / **99,8** / 152,6 ms | 85,3 / **95,7** / 103,1 ms |
| Recálculo con prefiltro | 148,8 / 191,1 / 234,5 ms | — |
| Recálculo con 2 trabajos | 80,1 / 104,0 / 146,9 ms | — |
| Cálculo inicial de los 55 pares (repo recién abierto), p50 / p95 | 703 / 814 ms | 664 / 719 ms |
| Ráfaga de 2 min (10 agentes que commitean cada 2 a 10 s): antigüedad publicada p50 / p95 / máx. | 139 / 281 / 368 ms | 139 / 285 / 402 ms |
| Ráfaga: cola máxima / eventos fusionados | 2 / 0 | 2 / 0 |
| Ráfaga: CPU media (% de un núcleo) | 20,8 % | 0,5 % el daemon + el trabajador (21 % ocupado) |
| Memoria residente máxima | 42 MB (proceso entero) | 5 MB el cliente, **42 MB el trabajador** |

**H4 se confirma con margen**: 96 a 100 ms de p95 frente a un objetivo de 5 s. **A2-W no añade coste medible por par**: la ida y vuelta por stdin y stdout queda dentro del ruido, porque el trabajador mantiene abierto el repo y su caché de objetos (H5, coste ≤ 50 ms por par: confirmado, < 1 ms). Con 2 trabajos el recálculo no mejora, porque los 10 pares de un worktree ya caben en ~100 ms y la máquina estaba cargada: **concurrencia 1** basta, y se deja 2 como techo para la ráfaga de la base que se mueve (55 pares).

En el escenario realista no hay que coalescer: con 10 agentes que commitean cada 2 a 10 s, la cola nunca pasó de 2 entradas. La ráfaga de la fase (b) dura 10 minutos.

**Pendiente de la fase (b) (E9)**: el p95 del motor (`t0` → `t_client_recv`, banco de INF-GRP-002) solo y con una ráfaga del predictor de 1 y de 2 trabajos al mismo tiempo. `measure.sh` lo hace. Con un 21 % de un núcleo en la ráfaga y QoS *utility*, no se espera regresión, pero no está medido.

### 5.1 Solape (E11)

Una ronda completa de intersecciones de rutas sin commitear entre 10 worktrees, con uno de ellos en 10.000 rutas, tarda **0,022 ms de p95** y ocupa unos 545 KB. Con 50.000 rutas, 0,021 ms y 2,5 MB. El tope de 10.000 rutas por worktree de ADR-CKP-001 sobra para el coste: **se mantiene como cota de memoria**, no de tiempo.

## 6. Cotas (E10, M-06): por qué el trabajador

`e10-hostile.txt` (5 muestras por caso en la fase (a); 20 en la (b)) y `e10-dense-scaling.txt`.

| Caso hostil | A2 sin cotas | A2 con cotas previas (tope de blob de 8 MiB por cabecera, `rewrites.limit` 1.000, 64 MiB escritos) | A2 con plazo de 2 s en cada acceso a objetos | A2-W (2 s, RSS 512 MiB) con cotas previas |
|---|---|---|---|---|
| Blob de 104 MB en conflicto | 0,9-1,2 s, **2,2 GB de RSS** | cortado por cabecera en < 1 ms | 0,95-1,15 s, 2,2 GB (el plazo no corta) | cortado por cabecera en < 6 ms |
| 10.000 archivos en conflicto | 1,9-2,1 s, **2,6 GB** | 1,8-2,2 s, **882 MB** | 2,0 s, 2,6 GB | **muerto por RSS a los 120-140 ms** |
| Árbol de 200 niveles | 5-13 ms | 5-13 ms | 5-13 ms | 6-13 ms |
| 5.000 renombrados en un lado | 93-100 ms | 93-100 ms | 92-116 ms | 93-99 ms |
| Un archivo de 8 MB con cambios en una de cada 2 líneas (ours) y una de cada 3 (theirs) | **33-36 s** | cortado (8,47 MB > 8 MiB) | **36 s: el plazo vence durante el merge del blob y solo se ve al terminar** | cortado (por tamaño); sin tope de tamaño, **muerto a los 2,0 s** |

**Escalado del caso denso** (`e10-dense-scaling.txt`): el merge de texto de un solo blob es cuadrático. Tarda 7 ms con 2.000 líneas (96 KB), 111 ms con 10.000 (484 KB), 429 ms con 20.000 (979 KB) y **1,68 s con 40.000 líneas (1,97 MB)**. Git 2.56 tarda más: 2,37 s con 40.000 líneas. Con 80.000 líneas (4 MB) son más de 6 s.

**Conclusión de E10 (H5 confirmada):**

- **Tiempo**: el único punto de corte dentro del proceso es el acceso a objetos (P6), y el merge de texto de un blob no accede a objetos. Un blob de 2 MB ya basta para pasar de 2 s sin que nada lo interrumpa. Bajar el tope de blob a ~1 MB dejaría fuera archivos normales (un `Cargo.lock` o un `package-lock.json` de un proyecto mediano pasa de 1 MB) y aun así no da una garantía, porque el coste depende del contenido y no solo del tamaño.
- **Memoria**: con todas las cotas previas, un par de 10.000 archivos en conflicto llega a 882 MB, y con `fail_on_conflict` (para en el primero), a 437 MB. La memoria va al diff de árboles y a las estructuras del merge, que ninguna cota previa controla.
- **Con A2-W, las dos cotas son duras**: el temporizador mata al trabajador a los 2,0 s y la vigilancia de RSS lo mata cuando pasa la cota (con 20 ms de muestreo se pasó hasta 130 MB por encima de 512 MB antes de morir). El trabajador se relanza y el par queda "no calculable (límite)". En macOS, `RLIMIT_AS`/`RLIMIT_RSS` no se imponen: la vigilancia desde el padre es necesaria (T006 de la Dev Spec ya lo preveía).

Las cotas previas siguen siendo útiles dentro del trabajador, porque evitan matarlo por casos que se reconocen antes: el tope de blob por cabecera corta el blob de 104 MB en milisegundos sin relanzar nada.

## 7. Decisión: S4 con A2-W → rama R-A2W

Árbol de salida del plan (§ 5), aplicado en orden:

1. **Criterios eliminatorios 1 a 4**: A2 y A2-W pasan. B falla el 1 (E1).
2. **Fidelidad (criterio 5)**: A2 se queda en el 93,4 % de paridad en `git/git` con el escalado, por debajo del umbral provisional del 95 %, con 0 falsos negativos en la demo del BRD § 13. Como B no pasa los eliminatorios, **S3 y S3b no aplican → S4**: se mantiene A2 y las diferencias medidas pasan a límites declarados (§ 4.4), con revisión del KPI del 70 % por el PO.
3. **Frescura (criterio 6)**: cumple (≤ 0,1 s p95 frente a 5 s).
4. **Coste (criterio 7)**: cumple en proceso (42 MB, 21 % de un núcleo en ráfaga). El p95 del motor queda para la fase (b).
5. **Cotas (criterio 8)**: no se cumplen dentro del proceso (§ 6) → **A2-W**.

**Rama de la Dev Spec: R-A2W.** Se aplica la enmienda condicionada de ADR-GRP-009 Validación 5: el propio binario (`raptor`) como único módulo autorizado a lanzar el trabajador del predictor, fuera de `crates/git`.

## 8. Cifras para ADR-CKP-001 y la Dev Spec (plan § 7)

| # | Cifra o decisión | Valor | Evidencia |
|---|---|---|---|
| 1 | Mecanismo y rama | A2 (`gix_merge::commit` con plataformas propias) en un proceso trabajador; **R-A2W** (S4) | § 3, § 6, § 7 |
| 2 | Dónde corre | Proceso trabajador de vida larga (el propio binario), uno por daemon, relanzado tras un límite; se aplica la enmienda de ADR-GRP-009 V5 | § 6 |
| 3a | `PER_PAIR_TIMEOUT` | 2 s, temporizador del daemon sobre el trabajador | E10 |
| 3b | `JOB_MEMORY_CAP` | **512 MiB de RSS del trabajador como umbral de muerte** (no es un presupuesto), vigilado cada 20 ms desde el daemon; puede pasarse ~130 MiB antes de morir. Un trabajo normal usa 42 MB. El trabajador se cierra cuando no tiene trabajo, para no contar en la huella en reposo | E8, E10 |
| 3c | `max_blob_bytes` | **8 MiB**, leído de la cabecera antes de cargar | E10 |
| 3d | Tope de bytes escritos en la memoria de objetos por par | 64 MiB | E10 |
| 3e | `rewrites_limit` | **1.000**, y **escalado a 100.000** como segundo trabajo, con su propio plazo de 2 s, solo para los pares cuyo resultado tenga conflictos de borrado o renombrado. Si el segundo trabajo choca con un límite, se publica el resultado del primero (con el límite declarado 2) | § 4.2 |
| 3f | Marcadores | `marker_size` = 32 en las opciones de texto. `marker_size_multiplier` se deja en su valor por defecto (⚠️ lo comprueba un caso de historia cruzada en T011) para que los marcadores de una base virtual no se confundan con los del merge | § 4.3 |
| 3g | Unión de bloques de hunks | Bloques separados por ≤ 3 líneas en los dos lados se publican unidos | § 4.3 |
| 3h | `PREDICT_CONCURRENCY` | **1** trabajo en el trabajador, sin techo de 2: los 55 pares en serie tardan 719 ms de p95. `RLIMIT_CPU` no sirve como cota por par en un trabajador de vida larga, porque se acumula | E8 |
| 3i | `MAX_UNCOMMITTED_FOR_OVERLAP` | 10.000 rutas por worktree (cota de memoria; el coste es despreciable) | E11 |
| 3j | Tiempo y memoria por par | 4-19 ms con deltas de 1 a 100 archivos y ~150-190 ms con 1.000; 42 MB residentes en el trabajador | E8 |
| 3k | `max_files` y `max_hunks_per_file` | Se mantienen (200 y 50); el spike no las midió por separado. Un par de 10.000 conflictos acaba por RSS antes de llegar a ellas | — |
| 4 | Prefiltro | **No** | § 4.5 |
| 5 | Umbral de fidelidad (`FIDELITY_THRESHOLD`) | **≥ 90 % de paridad y ≤ 2 % de pares con falso negativo** en `git/git`; 100 % y 0 falsos negativos en R-SYN y en la demo; sin empeorar la línea base (93,4 % y 3 pares) (decisión del PO, § 13) | § 4.2 |
| 6 | ¿El objetivo de 5 s pasa a gate en INF-GRP-002? | **Sí**, con el escenario de T012 (de fin del commit a predicción publicada) | E8 |
| 7 | Versión mínima de `gix` | 0.89.0 (ya en `main`; G3 cerrado) | § 4.6 |
| 8 | Correcciones a ADR-CKP-001 | F1, F3, F4 y F10 del plan, más los hallazgos de § 9 de este Brief | Enmienda de ADR-CKP-001 |

## 9. Hallazgos que no estaban en el plan

1. **El refresco de B toca objetos sueltos del repo del usuario** en las tres versiones de Git, también con `merge-tree --write-tree` desde un almacén con alternates. Confirmado en ejecución (F11).
2. **`--no-lazy-fetch` es una opción global de `git`**, no de `merge-tree` (corrige la redacción de F10 y de ADR-CKP-001 § 1 (b)).
3. **Con un almacén en el perfil no hay descarga perezosa**, porque el almacén no es *promisor*. El riesgo de M-02 con B era menor de lo previsto, pero B ya cae por E1.
4. **`gix-merge` no poda las fuentes de renombrado irrelevantes**, como hace `merge-ort`: con muchos archivos renombrados, el límite se agota y salen falsos positivos de modificado/borrado. El escalado del límite lo compensa sin pasar de 2 s.
5. **Hay una diferencia en el merge de texto de `gix-merge`**: inserciones distintas en el mismo punto, que `gix` fusiona y Git marca como conflicto (3 pares en `git/git`). Es candidata a un *issue* aguas arriba con el par reproducible.
6. **Los conflictos sin etapa en el índice** (F2) y los renombrados de directorio no aparecen con `apply_index_entries` sobre el árbol fusionado: el extractor los añade aparte (§ 2).
7. **El merge de texto de un blob es cuadrático** en el caso denso, también en Git. No hay tope de tamaño que acote el tiempo de forma razonable: el plazo solo se puede imponer desde fuera del proceso.
8. **La memoria por par crece con el número de archivos en conflicto** (unos 88 KB por archivo), no con el tamaño del repo, y ninguna cota previa la controla.

## 10. No verificado y pendiente

- **Fase (b), con la máquina en reposo**: todas las cifras de este Brief son indicativas. `measure.sh` repite E1 a E10 con 20 muestras en E10, la ráfaga de 10 minutos y H9, y añade el p95 del motor con y sin ráfaga (E9). **El coordinador la lanza cuando no haya workers**; sus resultados se añadirán en una actualización de este Brief, sin cambiar la decisión salvo que E9 muestre una regresión del p95 del motor.
- **Auditoría de `exec` con el tracer del kernel** (`eslogger`): no disponible sin `sudo` y Full Disk Access. Se cubrió con el canario y la trampa del `PATH`.
- **`gc` del usuario concurrente** (E1 del plan, con ejecución de control): no se ejecutó. A2 no escribe nada y E1 lo muestra en 24 combinaciones; la concurrencia con `gc` queda para la suite "predicción" de INF-GRP-001 (T010 de la Dev Spec).
- **Rendimiento y fidelidad de B**: no se midieron aparte, por P3 (B cae en E1).
- **`max_files` y `max_hunks_per_file`**: no se midieron por separado.
- **Linux y Windows: Pendiente: etapa de validación multiplataforma.** El procedimiento es el del README del prototipo. En Linux, la memoria del hijo se lee de `/proc/<pid>/statm` y se puede añadir `RLIMIT_AS`. En Windows, un *Job Object* da el tope de memoria y de CPU.

## 11. Reproducción

```sh
cd spikes/conflict-prediction
cargo build --release && (cd gix088 && cargo build --release)
git clone --bare https://github.com/git/git.git /private/tmp/ckp-spike/git.git
GIT_SRC=/private/tmp/ckp-spike/git.git ./setup-git.sh
bash suites/e1-intact.sh 3; bash suites/e2-e4-canary.sh; bash suites/e6-e7-fidelity.sh 2000 100
bash suites/e8-e9-perf.sh indicative 200 120; bash suites/e10-hostile.sh 5
./measure.sh        # fase (b), con la máquina en reposo
```

## 12. Decisiones de este spike

**Decisiones del orquestador (2026-10-20)**, validadas por el Arquitecto (`nassa-architect:architect`) y el PO (`nassa-aadd:product-owner`) en el § 13:

- **D-SPK-1**: se decide con la fase (a), sin esperar a la (b) (regla 5 del encargo). Los eliminatorios no dependen de la carga y la frescura cumple con un margen de 50 veces.
- **D-SPK-2**: la salida es S4 + A2-W → R-A2W.
- **D-SPK-3**: no hay prefiltro.
- **D-SPK-4**: `rewrites.limit` = 1.000, escalado a 100.000 como segundo trabajo con su propio plazo para los pares con conflictos de borrado o renombrado (ajuste del Arquitecto: si se corta, vale el primer resultado).
- **D-SPK-5**: `JOB_MEMORY_CAP` = 512 MiB de RSS del trabajador, como umbral de muerte y no como presupuesto (ajuste del Arquitecto: con ~88 KB por archivo, 256 MiB cubrirían ~2.900 archivos, muy por encima de `max_files`).
- **D-SPK-6**: los bloques de hunks separados por ≤ 3 líneas se unen.
- **D-SPK-7**: `marker_size` = 32, con `marker_size_multiplier` en su valor por defecto (ajuste del Arquitecto).
- **D-SPK-8** (Arquitecto): `PREDICT_CONCURRENCY` = 1, sin techo de 2.

## 13. Validación

**Arquitecto (`nassa-architect:architect`, 2026-10-20)**:
- De acuerdo con la salida S4 + A2-W → R-A2W (D-SPK-2), con decidir en la fase (a) (D-SPK-1), con quitar el prefiltro (D-SPK-3) y con unir los bloques (D-SPK-6).
- De acuerdo con ajuste en D-SPK-4, D-SPK-5, D-SPK-7 y la cifra 3h. Los ajustes ya están en el § 8 y el § 12.
- E9 sin medir no bloquea: T012 lo lleva como gate desde el principio, y una regresión cambiaría la concurrencia o la clase del trabajador, no el mecanismo (G11 de la Dev Spec).
- Corrigió dos datos de este Brief: el 85,0 % y el 96,5 % salen de corridas con 287 y 288 archivos, y los falsos negativos son 4 archivos en 4 pares.
- Aplicó la rama R-A2W a la Dev Spec (`ready_to_implement: true`, con las tareas que dependen de G2 bloqueadas), la enmienda de ADR-CKP-001 y la de la Validación 5 de ADR-GRP-009.

**PO (`nassa-aadd:product-owner`, 2026-10-20)**:
1. **Umbral de fidelidad corregido.** En `git/git` (los 1.989 pares), paridad ≥ 90 % y falsos negativos ≤ 2 % de los pares con conflicto real. En R-SYN y la demo del BRD § 13, 100 % y 0 falsos negativos. Los falsos positivos no son eliminatorios. Ninguna versión puede empeorar la línea base (93,4 %). Mantener el 95 % dejaría el predictor sin un mecanismo viable, porque B cae por NFR-01.
2. **El KPI del 70 % no cambia** (Q-CKP-21, BR-CKP-CONS-005). Se añade a la medición un diagnóstico que separa dos tipos de conflicto no detectado: los que tenían commits en los dos lados y no recibieron un ⚡, y los que no tenían nada commiteado.
3. **Los límites del § 4.4 son aceptables.**
   - El límite 1 se indica en el archivo del ⚡ cuando ese archivo tiene una regla de merge propia.
   - El límite 3 se declara como "sin ⚡ no garantiza un merge limpio". El Arquitecto confirmó que un archivo que modifican los dos lados sin ⚡ sigue apareciendo como ⚠ solape, porque el solape cubre el trabajo propio de cada lado, commiteado y sin commitear (ADR-CKP-001 § 3).
   - Los límites 2 y 4 van solo en el texto.
