---
id: ADR-GRP-010
title: Observación de cambios en worktrees
type: adr
status: accepted
accepted: 2026-10-04
date: 2026-10-03
created: 2026-10-03
updated: 2026-10-04
deciders: [Rene Bonilla]
domain: GRP
feature: motor-local
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-007, ADR-GRP-009, ADR-GRP-011, ADR-GRP-012, ADR-GRP-013, ADR-GRD-005, SPIKE-GRP-002, INF-GRP-002, CTX-GRP-001, BR-GRP-001]
tags: [watcher, notify, fsevents, inotify, readdirectorychangesw, debounce, reconciliacion, sondeo, worktrees, nfr-04, nfr-05, br-cons-005, br-edge-005, seguridad]
---

# ADR-GRP-010 — Observación de cambios en worktrees

> **Estado**: aceptado por Rene Bonilla el 2026-10-04.

## Contexto

El motor tiene que reflejar el estado de cada worktree (rama, cambios sin commitear, ahead/behind, operaciones en curso, BR-EDGE-002) con tres exigencias que tiran en direcciones distintas:

- **Frescura**: la TUI refleja un cambio en menos de 500 ms (NFR-04); al motor le tocan 300 ms de ese presupuesto (ADR-GRP-011).
- **Escala**: 10 o más worktrees activos y repos de más de 100K commits sin degradarse (NFR-05), con agentes que generan ráfagas (instalar dependencias, compilar, reescribir cientos de archivos) y que crean y borran worktrees constantemente (BR-EDGE-001).
- **Sin huecos**: la observación es continua aunque no haya ninguna superficie abierta (BR-CONS-005, Q1, Q6). Si aun así hay un hueco, se reconcilia el estado actual y lo no visto queda "sin atribuir" (BR-EDGE-005, Q25, Q26).

Restricciones: el motor es un proceso en segundo plano por usuario (ADR-GRP-005); no escribe nada en el repo, ni siquiera transitoriamente, y no ejecuta programas del usuario (ADR-GRP-009); no instala hooks (Q22); no modifica la máquina fuera del perfil (Q17), lo que incluye los límites del kernel. Los repos en WSL quedan fuera (Q8). Con Git por debajo de 2.38 no se observa nada (Q28).

En un repo con worktrees, el directorio Git común (`.git/`) guarda refs, objetos y `packed-refs`, y cada worktree enlazado tiene su `HEAD`, su `index` y sus marcadores de operación en `.git/worktrees/<nombre>/`. El working tree de un worktree enlazado puede estar en cualquier ruta del disco (p. ej. `~/orca/workspaces/...`).

## Decisión

Recomendación aceptada por Rene Bonilla el 2026-10-03 (índice de ADRs, opción 1). El observador vive en `crates/core` y lee a través de la capa de solo lectura de `crates/git` (ADR-GRP-009).

### 1. Mecanismo

- Crate `notify` sobre las APIs nativas: **FSEvents** en macOS, **inotify** en Linux y **ReadDirectoryChangesW** en Windows. Un único watcher compartido por el proceso (en Linux, una sola instancia de inotify, que respeta `max_user_instances`).
- **Recreación del stream en macOS** (Enmienda 2026-10-04): con `notify` 8.2, cada `watch()` o `unwatch()` detiene y vuelve a crear el stream de FSEvents "desde ahora", y los eventos de ese intervalo se pierden **sin marca de rescan**. SPIKE-GRP-002 midió de 11 a 17 archivos perdidos en 40 recreaciones, con un archivo nuevo por milisegundo. Por eso:
  - las altas y bajas de watches se agrupan (`paths_mut()`) para recrear el stream una sola vez por lote;
  - cada recreación dispara una reconciliación de todos los worktrees del stream (apartado 6).
  Reanudar el stream desde el último `FSEventStreamEventId`, con una integración propia con FSEvents, queda como optimización, no como requisito.
