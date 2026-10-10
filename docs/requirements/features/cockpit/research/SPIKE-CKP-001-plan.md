---
id: SPIKE-CKP-001-PLAN
title: "Plan de SPIKE-CKP-001: merge en seco en memoria frente a almacén en el perfil, en ≤ 5 s p95 y sin escribir en el repo"
type: research
status: ready
feature: cockpit
domain: GRP
spike: SPIKE-CKP-001
created: 2026-10-09
updated: 2026-10-09
related:
  adrs: [ADR-CKP-001, ADR-GRP-001, ADR-GRP-006, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-TMC-001]
  stories: [TS-CKP-001, INF-GRP-001, INF-GRP-002, US-CKP-006, US-MCP-016]
  spikes: [SPIKE-CKP-001]
tags: [cockpit, spike, plan, prediccion-conflictos, merge-en-seco, gitoxide, gix-merge, merge-tree, rendimiento, fidelidad, repo-intacto, m-02, m-06, l-04]
---

# Plan de SPIKE-CKP-001 (macOS)

> **Qué es**: el plan de ejecución de [SPIKE-CKP-001](../technical-stories/SPIKE-CKP-001-prediccion-5s.md). Fija las hipótesis, qué se mide y con qué, las alternativas, los criterios de decisión y el prototipo mínimo. Sus resultados irán en `research/SPIKE-CKP-001-resultados.md` (forma de [SPIKE-GRD-001-resultados](../../guardrails/research/SPIKE-GRD-001-resultados.md)) y enmendarán [ADR-CKP-001](../../../../architecture/decisions/ADR-CKP-001-prediccion-conflictos-merge-en-seco.md). La Dev Spec que depende de él es [DS-TS-CKP-001](../dev-specs/TS-CKP-001-predictor-conflictos.md), en borrador condicionado.
>
> **Plataformas**: macOS (arm64, APFS). Linux y Windows: **Pendiente: etapa de validación multiplataforma**, con el mismo procedimiento del README del prototipo.
>
> **No es código del producto**: el prototipo vive en `spikes/conflict-prediction/`, fuera del workspace de Cargo, como `spikes/snapshot-overhead/`. Repos y perfiles temporales; nunca este repo ni el perfil real (NFR-01).

## 1. Estado del arte (investigado el 2026-10-09)

Fuentes: código del tag `gix-v0.89.0` de gitoxide, `master` de `git/git` (2026-10-06), release notes de Git y la API de crates.io. El detalle, con las rutas de código, está en el [anexo](./SPIKE-CKP-001-anexo-estado-del-arte.md). Lo que cambia respecto de ADR-CKP-001 (redactado el 2026-10-04):

