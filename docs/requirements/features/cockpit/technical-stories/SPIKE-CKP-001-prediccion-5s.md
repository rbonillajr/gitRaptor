---
id: SPIKE-CKP-001
title: "Predicción de conflictos en ≤ 5 s p95 sin escribir en el repo: merge en memoria frente a almacén en el perfil"
type: spike
status: draft
feature: cockpit
domain: GRP
priority: critical
complexity: medium
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-CKP-001, ADR-GRP-009, ADR-GRP-006, ADR-GRP-011, ADR-TMC-001]
  stories: [INF-GRP-001, INF-GRP-002]
  specs: []
ado:
  id: null
  url: null
tags: [cockpit, spike, prediccion-conflictos, merge-en-seco, gitoxide, gix-merge, merge-tree, rendimiento, fidelidad, repo-intacto, dep-ckp-1]
---

## SPIKE-CKP-001: Predicción de conflictos en ≤ 5 s p95 sin escribir en el repo

**Valor**: decidir con mediciones el mecanismo del merge en seco de ADR-CKP-001 y confirmar o corregir el supuesto S-CKP-1 (≤ 5 s p95) antes de que la historia del predictor y las de BR-06 entren en desarrollo.

> Un SPIKE no lleva Dev Spec: su entregable es un Research Brief en `research/SPIKE-CKP-001-resultados.md`. Es un prototipo aislado, sin código del daemon: un binario de prueba que usa `gix` (feature `merge`, versión fijada; en docs.rs, 0.88.0 con `gix-merge` 0.21.0) y el Git del sistema. Repos temporales, nunca este repo. **Depende de**: el núcleo del arnés de INF-GRP-001 (huella y ejecución de control) y el repo de 100K commits del banco de INF-GRP-002. **Valida**: ADR-CKP-001 (opción preferida, respaldo y cifras de § 3 a § 5). **Bloquea**: el paso de ADR-CKP-001 a `accepted` y la Dev Spec del predictor.
>
> **Plataformas**: macOS ahora (máquina de dogfooding y runner de CI). Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

### Pregunta

¿Qué mecanismo de merge en seco cumple a la vez estas cuatro condiciones?

- No deja **ninguna** diferencia en el repo del usuario (ni objetos, ni locks, ni mtime) y no ejecuta programas configurados por el usuario (ADR-GRP-009, SEC-09).
- Detecta los mismos archivos en conflicto que un merge real de Git, con una tasa de falsos positivos y negativos aceptable.
- Recalcula los pares afectados por un commit en **≤ 5 s p95** con 10 worktrees (55 pares) y un repo de 100K commits.
- Cabe en un presupuesto de CPU y memoria que no empeora el p95 del motor (≤ 300 ms, ADR-GRP-011).

Las opciones son **(a)** merge en memoria con `gix` (`merge_trees` sobre una instancia con `with_object_memory`) y **(b)** `git merge-tree --write-tree` con `--git-dir` en un almacén de trabajo del perfil, con `objects/info/alternates` hacia el repo.

### Hipótesis

- **(a) deja el repo intacto por construcción**: ningún archivo creado ni modificado bajo `.git`, ningún proceso hijo y ninguna conexión, incluso con drivers de merge, procesos de filtro y `.gitattributes` hostiles, siempre que la lista de drivers esté vacía y la pila de atributos no lea el disco.
- **(b) cambia el mtime** de packs u objetos sueltos del repo al refrescar (`freshen`) objetos que ya existen en el alternate. En un *partial clone* intenta descargar objetos del remoto *promisor*. Si se confirma cualquiera de las dos cosas, (b) no es viable sin una enmienda de ADR-GRP-009.
- **Fidelidad de (a)**: coincide con `git merge` en el conjunto de archivos en conflicto en ≥ 95 % de los pares del corpus (⚠️ **ASSUMPTION**: umbral provisional). Las diferencias se concentran en renombrados de directorio y en historias cruzadas con merge-base virtual.
- **Rendimiento**: un commit en un worktree recalcula sus ≤ 10 pares en ≤ 5 s p95 con (a). (a) es más rápido que (b) porque reutiliza las cachés de packs en un solo proceso, y (b) en Git 2.38 paga un proceso por par (sin `--stdin` ni `--merge-base`, que se suponen de 2.42 y 2.40).
- **Prefiltro**: si las rutas commiteadas de un par no se cortan, ampliadas con los directorios padre y los orígenes de renombrado, no hay conflicto. El prefiltro ahorra la mayoría de los pares sin introducir falsos negativos.

### Experimento

