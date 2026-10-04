# Resultados de SPIKE-GRP-002: viabilidad del observador de cambios a escala

> **Research Brief** de [SPIKE-GRP-002](../technical-stories/SPIKE-GRP-002-viabilidad-observador.md) · Fecha: 2026-10-04 · Valida: ADR-GRP-010 y ADR-GRP-011 · Relacionado: ADR-GRP-006, ADR-GRP-009, INF-GRP-002, US-GRP-002 a US-GRP-005.

> **Alcance de esta entrega**: medido **solo en macOS**. Linux y Windows están **sin verificar**; el procedimiento para medirlos está en el [README del prototipo](../../../../../spikes/watcher-viability/README.md#reproducir-en-linux-y-windows). Las recomendaciones del § 6 se **aplicaron el 2026-10-04** como enmiendas de ADR-GRP-010 y ADR-GRP-011.

## 1. Resumen

En macOS, el diseño de ADR-GRP-010 **cumple el presupuesto del motor con holgura**. El p95 de extremo a extremo (de `t0` a `t_client_recv`) va de 12 a 136 ms según el escenario, frente a 300 ms, y el máximo en 1.800 muestras fue de 175 ms. Hay cuatro hallazgos que piden enmienda:

1. **La ventana de 75 ms dura en realidad unos 85 ms**: el temporizador de macOS se despierta tarde, entre 7 y 10 ms. La ventana fija sí acota la espera durante una ráfaga (hipótesis confirmada).
2. **Con `notify` 8.2, cada alta o baja de un watch recrea el stream de FSEvents y pierde eventos en silencio**: entre 11 y 17 archivos en 40 recreaciones, sin ninguna marca de "rescan". Como dar de alta o de baja un worktree hace `watch`/`unwatch`, cada alta o baja abre un hueco silencioso en todos los worktrees.
3. **El sondeo de respaldo de 30 s no recupera los cambios perdidos que solo tocan el working tree** (4 de 7 en el escenario de huecos). Solo la reconciliación completa los recupera todos.
4. **Ahead/behind sin `commit-graph`** con una rama a 50K commits de la base cuesta 145 ms p50 (162 ms p95), casi todo el presupuesto de recomputo.

La reconciliación recuperó el **100%** de los cambios en los huecos simulados. El repo quedó **intacto**: 52.697 entradas idénticas antes y después.

## 2. Método

### Máquina y entorno

| Dato | Valor |
|---|---|
| Máquina | Apple M5, 10 núcleos, 32 GiB, SSD interno (APFS) |
| SO | macOS 26.6.2 |
| Git | 2.50.1 (Apple Git-155) |
| Crates | `notify` 8.2.0 (FSEvents), `gix` 0.88, `rusqlite` 0.40 (SQLite embebido) |
| Corridas | Dos corridas completas e independientes (`run1` y `run2`). Las cifras son de `run2`; entre paréntesis, `run1` cuando difiere |

### Prototipo

Vive en [`spikes/watcher-viability/`](../../../../../spikes/watcher-viability/): un workspace de Cargo propio, fuera de `crates/` y `apps/`. Se reejecuta con un solo comando desde la raíz del repo:

```sh
cargo run --release --manifest-path spikes/watcher-viability/Cargo.toml -- --out spikes/watcher-viability/results
```

Reproduce el diseño de ADR-GRP-010:

- **Watcher**: uno solo, `notify` recursivo sobre la raíz de cada worktree. La raíz del principal cubre `.git/`. Los eventos de `.git/objects/` y de rutas ignoradas se descartan antes del debounce.
- **Debounce**: ventana por worktree, fija (configurable) o deslizante para comparar.
- **Recomputo incremental**: índice leído con `gix`; comparación por stat y hash del contenido solo cuando el stat difiere, con caché de stat en memoria; árbol de `HEAD` con `gix` para lo preparado; ahead/behind con `git rev-list --left-right --count` (allowlist de ADR-GRP-009) solo cuando cambia una de las puntas.
- **Persistencia**: SQLite en WAL con `synchronous=FULL` y `fullfsync=ON`, una transacción por lote, en un perfil temporal fuera del repo.
- **Publicación**: socket Unix hacia un suscriptor del mismo proceso.
- **Sondeo, reconciliación y alta y baja de worktrees**: sondeo de respaldo por huella, reconciliación completa, y altas y bajas con validación bidireccional de `gitdir`.

### Repos sintéticos

Se generan en un directorio temporal con `git fast-import` y se borran al terminar (NFR-01):

- Historia: 100.000 commits.
- Árbol: 5.000 archivos en 250 directorios, con `.gitignore` para `target/` y `node_modules/`.
- `commit-graph` escrito.
- 10 worktrees: el principal y 9 enlazados.

### Marcas y medición

Las marcas son las de ADR-GRP-011 § 2, tomadas con el reloj monótono del proceso.

- **`t0`**: fin de la escritura del archivo o fin del comando de Git.
- **Escenario de latencia**: la muestra termina con la **primera publicación cuyo estado refleja el resultado final**: el oid del contenido escrito, la firma del `index`, el oid de `HEAD`, el alta o la baja del worktree.
- **Muestras**: 200 por escenario. Crear y borrar un worktree: 50 cada uno.
- **Operaciones de Git**: Git escribe `index`, refs y `HEAD` antes de terminar, así que el lote suele abrirse **antes** de `t0`. En esos escenarios la detección SO → motor no se puede aislar: se reporta el total desde que el comando termina, que es lo que percibe el usuario. La detección pura se mide en "modificar archivo".

### Límites del método

- El estado del worktree es una implementación propia sobre `gix`, no `gix status`.
- No hay cliente en otro proceso ni TUI, así que no se mide `t_render`.
- La CPU del motor se calcula como la CPU del proceso menos la CPU del hilo escritor, y es ruidosa.
- La memoria es el RSS de todo el proceso del banco.

## 3. Respuestas a las preguntas del SPIKE

### 3.1 Latencia por etapa (10 worktrees, 100K commits)

p95 en ms (`run2`). El total va de `t0` a `t_client_recv`.

| Escenario | Detección | Debounce | Recomputo | Persistencia | Recomputo + persistencia | Publicación | **Total p95** | Total p99 | Total máx. |
|---|---|---|---|---|---|---|---|---|---|
| Modificar archivo | 12,2 | 84,9 | 0,1 | 9,8 | 9,8 | 0,0 | **104,0** (101,5) | 105,7 | 107,3 |
| `git add` | n/a¹ | 85,0 | 11,7 | 14,7 | 24,5 | 0,1 | **109,9** (108,1) | 117,4 | 133,7 |
| Commit | n/a¹ | 85,0 | 48,3 | 10,1 | 53,3 | 0,1 | **133,6** (135,8) | 136,7 | 144,2 (174,7) |
| Checkout (rama 50 commits atrás) | n/a¹ | 85,0 | 33,7 | 9,3 | 41,2 | 0,0 | **111,6** (120,2) | 114,8 | 119,2 |
| Crear worktree | n/a¹ | —² | 49,0 | 14,7 | 59,4 | 0,0 | **63,2** (65,0) | 72,8 | 72,8 |
| Borrar worktree | n/a¹ | —² | 5,9 | — | 5,9 | 0,0 | **12,2** (10,3) | 13,4 | 13,4 |

¹ Los eventos llegan antes de que termine el comando: 154 de 200 en `git add` y 200 de 200 en commit y checkout.
² El alta y la baja no pasan por la ventana: el alta hace una reconciliación completa del worktree nuevo y la baja publica directamente.

- **Detección (FSEvents)**: mínimo 0,17 ms (0,07); p50 11,3 ms (2,4); p95 12,2 ms (12,1). Dentro de ≤ 50 ms.
- **Recomputo + persistencia**: p95 ≤ 59 ms en todos los escenarios. El máximo observado fue 115 ms (`run1`, commit). Dentro de ≤ 150 ms.
- **Publicación** (`t_persisted` → `t_client_recv`): ≤ 0,1 ms p95. Dentro de ≤ 25 ms, aunque el suscriptor está en el mismo proceso: no mide un cliente en otro proceso.
- **Total**: ningún escenario supera los 300 ms en ninguna muestra. Fueron 900 muestras por corrida y ninguna agotó el tiempo de espera.

### 3.2 Debounce de 75 ms

**Ráfaga**: 2.000 archivos nuevos en 2 s en un worktree (`run2`). Los tiempos van en ms.

| Ventana | Primer cambio visible | Estado final tras la última escritura | Recomputos | Espera máxima sin publicar | CPU del motor³ |
|---|---|---|---|---|---|
| Sin debounce (0) | 5 | 8 | 453 | 11 | 184 (485) |
| Fija 25 ms | 36 | 11 | 67 | 36 | 126 (164) |
| Fija 50 ms | 60 | 7 | 37 | 61 | 127 (246) |
| **Fija 75 ms** | **83** | **95** | **26** | **98** | **150 (248)** |
| Fija 100 ms | 111 | 96 | 20 | 112 | 114 (276) |
| Fija 150 ms | 161 | 150 | 14 | 161 | 126 (206) |
| Deslizante 75 ms | **2.142** | 149 | **1** | **1.993** | 113 (193) |

³ CPU del proceso menos la del hilo escritor, en ms durante la ráfaga. Es ruidosa entre corridas. Solo "sin debounce" es sistemáticamente más cara.

**Retraso del temporizador**: `recv_timeout`, la primitiva de la ventana, se despierta tarde en macOS. Con 75 ms, el retraso es de 7,1 ms p50, 9,9 ms p95 y 10,0 ms de máximo (`run1`: 8,1 / 10,0 / 10,0). Por eso el debounce medido es de **85 ms p95**, no 75 ms.

**Conclusión**:

- La ventana fija de 75 ms se **confirma** como valor razonable: 26 recomputos en 2 s frente a 453 sin debounce, y una espera acotada a una ventana.
- La deslizante pospone la primera publicación hasta el final de la ráfaga, más de 2 s: se descarta.
- Con 50 ms se ganarían unos 25 ms de latencia con un 40% más de recomputos, y la diferencia de CPU queda dentro del ruido. No hay motivo medido para cambiar el valor. Sí lo hay para presupuestar la etapa como **ventana + retraso del temporizador**.

### 3.3 Interpretación de NFR-04 (p95, p99 y máximo)

En 1.800 muestras (dos corridas), el máximo de extremo a extremo del motor fue de **175 ms**, y el p99 más alto, 140,7 ms. La distancia entre p95 y máximo es de 3 a 40 ms. En esta máquina, leer "< 300 ms" como p95, p99 o máximo no cambia el resultado.

**Recomendación**: mantener **p95 como gate**, por el ruido de los runners compartidos del CI, que aquí no se midió, y reportar p99 y máximo. La interpretación de ADR-GRP-011 § 1 **se confirma en macOS**. En Linux y Windows queda sin verificar.

### 3.4 Escala

**Ráfaga de 10K archivos** en `wt-01` mientras se modifican archivos en los otros nueve:

| Medida | `run2` | `run1` |
|---|---|---|
| Latencia de los otros nueve durante la ráfaga (p50 / p95 / máx.) | 89 / **95** / 96 ms (n = 25) | 88 / **91** / 91 ms (n = 9) |
| Tiempos agotados de los otros nueve | 0 | 0 |
| Estado final del worktree de la ráfaga tras la última escritura | 21 ms | 75 ms |
| Publicaciones del worktree de la ráfaga | 30 | 11 |
| Eventos de `notify` | 32.920 (unos 3,3 por archivo) | 32.887 |
| Desbordamientos (`MustScanSubDirs`, `rescans`) | 0 | 0 |

- **Watches**: FSEvents no consume un watch por directorio. Hay un único stream por proceso y los descriptores del proceso pasan de 4 a 9 con el motor (SQLite, WAL, SHM y el socket), con un pico de 10 a 11 en la ráfaga. El límite de watches **no aplica en macOS**.
- **Memoria**: el RSS del proceso del banco pasa de 70 a 200 MiB al arrancar el motor con 10 worktrees, con un pico de 201 a 206 MiB en la ráfaga. Es una **cota superior**: incluye el banco y la memoria que el asignador retiene de experimentos previos. La diferencia (unos 130 MiB) no se aisló; la causa probable son las cachés de objetos de `gix` (una instancia de repo por worktree). Queda como dato a investigar en TS-GRP-002, no como cifra del motor.
- **CPU en reposo**: 0,04% de un núcleo (`run1`: 0,06%) durante 31 s con 10 worktrees, incluido un ciclo del sondeo de respaldo.
- **Ráfaga de 10K archivos en un directorio ignorado** (`target/`): 33.009 eventos, el 100% filtrados antes del debounce, 0 publicaciones y unos 200 ms de CPU (unos 6 µs por evento).

### 3.5 Sondeo de respaldo y modo degradado (10 worktrees de 5.000 archivos)

| Medida | p50 | p95 | Carga |
|---|---|---|---|
| Ciclo del sondeo de respaldo (huella barata de los 10 worktrees) | 0,34 ms (0,29) | 0,51 ms (0,32) | 0,001% de un núcleo cada 30 s |
| Ciclo del modo degradado (estado completo por stat) de un worktree | 17,6 ms (12,8) | 20,2 ms (17,9) | **8,8% de un núcleo** (6,4%) con los 10 worktrees degradados cada 2 s |

El coste del modo degradado crece de forma lineal con los archivos de cada worktree.

### 3.6 Huecos

| Escenario | Sin recuperación | Tras el sondeo de respaldo | Tras la reconciliación completa |
|---|---|---|---|
| Eventos descartados (simula desbordamiento o suspensión), 7 worktrees con cambios | 0 de 7 correctos | **3 de 7** | **7 de 7** |
| Watcher detenido y recreado, 7 worktrees con cambios | 1 de 7 | — | **7 de 7** |

- **Qué recupera el sondeo**: los cambios que tocan metadatos de Git (commit, checkout, `git add`). Los cuatro que solo tocan el working tree (modificar, crear sin seguimiento, borrar y un directorio nuevo) **no cambian la huella** del sondeo (`HEAD`, `index`, `packed-refs` y marcadores), así que el sondeo no los ve.
- **Comprobación**: en todos los casos, el estado reconciliado se comparó con `git status --porcelain` (con `GIT_OPTIONAL_LOCKS=0`) y con `rev-parse HEAD`: rama, cambios sin preparar y preparados coinciden.

**Recreación del stream de FSEvents**: `notify` 8.2 detiene y vuelve a crear el stream con `kFSEventStreamEventIdSinceNow` en cada `watch()`/`unwatch()`. Con un escritor creando un archivo por milisegundo:

| Modo | Archivos | Recreaciones del stream | Archivos sin evento | Marca de rescan |
|---|---|---|---|---|
| Control (sin recreaciones) | 4.048 (8.463) | 0 | **0** (0) | — |
| Con alta y baja de watches | 4.166 (6.249) | 40 (40) | **11** (17), en 3 de 5 corridas (4 de 5) | **ninguna** |

Es un hueco **silencioso**: nada indica que haya que reconciliar. En el diseño, cada alta o baja de un worktree hace `watch`/`unwatch`, así que **cada alta o baja de un worktree abre un micro-hueco en todos los worktrees del proceso**.

**Desbordamiento forzado con un búfer reducido**: **no verificado en macOS**. FSEvents no expone un tamaño de búfer y ninguna ráfaga de 10K archivos produjo `MustScanSubDirs`. Se sustituyó por la simulación de eventos descartados.

**Suspensión y reanudación**: **no verificado**. No se puede suspender la máquina de forma automática y segura desde el banco, y tampoco se probó la detección por el salto entre el reloj de pared y el monótono. La simulación de eventos descartados cubre la recuperación, no la detección de la reanudación.

### 3.7 Windows

**No verificado**. El prototipo no se ejecutó en Windows. Como referencia, en macOS el experimento `handles` dio **0 fallos** con el watcher activo:

- 10 `git worktree remove`, 10 borrados y 10 renombrados de la raíz.
- 30 renombrados y 30 borrados de archivos.
- Las 30 bajas se publicaron como "removed".

En macOS no hay bloqueo por handles abiertos, así que este resultado **no dice nada** sobre el riesgo de ReadDirectoryChangesW. El procedimiento para medirlo está en el README.

### 3.8 Linux

**No verificado**. Ni el agotamiento de `max_user_watches`, ni el modo degradado sin caída del resto, ni `IN_Q_OVERFLOW` se pudieron medir desde este Mac.

Advertencia para quien lo mida: el prototipo usa el modo recursivo de `notify`, que en Linux registra un watch por directorio, **incluidos los ignorados y `.git/objects`**. El recuento será una cota superior del diseño de ADR-GRP-010 § 2. El prototipo tampoco implementa el paso a degradado por worktree. Procedimiento en el README.

### 3.9 macOS: latencia mínima de FSEvents y fusión de eventos

- **Configuración**: `notify` crea el stream con latencia 0,0 y `kFSEventStreamCreateFlagFileEvents | NoDefer`, la configuración mínima posible. No hace falta ajustarla.
- **Detección**: mínimo 0,07 a 0,17 ms, p95 12,2 ms. Muy por debajo del presupuesto de 50 ms.

**Fusión**: 200 escrituras al mismo archivo.

| Patrón | Eventos recibidos | Publicaciones | Estado final |
|---|---|---|---|
| Bucle sin pausa | 64 (46) | 2 (1) | Correcto |
| Una por ms | 256 (62) | 12 (5) | Correcto |
| Una cada 10 ms | 416 (406) | 36 (34) | Correcto |

FSEvents fusiona eventos, pero el estado final siempre fue el correcto: la fusión no compromete el presupuesto de detección.

### 3.10 Repo intacto

La huella cubre ruta, tamaño, mtime e inodo de todo, y el hash del contenido de los metadatos de Git. Se tomó antes y después de arrancar el motor, dos reconciliaciones completas y ocho ciclos de sondeo: **0 diferencias en 52.697 entradas**, en las dos corridas. La base de datos del perfil vive fuera del repo.

### 3.11 Mediciones de apoyo

**Persistencia**: una transacción SQLite por lote, en WAL. p95 en ms.

| Filas por transacción | `FULL` + `fullfsync=ON` | `FULL` + `fullfsync=OFF` | `NORMAL` |
|---|---|---|---|
| 1 | 8,2 (5,1) | 2,8 (0,1) | 0,1 (0,0) |
| 100 | 8,1 (5,1) | 3,6 (0,1) | 3,1 (0,1) |
| 1.000 | 10,9 (8,1) | 5,7 (0,4) | 5,7 (0,3) |

Dentro del motor, con 10 workers compartiendo una conexión, la persistencia es de 9 a 15 ms p95 y 43 ms de máximo. `F_FULLFSYNC` cuesta unos 4 a 8 ms en este SSD, no "decenas de milisegundos". En Windows queda sin verificar.

**Ahead/behind en 100K commits**: `git rev-list --left-right --count`, p50 / p95 en ms.

| Caso | Con `commit-graph` | Sin `commit-graph` |
|---|---|---|
| Punta cercana | 15,7 / 18,5 | 16,9 / 20,4 |
| Rama a 50K commits de la base | 26,1 / 26,7 | **144,8 / 162,0** (133,1 / 137,2) |

En la punta cercana, el coste es casi todo el arranque del proceso, unos 15 ms.

## 4. Hipótesis del SPIKE

| Hipótesis | Resultado en macOS | Dato |
|---|---|---|
| Detección ≤ 50 ms p95 | ✅ Confirmada | 12,2 ms |
| Debounce de 75 ms | ⚠️ **Corregida** | 85 ms p95: 75 ms de ventana más 10 ms de retraso del temporizador |
| Recomputo y persistencia ≤ 150 ms p95 | ✅ Confirmada, con matiz | ≤ 59 ms p95 y 115 ms de máximo. Riesgo: ahead/behind sin `commit-graph` llega a 145 ms p50 |
| Publicación ≤ 25 ms p95 | ✅ Confirmada (en el mismo proceso) | ≤ 0,1 ms |
| Total ≤ 300 ms p95 | ✅ Confirmada | ≤ 136 ms p95 y 175 ms de máximo |
| La ventana fija acota la espera durante una ráfaga continua | ✅ Confirmada | Fija: primera publicación a 83 ms y espera máxima de 98 ms. Deslizante: 2.142 ms |
| Persistir antes de publicar cabe aunque el `fsync` cueste decenas de ms | ✅ Cabe; ❌ el coste supuesto no se observa | `fullfsync` ≤ 11 ms p95 |
| Sin refrescar el índice, la caché de stat mantiene el recomputo en presupuesto | ✅ Confirmada (5.000 archivos por worktree) | Incremental 0,1 ms; tras cambio del índice ≤ 53 ms p95. Sin medir: worktrees de más de 5.000 archivos |
| 100% de los cambios recuperados por reconciliación | ✅ Confirmada | 7 de 7 en los huecos simulados. ⚠️ El sondeo solo recupera 3 de 7, y la recreación del stream pierde eventos sin avisar |

## 5. Criterios de éxito

| Criterio | Estado |
|---|---|
| Cifras de ADR-GRP-011 § 2 en p95 en los tres SO | **macOS: sí**, con el debounce corregido a ventana más retraso del temporizador. **Linux y Windows: sin verificar** |
| 100% de los cambios recuperados por reconciliación en los huecos | **macOS: sí** cuando se dispara la reconciliación. El disparo no está garantizado en dos casos: la recreación del stream y los cambios del working tree con eventos perdidos sin marca (§ 3.6) |
| Ningún fallo de borrado o movimiento atribuible al watcher en Windows | **Sin verificar** |

**El SPIKE queda parcialmente cerrado**: la vía de fracaso no se activa en macOS, y para Linux y Windows hay que ejecutar el procedimiento del README.

## 6. Recomendaciones de enmienda (aplicadas el 2026-10-04)

En la fila de debounce se eligió la ventana compensada (duración efectiva de 75 ms) en lugar de presupuestar 85 ms, para que las etapas de ADR-GRP-011 § 2 sigan sumando 300 ms. Para el sondeo de respaldo se eligió depender de los disparadores de reconciliación, con la reconciliación tras cada recreación del stream como disparador nuevo. La reconciliación periódica queda como opción si el dogfooding la pide.

### ADR-GRP-011

1. **§ 2, fila Debounce**: presupuestar la etapa como "ventana fija de 75 ms + retraso del temporizador (≤ 10 ms p95 medido en macOS)", es decir, 85 ms p95. Otra opción es implementar la ventana con compensación (despertar en `W − holgura` y esperar activamente el resto) para que dure 75 ms. El total no cambia de veredicto.
2. **§ 1, interpretación de NFR-04**: confirmar p95 como gate en macOS y añadir p99 y máximo como métricas reportadas. Dejar la confirmación en Linux y Windows pendiente de su medición.
3. **§ 4, banco de INF-GRP-002**:
   - En los escenarios de Git, `t0` es el fin del comando y los eventos llegan antes; la etapa de detección solo se puede aislar en "modificar archivo".
   - Medir la fila de debounce con la holgura del temporizador.
4. **Consecuencias, persistencia**: sustituir "decenas de milisegundos" por la cifra medida en macOS (≤ 11 ms p95 con `F_FULLFSYNC`) y mantener la medición pendiente en Windows.

### ADR-GRP-010

1. **§ 1 y § 6, recreación del stream de FSEvents**:
   - Registrar que, con `notify` 8.2, cada `watch`/`unwatch` recrea el stream "desde ahora" y pierde eventos sin marca de rescan.
   - Añadir a "Cuándo se reconcilia" la **reconciliación tras cada recreación del stream**, o evitar la recreación: un único stream sobre un ancestro estable, o una integración propia con FSEvents que reanude desde el último `FSEventStreamEventId`.
   - Agrupar las altas y bajas con `paths_mut()` para recrear menos.
2. **§ 5, alcance del sondeo de respaldo**: decir explícitamente que la huella solo detecta cambios de metadatos de Git, no cambios del working tree perdidos. Opciones:
   - aceptarlo y depender de los disparadores de reconciliación;
   - añadir una reconciliación completa periódica de baja frecuencia (unos 18 ms por worktree de 5.000 archivos).
3. **§ 4, ahead/behind**: sin `commit-graph`, una rama lejana de la base cuesta de 145 a 160 ms. Hay que calcularlo fuera del presupuesto de 150 ms, en la segunda fase de publicación y con caché, o en proceso con `gix`, y no lanzar un proceso por cada cambio de punta.
4. **§ 2, tope de watches por repo**: no se puede fijar desde macOS, porque FSEvents no consume watches por directorio. Sigue como ⚠️ ASSUMPTION hasta medir en Linux.
5. **§ 5, coste del modo degradado**: 17,6 ms por worktree de 5.000 archivos (8,8% de un núcleo con 10 worktrees cada 2 s). Conviene registrarlo y valorar un intervalo adaptativo por número de archivos.

### Fuera de los ADR

- **Memoria con 10 worktrees** (unos 130 MiB de RSS en el prototipo): investigarla en TS-GRP-002 (cachés de `gix`) antes de fijar un presupuesto.

## 7. Pendiente

- Ejecutar el procedimiento del README en **Linux**: agotamiento de `max_user_watches`, `IN_Q_OVERFLOW` y watches solo de directorios no ignorados.
- Ejecutarlo en **Windows**: `handles` y la latencia con ReadDirectoryChangesW.
- Medir la **suspensión y reanudación** reales de forma manual.
- Medir un **cliente en otro proceso**, y `t_render` cuando exista el Cockpit (INF-GRP-002).
- Medir **worktrees con muchos más de 5.000 archivos** y el **RSS aislado** del motor.

## Evidencia

- Datos crudos: [`results-macos-run1.json`](../../../../../spikes/watcher-viability/results/results-macos-run1.json) y [`results-macos-run2.json`](../../../../../spikes/watcher-viability/results/results-macos-run2.json). `run1` es anterior al experimento `handles`.
- Prototipo y procedimiento: [`spikes/watcher-viability/README.md`](../../../../../spikes/watcher-viability/README.md).