| # | Hecho verificado | Fuente | Efecto en el spike |
|---|---|---|---|
| F1 | La última versión es **`gix` 0.89.0 / `gix-merge` 0.22.0 (2026-10-08)**, no 0.88.0 / 0.21.0. El workspace usa `gix` 0.88 **sin** la feature `merge` (`Cargo.toml:27`) | crates.io; `Cargo.toml` | El prototipo fija 0.89.0 y mide también 0.88.0 |
| F2 | `gix-merge` 0.22.0 corrige una **pérdida silenciosa de datos** en el merge de árboles: si un lado quitaba `a/b` y llenaba `a/b/` solo con renombrados, el merge salía limpio y perdía el directorio (commit `18f476cd`) | CHANGELOG del tag | Un falso negativo de la predicción con 0.88. **La versión mínima del predictor es 0.89.0** (H9) |
| F3 | **`gix-merge` no tiene interrupción cooperativa**: no hay `should_interrupt` ni `AtomicBool` en `gix-merge/src`. Las únicas salidas tempranas son `fail_on_conflict` y `rewrites.limit` | código del tag | La "bandera de interrupción" de ADR-CKP-001 § 1 y § 5 no existe. M-06 dentro del proceso solo puede apoyarse en cotas previas (H5) |
| F4 | `Repository::merge_trees` y `merge_commits` construyen su propia `merge_resource_cache`: leen los drivers `merge.<x>.driver` de la configuración, los `.gitattributes` del índice (o del árbol de `HEAD` si no hay índice) y montan un `gix_filter::Pipeline` que **puede lanzar** `filter.<x>.process/clean` | `gix/src/repository/merge.rs` | La API de alto nivel **no cumple L-04 ni SEC-09**. Hay que llamar a `gix_merge::tree(...)` con un `blob::Platform::new(filter, mode, attrs, Vec::new(), opts)` propio: lista de drivers vacía, pila de atributos vacía y pipeline sin drivers (alternativa A2) |
| F5 | `write_buf` de `gix` no escribe un objeto que ya existe y **no refresca su mtime**. Con `with_object_memory()` todo lo escrito se queda en memoria | `gix/src/repository/impls.rs:118`, `cache.rs:48` | Refuerza H1 |
| F6 | No hay API pública de hunks: el blob fusionado con marcadores queda en la memoria de objetos (`ContentMerge::merged_blob_id`) y hay que parsearlo. Los marcadores miden `7 + 2 × marker_size_multiplier` | `gix-merge/src/blob/builtin_driver/text/mod.rs` | La extracción de rangos es código propio y se mide (§ 4, E7) |
| F7 | `gix` no implementa la descarga perezosa (no hay código de *promisor* en `gix-odb`): un objeto ausente es un error | código del tag | Refuerza M-02 con A2 |
| F8 | `gix` no lee `info/grafts`. Para `refs/replace` hay una **posible inversión** en `open/repository.rs:608` (`is_disabled = core.useReplaceRefs.unwrap_or(true)`), sin probar. `RepoReader` ya fija `objects.ignore_replacements = true` (`crates/git/src/reader.rs:185`) | código; `reader.rs` | L-04 se prueba en ejecución, no se supone (E5) |
| F9 | En **Windows**, `gix_path::env::system_config()` puede ejecutar `git`; para cero procesos hacen falta `config.system = false` y `attributes.system = false` | `gix-path/src/env/mod.rs:51` | Nota para la etapa multiplataforma; el prototipo lo fija ya |
| F10 | `git merge-tree`: `--write-tree` en 2.38, `--stdin` en **2.39**, `--merge-base` en 2.40, lee configuración desde 2.41, `-X` en 2.43, `--no-lazy-fetch` / `GIT_NO_LAZY_FETCH` en 2.45 y `--quiet` en **2.50**. La última estable es **2.56.0** (2026-09-28) | Release notes | Corrige ADR-CKP-001 § 11 (decía 2.42 y 2.40). En 2.38 no hay lotes ni corte de la descarga perezosa |
| F11 | Antes de escribir un objeto, Git lo **refresca** (`odb_freshen_object`) en **todas** las fuentes, alternates incluidos, y toca su mtime | `odb.c:815`, `:987` | H2 casi confirmada por el código: B dejaría huella en el repo. Se confirma en ejecución porque es eliminatoria |
| F12 | `--quiet` evita escribir **la mayoría** de los objetos, pero con varias merge-bases escribe los de la base virtual; es incompatible con `-z`, `--name-only`, `--messages` y `--stdin` | `builtin/merge-tree.c:602`, `merge-ort.c:4039` | No sirve para la predicción: no da archivos ni hunks y sigue escribiendo |
| F13 | `merge-tree` ejecuta drivers externos (`run_command`) y, en un repo no *bare*, lee el `.gitattributes` del worktree y luego el índice | `merge-ll.c:205`, `attr.c:851` | Confirma que C (contra el repo) queda rechazada |
| F14 | Madurez: `gix-merge` está en "Initial Development → usable"; su baseline cartesiano coincide 210/210 con `git merge-tree` en un modelo acotado (una base, renombrados exactos, sin atributos ni submódulos); declara desviaciones en renombrados de directorio anidados y no soporta submódulos (cuentan como conflicto) | `crate-status.md`, `tests/merge/tree/` | La fidelidad (H3) es la incógnita real del spike |