- El watcher solo abre handles de lectura o de notificación; nunca escribe ni crea archivos en el repo (ADR-GRP-009). En Windows, los handles de directorio se abren con borrado compartido.

### 2. Qué se vigila

| Ruta | Profundidad | Para qué |
|---|---|---|
| Working tree de cada worktree (raíz) | Recursiva, sin directorios ignorados en Linux | Cambios sin commitear |
| `.git/` común: `HEAD`, `index`, `packed-refs`, `ORIG_HEAD`, `MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `BISECT_LOG`, `rebase-merge/`, `rebase-apply/` | Archivos y marcadores concretos | Rama, preparación y operaciones en curso del worktree principal |
| `.git/refs/` y `.git/logs/` | Recursiva | Commits, ramas, tags, stash y ramas remotas conocidas (base de los eventos de Git) |
| `.git/worktrees/` y cada `.git/worktrees/<nombre>/` | Un nivel más sus marcadores | Alta y baja de worktrees, y `HEAD`, `index` y operaciones en curso de cada worktree enlazado |
| `.git/objects/` | No se vigila | Ruido sin valor: los commits se detectan por refs y reflogs |
| Archivos de configuración de los niveles personales (ADR-GRP-007) | Archivos concretos | Recarga de configuración (solo lectura). El nivel de equipo se lee de objetos commiteados y se recarga por los cambios de refs y `HEAD` de las filas anteriores (ADR-GRP-007, Enmienda 2026-10-04) |
| **Solo en repos protegidos por Guardrails**: `<git-common-dir>/config`, el `config.worktree` de cada worktree (`<git-common-dir>/config.worktree` y `<git-common-dir>/worktrees/<nombre>/config.worktree`) y `<git-common-dir>/gitraptor/` | Archivos concretos; `gitraptor/`, recursiva | Detección de pérdida de la protección: `core.hooksPath` reescrito o añadido en un `config.worktree`, dispatchers editados o carpeta borrada (Enmienda 2026-10-04; ADR-GRD-005 § 4). El motor solo notifica; la comprobación es de Guardrails |

- **Altas y bajas de worktrees**: un directorio nuevo en `.git/worktrees/` lanza la lectura de su `gitdir` y el alta del watch de su working tree, sin intervención del desarrollador. La desaparición del directorio o de su working tree lanza la baja (ver apartado 6).
- **Validación del worktree enlazado (SEC-11, M2)**: `.git/worktrees/<nombre>/gitdir` lo puede escribir un agente. Antes de vigilar, el motor exige que el enlace sea **bidireccional** (el `.git` del working tree apunta de vuelta a ese `.git/worktrees/<nombre>`) y que la raíz **no sea `/`, `$HOME`, la raíz de una unidad ni un ancestro del repo**. Si no cumple, el worktree se reporta "no disponible" con el motivo y no se vigila. Las rutas UNC o de red no se vigilan sin acción explícita del desarrollador (M9).
- **Tope de watches por repo**: además del límite del SO, cada repo tiene un tope de watches (⚠️ **ASSUMPTION**: valor fijado en SPIKE-GRP-002); al superarlo, el worktree pasa a modo degradado (apartado 5) en vez de consumir los watches de los demás repos. En macOS el tope no aplica: FSEvents usa un único stream sin watches por directorio (SPIKE-GRP-002: el proceso pasa de 4 a 9 descriptores con 10 worktrees). El valor para Linux sigue pendiente de la medición de SPIKE-GRP-002 en ese SO.
- **Filtros**: los eventos de rutas ignoradas por Git se descartan antes del debounce. Las reglas de ignore (`.gitignore`, `.git/info/exclude`, `core.excludesFile`) se leen con `gix` y se recargan cuando cambia cualquiera de esos archivos. Directorios pesados como `target/` o `node_modules/` no tienen trato especial: se excluyen porque y solo si Git los ignora; si no están ignorados, sus cambios son cambios del usuario y se observan.
- **Linux**: inotify no es recursivo, así que se registra un watch por directorio no ignorado, al recorrer el árbol y al aparecer directorios nuevos. El escaneo de un directorio recién creado se hace de inmediato para no perder archivos creados antes de registrar su watch.

### 3. Debounce

- **Ventana fija por worktree, no deslizante**: el primer evento abre una ventana de 75 ms (ADR-GRP-011); al cerrarse, se recomputa con todas las rutas acumuladas. Los eventos que llegan durante el recomputo abren la ventana siguiente. Una ráfaga continua produce una publicación cada ciclo, en lugar de posponerla hasta que la ráfaga acaba: la espera máxima por debounce está acotada.
- Ventanas independientes por worktree: la ráfaga de un worktree no retrasa a los demás (SPIKE-GRP-002: con una ráfaga de 10K archivos en un worktree, los otros nueve publican en 95 ms p95).
- **Holgura del temporizador** (Enmienda 2026-10-04): el temporizador del SO se despierta tarde (SPIKE-GRP-002 en macOS: hasta 10 ms; una ventana programada de 75 ms dura 85 ms p95). La ventana se programa con la holgura descontada, medida por SO, para que su duración efectiva sea de 75 ms, y sin espera activa. ADR-GRP-011 § 2 presupuesta la duración efectiva. ⚠️ **ASSUMPTION**: la holgura es una constante por SO que calibra el banco (INF-GRP-002). Está sin decidir si basta o si hace falta calibrarla en tiempo de ejecución. En ambos casos, la Validación mide la duración efectiva (`t_recv` → `t_flush`).

### 4. Recomputo incremental

- **Eventos del working tree**: solo se recalcula el estado de las rutas tocadas, con una caché de stat en memoria del motor por worktree (nunca en el repo, ADR-GRP-009) que evita volver a leer contenido sin cambios.
- **`index` del worktree**: recálculo del estado completo de ese worktree (lo preparado puede haber cambiado entero).
- **Refs, `HEAD` y reflogs**: relectura de las refs afectadas; ahead/behind solo si cambió la punta de la rama o la base, con caché por par de commits.
- **Operaciones en curso**: relectura de los marcadores para reportar el estado especial (BR-EDGE-002).
- **Escala de historia (100K commits)**: ahead/behind se calcula con un recorrido acotado desde la base de fusión, usando el `commit-graph` del repo si existe, solo para leerlo (nunca se escribe, ADR-GRP-009).
- **Ahead/behind fuera del primer evento** (Enmienda 2026-10-04): sin `commit-graph`, una rama a 50K commits de la base cuesta 145 ms p50 (SPIKE-GRP-002, medido con `git rev-list`; el coste con `gix` está sin medir), casi todo el presupuesto de cómputo. Por eso ahead/behind no entra en el presupuesto del primer evento:
  - se publica en la segunda fase, con caché por par de commits;
  - se calcula en proceso con `gix`, sin lanzar un proceso de Git por cada cambio de punta. ⚠️ **ASSUMPTION**: que `gix` cueste lo mismo o menos que `git rev-list`, que es lo que se midió (ver Validación). Hasta medirlo, `git rev-list --count --left-right` de la allowlist (ADR-GRP-009) se mantiene como alternativa.
- **Publicación en dos fases**: si un cambio grande (p. ej. un checkout de miles de archivos) no cabe en el presupuesto de cómputo, el motor publica primero lo barato (rama, `HEAD`, operación en curso) y después los recuentos (ADR-GRP-011). Cuando hay que recalcular ahead/behind, este también va en la segunda fase, aunque el cambio sea pequeño (punto anterior).

### 5. Sondeo de respaldo y modo degradado

- **Sondeo ligero de respaldo** para cada worktree observado por eventos (⚠️ **ASSUMPTION**: cada 30 s): compara una huella barata (oid de `HEAD`, tamaño y mtime de `index` y `packed-refs`, marcadores de operación) con la última conocida. Si difiere sin que haya llegado un evento, lanza una reconciliación de ese worktree. Cubre eventos perdidos o fusionados por el SO **que tocan metadatos de Git** (commit, checkout, `git add`, ramas).
  - **Alcance** (Enmienda 2026-10-04): la huella no detecta un cambio del working tree cuyo evento se perdió (SPIKE-GRP-002: 4 de 7 no recuperados por el sondeo). Esos cambios solo se recuperan con los disparadores de reconciliación del apartado 6.
- **Modo degradado por worktree**: cuando no se puede vigilar por eventos, ese worktree pasa a sondeo frecuente (⚠️ **ASSUMPTION**: cada 2 s, con el estado completo por stat) y el motor expone "observación degradada" con el motivo. Ocurre si se agota el límite de watches de inotify, en sistemas de archivos de red o sin notificaciones, o si falla el registro del watch. En modo degradado NFR-04 no se garantiza, pero la observación sigue sin huecos. Coste medido en macOS (SPIKE-GRP-002): 17,6 ms p50 por ciclo en un worktree de 5.000 archivos, es decir, un 8,8% de un núcleo con 10 worktrees degradados cada 2 s. Crece de forma lineal con los archivos.
- **Límites de inotify**: el motor estima los watches necesarios (directorios no ignorados) y los compara con `max_user_watches` leído de `/proc`. Nunca cambia `sysctl` ni ningún límite del sistema (Q17); expone el déficit y la guía para subirlo, y el Cockpit y la CLI la presentan.

### 6. Reconciliación y robustez

- **Cuándo se reconcilia por completo** (estado actual leído de cero y comparado con el último estado persistido en el perfil, ADR-GRP-013):
  - Al arrancar el motor.
  - Al volver de suspensión, detectada por la notificación de energía del SO o por un salto entre el reloj de pared y el monótono.
  - Ante un desbordamiento de la cola del watcher: `IN_Q_OVERFLOW` en inotify, `MustScanSubDirs` o eventos descartados en FSEvents y desbordamiento del búfer en ReadDirectoryChangesW. Se reconcilia el worktree afectado o todos si el SO no indica cuál.
  - Al volver a añadir un repo (Q25), al recuperar Git 2.38 o superior (S19) y al recrear un watch que falló.
  - Tras cada recreación del stream del watcher, p. ej. por el alta o la baja de un worktree en macOS (apartado 1; Enmienda 2026-10-04): se reconcilian todos los worktrees de ese stream.
- **Resultado**: las diferencias encontradas se publican como eventos **"sin atribuir"** con una marca de hueco (inicio y fin del periodo no observado) para que la Time Machine sepa que no hay atribución en ese tramo (BR-EDGE-005). Nunca se atribuyen a un agente, aunque hubiera uno registrado antes del hueco.
- **Worktree o repo que desaparece** (BR-EDGE-001): el evento de borrado de la raíz, de su `gitdir` o un error del watch disparan una comprobación de existencia. Si no existe o no es accesible, se cierran sus watches de inmediato, el worktree pasa a "no disponible" y sus sesiones terminan (BR-WF-001). Cada worktree se procesa en una tarea aislada: el error de uno no detiene a los demás.
- **Windows**: los handles del watcher no deben impedir borrar ni mover un worktree. Al primer evento de borrado dentro de una raíz vigilada, o si el directorio queda pendiente de borrado, el motor cierra el handle de esa raíz y vuelve a abrirlo solo si el directorio sigue existiendo (lo valida SPIKE-GRP-002).

## Alternativas consideradas

- **Solo sondeo**: simple y sin límites del kernel, pero 10 o más worktrees por debajo de 300 ms obligan a recorrer árboles grandes varias veces por segundo, con CPU y disco constantes aunque nadie trabaje. Se mantiene solo como respaldo y modo degradado.
- **fsmonitor de Git**: el daemon integrado crea archivos y un socket en `.git` y el hook escribe la untracked cache; lo prohíbe ADR-GRP-009.
- **Watchman**: excelente a escala, pero es una dependencia externa que el usuario tiene que instalar (choca con NFR-06, binario único) y con Q17 si el motor la instalara.
- **Eventos de hooks de Claude Code o de Git**: el motor no instala hooks (Q22) y los hooks de Guardrails son solo señal adicional opcional; tampoco ven los cambios sin commitear.
- **Watcher nativo con debounce, recomputo incremental, sondeo de respaldo y reconciliación (elegida)**: única opción que cumple frescura, escala y cero huecos sin dependencias ni escrituras.

## Consecuencias

- ✅ Frescura dentro de los 300 ms del motor en el caso normal, con espera de debounce acotada aunque haya ráfagas.
- ✅ CPU casi nula en reposo: sin eventos no hay trabajo, salvo el sondeo de respaldo.
- ✅ Cero huecos silenciosos en los casos con causa detectable: lo que se pierde por la suspensión, un desbordamiento o la recreación del stream se recupera por reconciliación y queda marcado como hueco. La pérdida sin marca de un evento del working tree fuera de esos casos queda como riesgo residual (ver el ⚠️ del sondeo de respaldo).
- ✅ Los worktrees que crean y borran los agentes se incorporan y se retiran solos.
- ⚠️ En Linux, repos con muchos directorios no ignorados pueden agotar `max_user_watches` (por defecto 8192 en kernels antiguos; proporcional a la RAM desde 5.11). **Mitigación**: estimación previa, modo degradado por worktree y guía para subir el límite; el motor nunca lo cambia (Q17).
- ⚠️ Un desbordamiento durante una sesión activa produce un micro-hueco cuyos cambios quedan "sin atribuir", lo que reduce la atribución de Claude Code. **Mitigación**: búfer amplio del watcher, filtrado temprano de ignorados y medición de la frecuencia en SPIKE-GRP-002. ADR-GRP-012 puede reatribuir el hueco solo si tiene evidencia independiente del watcher, nunca por suposición.
- ⚠️ FSEvents fusiona eventos y su latencia configurable compite con el presupuesto de detección (≤ 50 ms). **Mitigación**: latencia del stream mínima (0,0, la que usa `notify`) y eventos por archivo. SPIKE-GRP-002 midió una detección de 12,2 ms p95, y la fusión nunca dejó un estado final incorrecto.
- ⚠️ En macOS, cada alta o baja de un worktree recrea el stream de FSEvents y abre un micro-hueco silencioso en todos los worktrees (Enmienda 2026-10-04). **Mitigación**: altas y bajas agrupadas y reconciliación tras cada recreación (apartados 1 y 6). Los cambios de ese tramo quedan "sin atribuir".
- ⚠️ El sondeo de respaldo no ve los cambios del working tree con eventos perdidos sin marca. **Mitigación**: disparadores de reconciliación del apartado 6, incluida la recreación del stream. Si el dogfooding muestra pérdidas fuera de esos casos, se valora una reconciliación completa periódica de baja frecuencia (unos 18 ms por worktree de 5.000 archivos) y un intervalo del modo degradado adaptado al número de archivos.
- ⚠️ En Windows, un handle abierto sobre la raíz puede hacer fallar `git worktree remove` o un borrado del agente. **Mitigación**: cierre del handle al primer borrado (apartado 6); si SPIKE-GRP-002 muestra que no basta, se vigila desde el directorio padre o se pasa ese worktree a sondeo.
- ⚠️ El sondeo de respaldo y el modo degradado añaden carga en máquinas con muchos worktrees. **Mitigación**: huella barata y solo por stat; intervalos configurables en los niveles perfil y local, nunca en el de equipo (ADR-GRP-007).
- ⚠️ El arranque con 10 o más worktrees en repos grandes hace una reconciliación completa costosa. **Mitigación**: reconciliación en paralelo por worktree, con el estado previo del perfil publicado primero como "reconciliando"; el arranque no cuenta para NFR-04.

## Validación

**SPIKE-GRP-002** (prototipo aislado, en los tres SO) confirma o invalida este ADR antes de que US-GRP-002 entre en desarrollo:

- **Latencia**: p95 de detección SO → motor ≤ 50 ms y del ciclo completo del motor ≤ 300 ms (ADR-GRP-011), con un archivo modificado, un `git add`, un commit, un checkout y la creación y el borrado de un worktree.
- **Debounce**: duración efectiva de la ventana (`t_recv` → `t_flush`) de 75 ms en p95 en cada SO, con la holgura calibrada (apartado 3).
- **Ahead/behind con `gix`**: coste en proceso en una rama a 50K commits de la base, con y sin `commit-graph`, comparado con `git rev-list --count --left-right`. Hasta tener esta medición, la alternativa de la allowlist se mantiene (apartado 4).
- **Escala**: 10 worktrees de un repo de 100K commits o más, con ráfaga de 10K archivos en uno de ellos; se miden watches usados, memoria, CPU en reposo y p95 de los otros nueve durante la ráfaga.
- **Huecos**: suspensión y reanudación, desbordamiento forzado de la cola con búfer reducido, watcher reiniciado, y alta y baja de worktrees con escrituras concurrentes en los demás (recreación del stream, Enmienda 2026-10-04); en todos los casos la reconciliación detecta el 100% de los cambios y los marca "sin atribuir".
- **Windows**: `git worktree remove`, borrado y renombrado de la raíz y de archivos con el watcher activo, sin fallos atribuibles al motor.
- **Linux**: comportamiento al agotar `max_user_watches` (modo degradado, sin caída del resto).
- **Seguridad (SEC-11)**: un `gitdir` manipulado hacia `$HOME` o `/`, o sin enlace de vuelta, no se vigila y el worktree queda "no disponible"; un repo que supera el tope de watches pasa a degradado sin afectar a otros; una ruta UNC no abre conexiones SMB. Los tests de seguridad viven en INF-GRP-001.

**Éxito**: todas las cifras dentro de presupuesto en los tres SO. **Fracaso**: si una no se cumple, se revisa este ADR (p. ej. sondeo por defecto en el SO afectado o vigilancia desde el padre en Windows) y, si cambia el reparto, también ADR-GRP-011. Después, INF-GRP-002 convierte estas mediciones en gate de CI e INF-GRP-001 comprueba que el observador no escribe nada en el repo.

**Guardrails (Enmienda 2026-10-04)**: en un repo protegido, reescribir `core.hooksPath` en `<git-common-dir>/config`, añadirlo al `config.worktree` de un worktree, editar un dispatcher o borrar `<git-common-dir>/gitraptor/` llega a Guardrails como notificación dentro de su objetivo (⚠️ **ASSUMPTION** de ADR-GRD-005 § 4: ≤ 5 s); en un repo no protegido esas rutas no se vigilan. No forma parte de SPIKE-GRP-002: lo valida ADR-GRD-005 (Validación 2) con el observador de este ADR.

## Referencias

- Requerimiento: `docs/requirements/features/motor-local/context.md` (Q1, Q6, Q8, Q17, Q21, Q22, Q25, Q26, Q28; supuestos S4 y S19).
- Reglas: BR-CONS-005, BR-EDGE-001, BR-EDGE-002, BR-EDGE-005, BR-WF-001.
- BRD: NFR-04, NFR-05, NFR-06.
- ADRs: ADR-GRP-001 (watcher en el core), ADR-GRP-005 (proceso por usuario), ADR-GRP-007 (configuración), ADR-GRP-009 (frontera de solo lectura), ADR-GRP-011 (presupuesto), ADR-GRP-012 (atribución), ADR-GRP-013 (eventos y huecos persistidos).
- Historias técnicas: SPIKE-GRP-002 (valida), INF-GRP-002 (banco de frescura y escala), INF-GRP-001 (repo intacto).
- Documentación: crate `notify`; Apple File System Events; `inotify(7)`; Win32 `ReadDirectoryChangesW`.

## Revisión de seguridad (2026-10-03)

Enmienda tras la revisión del security-expert. No cambia el mecanismo ni el reparto de presupuesto.

| Hallazgo | Cómo se cubre |
|---|---|
| M2 · `gitdir` escribible por un agente permite vigilar `$HOME` o `/` | Apartado 2: enlace `gitdir` bidireccional, raíces prohibidas (`/`, `$HOME`, raíz de unidad, ancestros del repo) y tope de watches por repo (SEC-11) |
| M9 · Rutas UNC en Windows | Apartado 2: no se vigilan sin acción explícita del desarrollador (SEC-11) |

Validación ampliada: SEC-11 (punto "Seguridad").

## Enmienda (2026-10-04, Guardrails)

Aplicada desde la tabla de enmiendas de [non-functional-guardrails.md](../non-functional-guardrails.md) (J10). No cambia el mecanismo, el debounce, el sondeo ni la reconciliación. El `status` siguió en `proposed` hasta su aceptación (Rene Bonilla, 2026-10-04).

| Cambio | Dónde | Fuente |
|---|---|---|
| Vigilar `<git-common-dir>/config`, el `config.worktree` de cada worktree y `<git-common-dir>/gitraptor/` en los repos protegidos | § 2 (tabla); Validación | ADR-GRD-005 § 4 |
| Aclaración derivada de la Enmienda de ADR-GRP-007: la fila de configuración cubre los niveles personales; el nivel de equipo se recarga por refs y `HEAD`, que ya se vigilan (sin rutas nuevas) | § 2 (tabla) | ADR-GRP-007, ADR-GRD-004 § 1 |

## Enmienda (2026-10-04, SPIKE-GRP-002)

Aplicada desde las recomendaciones de [SPIKE-GRP-002-resultados.md](../../requirements/features/motor-local/research/SPIKE-GRP-002-resultados.md) (§ 6), que se midieron **solo en macOS**. No cambia el mecanismo elegido. El `status` sigue en `proposed`. Linux y Windows siguen pendientes de la Validación. En macOS también quedan sin verificar la suspensión y reanudación reales y el desbordamiento forzado (Resultados § 3.6).

| Cambio | Dónde | Fuente |
|---|---|---|
| Recreación del stream de FSEvents con `notify` 8.2: altas y bajas agrupadas y reconciliación tras cada recreación; reanudar desde el último `FSEventStreamEventId` queda como optimización | § 1, § 6, Consecuencias, Validación (escenario de huecos) | Resultados § 3.6 |
| Tope de watches por repo: no aplica en macOS; el valor para Linux sigue pendiente | § 2 | Resultados § 3.4 |
| Ventana con la holgura del temporizador descontada (duración efectiva de 75 ms) | § 3 | Resultados § 3.2; ADR-GRP-011 § 2 |
| Ahead/behind fuera del presupuesto del primer evento: segunda fase, con caché y en proceso con `gix` | § 4 | Resultados § 3.11 |
| Alcance del sondeo de respaldo (solo metadatos de Git) y coste medido del modo degradado | § 5, Consecuencias | Resultados § 3.5 y § 3.6 |
| Revisión de coherencia: la consecuencia "cero huecos silenciosos" se limita a los casos con causa detectable; ahead/behind va en la segunda fase también en cambios pequeños; la cifra de 145 ms se midió con `git rev-list` | Consecuencias, § 4 | Revisión del Arquitecto (2026-10-04) |
| Ahead/behind en proceso con `gix` marcado como ⚠️ ASSUMPTION; Validación nueva con una rama a 50K commits de la base, con y sin `commit-graph`; `git rev-list` de la allowlist como alternativa hasta medirlo | § 4, Validación | Resultados § 3.11; Artifact Judge (reservas) |
| Holgura del temporizador: constante por SO calibrada por el banco (INF-GRP-002), como ⚠️ ASSUMPTION; Validación de la duración efectiva de la ventana | § 3, Validación | Resultados § 3.2; Artifact Judge (reservas) |