- **Repo intacto (las dos opciones)**: arnés de INF-GRP-001 con ejecución de control. 10 worktrees, los 55 pares calculados, recálculos tras commits y tras mover la base. Huella del directorio común, de `.git/worktrees/*` y de cada working tree, incluido el mtime de packs, objetos sueltos y directorios de `objects/`. Casos: `gc` del usuario concurrente, repo con `alternates` propios y *partial clone* con objetos ausentes (captura de red: 0 conexiones esperadas).
- **Cero ejecución (las dos opciones)**: repo canario con `merge.<x>.driver`, `filter.<x>.process`, `filter.<x>.clean`, `.gitattributes` con `merge=<x>` y `filter=<x>`, y `core.fsmonitor`, todos apuntando a un script que deja un marcador. Auditoría dinámica de `exec` con eslogger: procesos hijo con (a), cero; con (b), solo `git merge-tree` con el argv fijo.
- **Fidelidad**: comparar cada opción con `git merge` (estrategia por defecto) ejecutado en un **clon temporal desechable**, nunca en el repo observado.
  - Corpus sintético: misma función y líneas contiguas, añadido/añadido, modificado/borrado, renombrado con modificación, renombrado/renombrado, renombrado de directorio, directorio/archivo, binario, submódulo, punteros LFS, historia cruzada con varios merge-base, CRLF, y `.gitattributes` con `merge=union` y `-merge`.
  - Corpus real: reejecutar los merges de dos padres de un repo público con historia de merges (⚠️ **ASSUMPTION**: `git/git`; lo elige el SPIKE) y comparar el conjunto de archivos en conflicto y los rangos de hunks.
  - Medir falsos positivos (⚡ sin conflicto real) y falsos negativos (conflicto real sin ⚡) por tipo de conflicto, y su efecto sobre los límites declarados de BR-CKP-CALC-002.
- **Rendimiento** (repo de 100K commits del banco de INF-GRP-002, 10 worktrees):
  - Tiempo por par según el tamaño del delta (1, 10, 100 y 1.000 archivos cambiados por lado). p50, p95, p99 y máximo.
  - **Recálculo tras un commit**: desde el fin del commit hasta el último par afectado calculado, sin y con prefiltro.
  - **Cálculo inicial** de los 55 pares y **movimiento de la base**: tiempo hasta salir de "calculando".
  - **Ráfaga**: 10 agentes simulados que commitean cada 2 a 10 s durante 10 minutos. Profundidad de la cola, trabajos descartados por entradas viejas y antigüedad máxima publicada.
  - Al menos 200 muestras por escenario, descartando las 10 primeras (como ADR-GRP-011 § 4).
- **Coste**: CPU (% de un núcleo, media y pico), memoria residente máxima por trabajo y del proceso, efecto sobre el p95 del motor (`t0` → `t_client_recv`) medido con el banco de INF-GRP-002 durante la ráfaga, con concurrencia 1 y 2 y prioridad baja del SO (QoS *utility*). Disco del almacén de trabajo con (b).
- **Solape**: coste de mantener y cortar los conjuntos de rutas sin commitear con 10 worktrees y una ráfaga de 10K archivos en uno; valor del tope por worktree.
- **Hunks**: extracción de rangos de líneas desde el resultado en memoria, con marcadores ampliados, frente a los rangos de `git merge`; archivos con líneas que imitan marcadores.
- **Límites**: valor del tiempo máximo por par, de la memoria por trabajo y de los topes de archivos y hunks con un repo hostil (archivo de 100 MB, 10K conflictos, árbol muy profundo).
- **Versiones de Git**: 2.38 (mínimo) y la última estable, para (b) y para la referencia de fidelidad.

### Criterios de Éxito

- **Repo intacto**: cero diferencias imputables y cero marcadores en el canario con la opción elegida. Es una condición eliminatoria.
- **Rendimiento**: recálculo tras un commit ≤ 5 s p95 con 10 worktrees y 100K commits en macOS, y p95 del motor sin regresión fuera del ruido medido.
- **Fidelidad**: paridad de archivos en conflicto ≥ el umbral, con el umbral confirmado o corregido con datos, y 0 falsos negativos en la demo del BRD § 13 (Q-CKP-25).
- **Salida hacia ADR-CKP-001**:
  - Mecanismo elegido.
  - Cifras de § 3 a § 5: tope de rutas, topes de hunks, concurrencia, tiempo y memoria por par.
  - Si el prefiltro se mantiene.
  - Las enmiendas de su tabla que hay que aplicar.
  - Si el objetivo de 5 s pasa a gate en INF-GRP-002.
- **Vía de fracaso**:
  - Si (a) pasa el repo intacto pero no la fidelidad, se activa (b) solo si (b) también deja el repo intacto. Si no lo deja, se mantiene (a) y las diferencias medidas pasan a límites declarados, con revisión del KPI del 70 % por el PO.
  - Si ninguna opción cumple los 5 s, se revisa el objetivo con el PO (Q-CKP-6) antes que el mecanismo, con la antigüedad visible como mitigación.
  - Si ninguna deja el repo intacto, la predicción se reduce a solape y el ⚡ queda "no disponible" hasta un ADR nuevo.

### Time-box

⚠️ **ASSUMPTION**: 1,5 semanas en macOS para las dos opciones. Linux y Windows: **Pendiente: etapa de validación multiplataforma**, con el mismo procedimiento del README del prototipo.