**No verificado** (lo cierra el spike en ejecución): la inversión de F8; en qué versión llegaron `-z`, `--name-only` y `--messages`; si `protocol.allow=never` corta la descarga perezosa en 2.38–2.44; el comportamiento de `--quiet` en el tag v2.56.0 (se leyó `master`).

## 2. Alternativas

| Id | Mecanismo | Dónde corre | Estado en este plan |
|---|---|---|---|
| **A1** | `Repository::merge_trees` / `merge_commits` de `gix` sobre `with_object_memory()` | daemon | **Descartada antes de medir** (F4): lee drivers, filtros y atributos del repo. Solo se usa como control en E2 y E4 para demostrar que el canario salta con ella y no con A2 |
| **A2** | `gix_merge::tree(...)` directo, con `blob::Platform` sin drivers, pila de atributos vacía, pipeline de filtros sin drivers, `rewrites.limit` fijo, `marker_size_multiplier` ampliado y la memoria de objetos de `gix` | dentro del daemon (hilos de prioridad baja) | **Preferida** (ADR-CKP-001 § 1, opción a) |
| **A2-W** | A2 en un **proceso trabajador**: el propio binario, de vida larga, con `rlimits` de CPU y memoria, que recibe trabajos por stdin y al que el daemon mata y relanza si un par pasa de 2 s | proceso hijo del daemon | **Respaldo de M-06** (ADR-CKP-001 § 1): se mide siempre, porque F3 hace probable que M-06 no se demuestre dentro del proceso |
| **B** | `git --git-dir=<almacén del perfil> merge-tree --write-tree -z --name-only --messages` con `objects/info/alternates` hacia el repo y las opciones fijas de ADR-CKP-001 § 1 (b), más `--no-lazy-fetch` cuando la versión lo tenga (≥ 2.45) | proceso `git` por par (lote con `--stdin` en ≥ 2.39) | **Respaldo de fidelidad**. Se mide solo lo eliminatorio (E1 a E4); el rendimiento y la fidelidad, solo si lo pasa |
| C | `merge-tree --write-tree` contra el repo | — | Rechazada por ADR-CKP-001 (escribe en `.git/objects` y ejecuta drivers, F13). No se mide |

## 3. Hipótesis

| Id | Hipótesis | Cómo se refuta |
|---|---|---|
| H1 | **A2 deja el repo intacto por construcción**: ningún archivo creado ni modificado bajo `.git` (contenido ni mtime), ningún proceso hijo, ninguna conexión, también con drivers, filtros, `.gitattributes` y `core.fsmonitor` hostiles | Una diferencia en la huella, un marcador del canario o un `exec` en la auditoría (E1, E2) |
| H2 | **B cambia el mtime** de packs u objetos sueltos del repo por el refresco de objetos en alternates (F11), y en un *partial clone* con Git < 2.45 intenta contactar el remoto *promisor* | La huella con mtime queda intacta tras los 55 pares, y la captura de red da 0 conexiones en 2.38 (E1, E3) |
| H3 | **Fidelidad de A2**: coincide con `merge-ort` en el conjunto de archivos en conflicto en ≥ 95 % de los pares del corpus real **que tienen conflicto en alguno de los dos motores** (un corpus casi todo limpio cumpliría el umbral sin probar nada) (⚠️ **ASSUMPTION**: umbral provisional de SPIKE-CKP-001). Las diferencias se concentran en renombrados de directorio, merge-base virtual y submódulos (F14) | Paridad < 95 %, o un falso negativo en la demo del BRD § 13 (E6) |
| H4 | **Rendimiento**: con A2, un commit en un worktree recalcula sus ≤ 10 pares en ≤ 5 s p95 con 10 worktrees y el repo H (100K commits); A2 es más rápido que B porque reutiliza las cachés de packs en un solo proceso | p95 > 5 s (E8) |
| H5 | **M-06 dentro del proceso no se demuestra con A2** (F3): sin interrupción cooperativa, un par hostil puede pasar de 2 s aunque se apliquen el tope de tamaño por cabecera, el `rewrites.limit` fijo y `fail_on_conflict` tras N archivos. A2-W cumple la cota con un coste por par ≤ 50 ms (⚠️ **ASSUMPTION**) frente a A2 | Todos los pares hostiles de E10 terminan en ≤ 2 s con la memoria del proceso bajo su cota, solo con cotas previas |
| H6 | **Prefiltro**: si las rutas commiteadas de un par no se cortan (ampliadas con directorios padre y orígenes de renombrado), no hay conflicto. Ahorra ≥ 60 % de los pares (⚠️ **ASSUMPTION**) sin falsos negativos | Un par con conflicto real que el prefiltro marca "sin conflicto" (E6) |
| H7 | **Hunks**: los rangos de líneas extraídos del blob fusionado con marcadores ampliados, **llevados a las coordenadas de la versión de cada lado** (ADR-CKP-001 § 3; el blob fusionado no las da), coinciden con los de `merge-ort` en ≥ 95 % de los archivos con conflicto de contenido y nunca confunden una línea que imita un marcador | Diferencias en los rangos o un falso marcador (E7) |
| H8 | **Coste**: con 2 trabajos en paralelo y QoS *utility*, la ráfaga de predicciones no empeora el p95 del motor (≤ 300 ms, ADR-GRP-011) fuera del ruido medido, y la memoria residente por trabajo cabe en 256 MB (⚠️ **ASSUMPTION**). Con A2-W, la memoria del trabajador cuenta en la huella del daemon de ADR-GRP-015 | Regresión del p95 del motor o pico de memoria por encima de la cota (E9) |
| H9 | **Versión**: con `gix` 0.88.0 el caso de F2 da un falso negativo y con 0.89.0 no; el resto del corpus da el mismo resultado | Diferencias entre versiones fuera de F2 (E6) |

## 4. Experimentos

**Repos.** Todos en un directorio temporal fuera de cualquier repo:

- **R-H**: el perfil `H` de `repogen` (100K commits, 6.000 archivos, determinista; `crates/testkit/src/repogen.rs:87`), el mismo de INF-GRP-002 (Dev Spec, D2). Se genera una vez con el banco (`ENGINE_BENCH_ROOT=<tmp> cargo bench -p gitraptor-cli --bench engine -- --keep`) y cada corrida trabaja sobre un clon nuevo con **10 worktrees** y agentes simulados que commitean en ramas propias.
- **R-SYN**: corpus sintético de pares con conflicto conocido (§ E6).
- **R-REAL**: clon temporal de `git/git` (⚠️ **ASSUMPTION** de SPIKE-CKP-001, se mantiene: historia de merges larga y pública). Se reejecutan sus últimos **2.000 merges de dos padres**.
- **R-CAN**: repo canario de `crates/testkit/src/canary.rs`, ampliado con `merge.<x>.driver`, `filter.<x>.process/clean`, `.gitattributes` con `merge=<x>` y `filter=<x>` en el disco, en el índice y en el árbol, `core.fsmonitor`, `refs/replace/*` e `info/grafts`.
- **R-PC**: *partial clone* (`--filter=blob:none`) de R-SYN con un remoto *promisor* local (`file://`) y blobs ausentes en los pares.
- **R-HOS**: repo hostil: un blob de 100 MB en conflicto, 10K archivos en conflicto, árbol de 200 niveles y 5.000 renombrados en un lado.

**Versiones de Git**: 2.38.5 (mínima, NFR-07), 2.50.1 (Apple) y 2.56.0 (última estable), igual que SPIKE-GRD-001. Se usan para B y para la referencia de fidelidad.

**Referencia de fidelidad**: `git merge-tree --write-tree --name-only --messages -z` (motor `merge-ort`, el mismo de `git merge` por defecto) ejecutado en un **clon temporal desechable**, con la configuración global y de sistema aisladas y el **mismo límite de renombrados** que A2. En una muestra de 100 pares se comprueba que `git merge` da el mismo conjunto de archivos que `merge-tree`, para usar `merge-tree` como referencia sin perder equivalencia.

| Id | Experimento | Alternativas | Qué se mide | Muestras |
|---|---|---|---|---|
| **E1** | **Repo intacto** con el arnés de INF-GRP-001 (`crates/testkit/src/control.rs` y `fingerprint.rs`) y ejecución de control: R-H con los 55 pares, recálculos tras commits y tras mover la base, `gc` del usuario concurrente y un repo con `alternates` propios | A2, A2-W, B | Diferencias en la huella del directorio común, `.git/worktrees/*` y cada working tree, **con mtime** de packs, objetos sueltos y directorios de `objects/` | 3 corridas completas por alternativa y versión de Git |
| **E2** | **Cero ejecución** en R-CAN, con auditoría de `exec` (eslogger, `crates/testkit/src/exec_audit.rs`) | A1 (control), A2, A2-W, B | Marcadores del canario; procesos hijo (A2: 0; A2-W: solo el trabajador; B: solo `git merge-tree` con argv fijo) | 1 corrida por caso del canario y versión |
| **E3** | **Sin descargas (M-02)** en R-PC con captura de red | A2, A2-W, B (2.38, 2.50 y 2.56) | Resultado "no calculable (objeto ausente)", 0 conexiones y 0 objetos nuevos en el clon. Con B, además, si `protocol.allow=never` basta en 2.38 o hace falta `--no-lazy-fetch` (≥ 2.45) | 1 corrida por alternativa y versión |
| **E4** | **Mismos objetos y cero atributos (L-04)**: `refs/replace/*` que cambia un commit del par, `info/grafts`, y `merge=<x>` por las tres vías | A2, A2-W, B | El resultado es el de los objetos originales y el atributo se trata como el driver de texto. Incluye la prueba en ejecución de la posible inversión de F8, con y sin `core.useReplaceRefs` | 1 por caso |
| **E5** | **Barrera de API**: un test del prototipo que falla si el módulo de A2 llama a `merge_trees`, `merge_commits` o `merge_resource_cache` | A2 | Que la variante segura es la única ruta | — |
| **E6** | **Fidelidad**: R-SYN (misma función y líneas contiguas, añadido/añadido, modificado/borrado, renombrado con modificación, renombrado/renombrado, renombrado de directorio, directorio/archivo, binario, submódulo, punteros LFS, historia cruzada con varias merge-bases, CRLF, `merge=union`, `-merge`, el caso `a/b` → `a/b/` de F2 y la demo del BRD § 13) y R-REAL | A2 (0.88.0 y 0.89.0), B | Paridad del conjunto de archivos en conflicto; falsos positivos y negativos **por tipo**; efecto del prefiltro (H6) sobre los falsos negativos; diferencias entre versiones de `gix` (H9) | Todos los pares de R-SYN; 2.000 de R-REAL |
| **E7** | **Hunks**: rangos de líneas desde el blob fusionado con `marker_size_multiplier` ampliado frente a los rangos de `merge-ort`, con archivos que contienen líneas que imitan marcadores | A2 | Coincidencia de rangos por archivo; falsos marcadores | Los archivos con conflicto de contenido de E6 |
| **E8** | **Rendimiento** en R-H con 10 worktrees | A2, A2-W; B solo si pasó E1 a E4 | Por par, según el delta (1, 10, 100 y 1.000 archivos por lado): p50, p95, p99 y máximo. **Recálculo tras un commit** (fin del commit → último par afectado calculado), sin y con prefiltro. **Cálculo inicial** de los 55 pares y **movimiento de la base**: tiempo hasta salir de "calculando". **Ráfaga**: 10 agentes que commitean cada 2 a 10 s durante 10 min: profundidad de la cola, trabajos descartados por entradas viejas y antigüedad máxima publicada | ≥ 200 por escenario, descartando las 10 primeras (ADR-GRP-011 § 4) |
| **E9** | **Coste** durante la ráfaga de E8 | A2, A2-W | CPU (% de un núcleo, media y pico) y memoria residente máxima por trabajo y del proceso; **p95 del motor** (`t0` → `t_client_recv`) con el banco de INF-GRP-002 (`crates/testkit/src/freshness.rs`), con concurrencia 1 y 2 y QoS *utility* | Las mismas de E8 |
| **E10** | **Cotas (M-06)** en R-HOS | A2 con cotas previas; **A2 con plazo en el acceso a objetos**; A2-W | A2: si cada par termina en ≤ 2 s y bajo la cota de memoria **solo** con el tope de tamaño por cabecera, `rewrites.limit` fijo y `fail_on_conflict` tras el tope de archivos. **A2 con plazo**: `gix_merge::tree` recibe `objects` como parámetro; un envoltorio que devuelve error al vencer el plazo da un punto de corte que F3 no contempla, si todos los accesos pasan por él (se confirma en el código del tag antes de medir). A2-W: que el temporizador mata y relanza el trabajador a los 2 s, que `RLIMIT_CPU` corta y el coste por par de la ida y vuelta. En macOS `RLIMIT_AS` es un alias de `RLIMIT_RSS` y no se impone (⚠️ a confirmar): se mide que el daemon vigile la memoria residente del hijo y lo mate al pasar la cota | 20 por caso hostil |
| **E11** | **Solape**: mantener y cortar los conjuntos de rutas sin commitear con 10 worktrees y una ráfaga de 10K archivos en uno | — (independiente del mecanismo) | Coste por intersección y memoria; valor del tope por worktree (⚠️ **ASSUMPTION** de ADR-CKP-001: 10.000) | 200 |

## 5. Criterios de decisión

Se aplican en orden. Un criterio **eliminatorio** saca a la alternativa aunque gane en todo lo demás.

| Orden | Criterio | Tipo | Umbral |
|---|---|---|---|
| 1 | Repo intacto (E1) | Eliminatorio | 0 diferencias imputables, mtime incluido |
| 2 | Cero ejecución (E2) | Eliminatorio | 0 marcadores; procesos hijo según la alternativa |
| 3 | Sin descargas (E3) | Eliminatorio | 0 conexiones y "no calculable (objeto ausente)" |
| 4 | Mismos objetos y cero atributos (E4) | Eliminatorio | Resultado de los objetos originales; ningún driver |
| 5 | Fidelidad (E6) | Umbral | Paridad ≥ 95 % sobre los pares con conflicto en alguno de los dos motores (o el umbral corregido con datos y aceptado por el PO) y **0 falsos negativos en la demo del BRD § 13** |
| 6 | Frescura (E8) | Umbral | Recálculo tras un commit ≤ 5 s p95 |
| 7 | Coste (E9) | Umbral | p95 del motor sin regresión fuera del ruido; memoria por trabajo ≤ cota |
| 8 | Cotas (E10) | Decide **dónde** corre A2 | Si A2 cumple las cotas dentro del proceso → daemon. Si no → A2-W |

**Árbol de salida** (cada salida elige una rama de la Dev Spec y cierra su gap G1):

- **S1 — A2 pasa 1 a 7 y E10 dentro del proceso** → predictor en el daemon, sin procesos hijo. Rama **R-A2** de la Dev Spec.
- **S2 — A2 pasa 1 a 7 y E10 solo con A2-W** → predictor con proceso trabajador. Se aplica la enmienda condicionada de ADR-GRP-009 Validación 5 (el propio binario como módulo autorizado para lanzar procesos). Rama **R-A2W**. **Es el resultado que este plan considera más probable** por F3.
- **S3 — A2 falla en fidelidad o en un eliminatorio, y B pasa 1 a 4** → B, con proceso por par en 2.38 y lote con `--stdin` en ≥ 2.39. Se aplican las enmiendas de la columna (b) de ADR-CKP-001. Rama **R-B**. Por F11 se espera que B no pase el criterio 1.
- **S3b — A2 pasa 1 a 5 pero no cumple los 5 s, y B sí** → B (la tabla de alternativas de ADR-CKP-001 lo permite: "solo si (a) falla en fidelidad o rendimiento"). Rama **R-B**.
- **S4 — A2 falla en fidelidad y B no pasa 1 a 4** → se mantiene A2 y las diferencias medidas pasan a límites declarados (ADR-CKP-001 § 7), con revisión del KPI del 70 % por el PO. Rama R-A2 o R-A2W según E10.
- **S5 — ninguna cumple los 5 s** → no elige rama: se revisa el objetivo de frescura con el PO (Q-CKP-6) antes que el mecanismo, con la antigüedad visible como mitigación, y la rama sale de los demás criterios.
- **S6 — ninguna deja el repo intacto** → la predicción se reduce a solape y el ⚡ queda "no disponible" hasta un ADR nuevo. Rama **R-SOLAPE**.

## 6. Prototipo mínimo

`spikes/conflict-prediction/`, un workspace de Cargo propio (no es miembro del de GitRaptor) con un binario `ckp-spike`:

| Pieza | Qué hace | Tamaño orientativo |
|---|---|---|
| `Cargo.toml` | `gix = "=0.89.0"` (y una feature `gix088` que fija `=0.88.0` para H9) con `default-features = false` y las features `sha1`, `merge`, `revision` y `max-performance-safe`; `gitraptor-testkit` por ruta (`../../crates/testkit`), que solo depende de `serde_json` y `tempfile`; `libc` para `rlimits` y QoS | — |
| `src/open.rs` | Apertura como `RepoReader::open` (`crates/git/src/reader.rs:160`): `bail_if_untrusted`, `Permissions` aisladas (`git_binary = false`, `config.system = false`, `attributes.system = false`, F9), `objects.ignore_replacements = true` y `with_object_memory()` por trabajo | ~80 líneas |
| `src/a2.rs` | Merge-base (virtual si hay varias) y `gix_merge::tree(...)` con `blob::Platform::new(<pipeline sin drivers>, Mode::ToGit, <pila de atributos vacía>, Vec::new(), opts)`, `rewrites.limit` fijo, `marker_size_multiplier` ampliado, tope de tamaño por cabecera antes de cargar cada blob, y salida `{archivos, tipo, hunks}` | ~250 líneas |
| `src/hunks.rs` | Parser de rangos de líneas desde el blob fusionado (F6) | ~100 líneas |
| `src/worker.rs` | A2-W: `ckp-spike worker` lee trabajos por stdin y responde por stdout; el lado cliente aplica `setrlimit`, el temporizador de 2 s y el relanzamiento | ~150 líneas |
| `src/b.rs` | B: almacén *bare* temporal con `alternates`, argv fijo de ADR-CKP-001 § 1 (b), entorno vacío, `--stdin` en ≥ 2.39 | ~120 líneas |
| `src/prefilter.rs` | Prefiltro de H6 sobre rutas de `changed_paths` | ~60 líneas |
| `src/bench.rs` | Escenarios E8 a E11, con resumen p50/p95/p99/máx | ~200 líneas |
| `suites/*.sh` | E1 a E7 en macOS, con las tres versiones de Git, como `spikes/hook-interceptability/` | — |
| `README.md` | Procedimiento reproducible, también para Linux y Windows | — |

Reglas del prototipo: nunca `merge_trees` ni `merge_commits` en `a2.rs` (E5); ningún `unwrap` sobre datos del repo; la evidencia de cada corrida va a `results/macos-git<versión>/` en TSV, como SPIKE-GRD-001. Si la dependencia por ruta de `gitraptor-testkit` no compila fuera del workspace (herencia `workspace = true`), el prototipo copia `fingerprint.rs` y `control.rs` sin cambios y lo dice en el README.

## 7. Salida hacia ADR-CKP-001 y la Dev Spec

`research/SPIKE-CKP-001-resultados.md` responde, con una fila de evidencia por afirmación:

1. Mecanismo elegido (S1 a S6) y rama de la Dev Spec.
2. Dónde corre el merge en seco (daemon o A2-W) y si se aplica la enmienda condicionada de ADR-GRP-009 Validación 5.
3. Cifras: tope de rutas sin commitear por worktree, topes de archivos y hunks por par, tope de tamaño de blob, `rewrites.limit`, `marker_size_multiplier`, concurrencia, tiempo y memoria por par.
4. Si el prefiltro se mantiene.
5. Umbral de fidelidad confirmado o corregido, con los límites declarados que añade.
6. Si el objetivo de 5 s pasa a gate en INF-GRP-002.
7. Versión mínima de `gix` (0.89.0 si se confirma H9) y si la subida del workspace es requisito previo de TS-CKP-001.
8. Las correcciones a ADR-CKP-001 que ya trae este plan: F1 (versiones), F3 ("bandera de interrupción" inexistente), F4 (A1 descartada) y F10 (versiones de `merge-tree` en § 11).

## 8. Time-box

1,5 semanas en macOS (⚠️ **ASSUMPTION** de SPIKE-CKP-001, se mantiene): 2 días para E1 a E5 (eliminatorios, primero, porque pueden cerrar B y A1 pronto), 3 días para E6 y E7, 2 días para E8 a E11 y 0,5 días para el informe. Si E1 a E4 descartan B el primer día, el tiempo de B pasa a E6.

## 9. Decisiones de este plan

**Decisiones del orquestador (2026-10-09), validadas por Arquitecto** (`nassa-architect:architect`):

- **P1**: A1 (`merge_trees` de alto nivel) se descarta sin medir por F4 y se usa solo como control del canario. A2 es la forma concreta de la opción (a) de ADR-CKP-001.
- **P2**: A2-W se mide siempre, no solo si A2 falla, porque F3 hace probable que M-06 no se demuestre dentro del proceso.
- **P3**: B se mide primero en lo eliminatorio; su rendimiento y su fidelidad solo si pasa E1 a E4.
- **P4**: la referencia de fidelidad es `git merge-tree --write-tree` en un clon desechable, contrastada con `git merge` en una muestra.
- **P5**: el prototipo fija `gix` 0.89.0 y mide 0.88.0 para cuantificar F2. La subida del workspace a 0.89.0 mueve también `gix-pack` (fijado junto a `gix`, lo usa la Time Machine): va en un PR `chore/` propio, con la feature `merge` solo en `crates/git`, antes de TS-CKP-001 (DS-TS-CKP-001, G3).
- **P6** (propuesta del Arquitecto): E10 mide también "A2 con plazo en el acceso a objetos" y, para A2-W, la vigilancia de la memoria residente del hijo, porque `RLIMIT_AS` no se impone en macOS.
- **P7** (propuesta del Arquitecto): la paridad se mide sobre los pares con conflicto en alguno de los dos motores, y la referencia corre con la configuración aislada y el mismo límite de renombrados.

**Validación del Arquitecto (2026-10-09)**: de acuerdo con P3 y con H1 a H9; de acuerdo con ajuste con P1, P2, P4, P5, el § 5 y el árbol de salida. Los ajustes están aplicados en este documento (fila A1, E10, H3, H7, H8, referencia de fidelidad, S3, S3b y S5).
