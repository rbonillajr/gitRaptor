---
id: ADR-CKP-001
title: Predicción de conflictos con merge en seco sin escribir en el repo
type: adr
status: proposed
date: 2026-10-04
created: 2026-10-04
updated: 2026-10-04
deciders: [Rene Bonilla]
domain: GRP
feature: cockpit
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRP-013, ADR-GRD-006, ADR-TMC-001, SPIKE-CKP-001, INF-GRP-001, INF-GRP-002, TS-GRP-004, CTX-CKP-001, BR-CKP-001]
tags: [cockpit, prediccion-conflictos, merge-en-seco, gitoxide, gix-merge, merge-tree, solape, kpi, nfr-01, nfr-07, br-06, dep-ckp-1]
---

# ADR-CKP-001 — Predicción de conflictos con merge en seco sin escribir en el repo

> **Estado**: propuesto. Decisión del orquestador (2026-10-04), validada por Arquitecto; el PO valida el alcance después. El mecanismo queda **pendiente de confirmar por SPIKE-CKP-001**. El ADR no pasa a `accepted` sin sus resultados en macOS.

## Contexto

BR-06 (Must) pide detectar solapes de archivos y hunks entre agentes, y con la rama base, antes del merge. Q-CKP-5 fija dos niveles publicados por el motor: **solape** (⚠), que incluye lo sin commitear, y **conflicto previsto** (⚡), que es el merge en seco de lo commiteado, con archivos y hunks. Q-CKP-6 deja la predicción fuera del gate de 500 ms, con un objetivo de ≤ 5 s p95 que es supuesto (S-CKP-1), "calculando" en el cálculo inicial y la antigüedad siempre visible. Q-CKP-21 mide el KPI del 70 % con un registro en el perfil de 90 días. Q-CKP-25 hace de la demo del BRD § 13 una prueba de aceptación. R-CKP-2 (falsos positivos y negativos) y R-CKP-9 (55 pares con 10 worktrees) son los riesgos de esta decisión. Reglas: BR-CKP-CALC-002, BR-CKP-CALC-003, BR-CKP-WF-007, BR-CKP-CONS-005, BR-CKP-WF-005 y BR-CKP-EDGE-001.

El BRD nombra `git merge-tree --write-tree` como mecanismo y lo usa para justificar Git ≥ 2.38 (NFR-07). Ese comando escribe objetos en `.git/objects`, y ADR-GRP-009 § 2 lo prohíbe al motor: el motor no provoca **ninguna** escritura en el repo, ni transitoria, y **nunca ejecuta programas configurados por el usuario**. El overview (§ 10.2) y la Consecuencia 4 de ADR-GRP-009 dejaron el problema abierto para un ADR del Cockpit (DEP-CKP-1).

Hechos que condicionan la elección:

- **gitoxide ya tiene merge de árboles.** Comprobado en docs.rs el 2026-10-04: `gix` 0.88.0 (publicado el 2026-09-25) con la feature `merge` expone `Repository::merge_trees`, `merge_commits`, `merge_base`, `virtual_merge_base`, `tree_merge_options` y `merge_resource_cache`, sobre `gix-merge` 0.21.0. `Repository::with_object_memory()` hace que los objetos que escribe esa instancia se queden en memoria y no persistan. El `Outcome` devuelve el árbol sin escribir y la lista de conflictos. Las opciones del merge de árboles incluyen detección de renombrados (`rewrites`) y un contexto para **invocar drivers de merge** (`blob_merge_command_ctx`). Ese contexto es un vector de ejecución de código que hay que cerrar. La herramienta de consulta de context7 no estaba disponible para este agente; la comprobación se hizo directamente en docs.rs.
- **La Time Machine ya descartó `alternates` hacia el repo** para su almacén (ADR-TMC-001, alternativa B): un `gc --prune=now` del usuario borra objetos que el almacén necesita. Para la predicción el efecto es menor, porque el resultado se descarta, pero confirma que un almacén con alternates depende de objetos que no controla.
- **Escribir un objeto que ya existe** hace que Git lo "refresque" (`freshen`) y actualice el mtime del archivo, también en los alternates. ⚠️ **ASSUMPTION** a confirmar por SPIKE-CKP-001: si se confirma, `merge-tree` contra un almacén con alternates cambiaría el mtime de packs y objetos sueltos del repo del usuario, y el arnés de INF-GRP-001 lo detecta (huella con mtime).
- La capa que toca repos es `crates/git` (ADR-GRP-009); la lógica del motor, incluida la de "conflictos", vive en `crates/core` (ADR-GRP-002, ADR-GRP-005). La TUI no calcula nada (ADR-GRP-005, ADR-GRP-013).

## Decisión

Decisión del orquestador (2026-10-04), validada por Arquitecto: **opción preferida (a), merge en memoria con gitoxide; respaldo (b), `git merge-tree --write-tree` sobre un almacén de trabajo en el perfil; (c), `merge-tree` contra el repo, rechazada.** SPIKE-CKP-001 confirma (a) o activa (b) según sus criterios de salida. El resto de la decisión (pares, niveles, recálculo, cola, estados, límites, publicación y KPI) no depende del mecanismo.

### 1. Mecanismo

- **(a) Preferida — merge en memoria con `gix`**. `crates/git` abre el repo en solo lectura y obtiene una instancia con `with_object_memory()` para cada trabajo. Calcula el merge-base (virtual si hay varios) y ejecuta `merge_trees` sin escribir nada en disco. El árbol resultante nunca se escribe y la instancia se descarta al terminar. Condiciones obligatorias:
  - Lista de **drivers de merge vacía** y sin contexto de invocación: nunca se ejecuta `merge.<driver>.driver`.
  - **Sin procesos de filtro** (`filter.<name>.process`, `clean`, `smudge`) en el pipeline de conversión. Los blobs se comparan tal como están en la base de datos de objetos.
  - **Sin atributos del working tree**: la pila de atributos no lee `.gitattributes` del disco.
  - `gix` no invoca el binario `git` (M1 de ADR-GRP-009) y no hace descargas perezosas de un *partial clone*: un objeto ausente da "no calculable".
  - Marcadores de conflicto con tamaño ampliado (`marker_size_multiplier`) para que la extracción de hunks no confunda líneas del archivo con marcadores.
- **(b) Respaldo — `merge-tree --write-tree` sobre un almacén de trabajo en el perfil**. Un repo *bare* de trabajo por repo observado en la carpeta de datos del perfil, con `objects/info/alternates` hacia el directorio de objetos común del repo. `git --git-dir=<almacén de trabajo> merge-tree --write-tree -z --name-only --messages <a> <b>` con commits ya resueltos por `gix`, nunca nombres de ref. El `HEAD` del almacén está sin nacer, así que no hay `.gitattributes` del usuario y no se elige ningún driver. La configuración global y de sistema se aísla. Los objetos escritos van al almacén de trabajo, que se vacía tras cada lote. Solo es viable si SPIKE-CKP-001 demuestra que no cambia el mtime de nada del repo (refresco de objetos) y que no contacta con un remoto *promisor*.
- **(c) Rechazada — `merge-tree --write-tree` contra el repo**. Escribe objetos en `.git/objects` del usuario (ADR-GRP-009 § 2, NFR-01) y ejecuta los drivers de merge que configure el repo.

### 2. Pares

- **Worktree–base**: cada worktree con trabajo propio contra la **rama base confirmada** (Q-GRD-21). El worktree que tiene sacada la base no forma par consigo mismo.
- **Worktree–worktree**: cada par de worktrees con **trabajo propio**, es decir, con commits que no están en la base o con cambios sin commitear. Un worktree sin trabajo propio no forma pares entre worktrees.
- **Trabajo propio de un worktree** = rutas cambiadas entre `merge-base(punta, base)` y la punta, más sus rutas sin commitear. Con eso, un worktree que se ramificó de una base más antigua no "solapa" por commits de la base que el otro aún no tiene.
- Con 10 worktrees hay hasta **55 pares**: 10 contra la base y 45 entre worktrees.
- **Base no confirmada o pendiente** → los pares contra la base se publican "pendiente" y los pares entre worktrees siguen (BR-CKP-WF-005). **Base inexistente** → "no calculable" con el motivo, sin elegir otra rama (BR-CKP-EDGE-001).

### 3. Dos niveles

| Nivel | Entrada | Cálculo | Salida por par |
|---|---|---|---|
| **Solape** ⚠ | Trabajo propio de cada lado, **incluido lo sin commitear**. Contra la base: rutas cambiadas en la base desde el merge-base | Intersección de conjuntos de rutas, sin leer contenido | Archivos en común |
| **Conflicto previsto** ⚡ | **Solo lo commiteado**: punta de cada lado y su merge-base | Merge en seco (§ 1) | Archivos en conflicto con su tipo (contenido, añadido/añadido, modificado/borrado, renombrado, directorio/archivo, binario, submódulo) y, en conflictos de contenido, **hunks como rangos de líneas** en la versión de cada lado |

- El solape necesita el **conjunto completo de rutas sin commitear por worktree**. El motor ya lo calcula al recomputar el estado con `gix` (ADR-GRP-010 § 4), pero solo persiste una huella (ADR-GRP-013 § 1). El predictor lo consume **en memoria** dentro de `crates/core`, sin persistirlo. Con más rutas que el tope (⚠️ **ASSUMPTION**: 10.000 por worktree, a fijar por el SPIKE), el solape de ese worktree se publica como "parcial".
- Los hunks son **rangos de líneas, nunca contenido** (NFR-03): el MCP puede reutilizarlos sin exponer código. Topes por par: archivos y hunks por archivo (⚠️ **ASSUMPTION**: 200 y 50). Si se superan, se marca "truncado". Un binario o un submódulo da el archivo sin hunks.
- **Prefiltro**: si los conjuntos de rutas commiteadas de un par no se cortan, ni siquiera ampliados con los directorios padre (conflictos directorio/archivo) y con los orígenes de renombrado, el par se publica "sin conflicto" sin ejecutar el merge. SPIKE-CKP-001 mide cuántos pares ahorra y si introduce falsos negativos. Si los introduce, el prefiltro se retira.

### 4. Recálculo incremental

- **Disparadores del merge en seco**: cambia la punta de un worktree (commit, amend, reset, rebase terminado), cambia la punta de la base, se confirma o cambia la base, o se da de alta o de baja un worktree. Un cambio sin commitear solo recalcula el **solape**, que es barato.
- **Solo los pares afectados**: un commit en W recalcula los pares que incluyen a W (como mucho 10 con 10 worktrees). Un movimiento de la base recalcula los pares contra la base y vuelve a evaluar quién tiene trabajo propio.
- **Caché por contenido**: el resultado de un par se indexa por las tres ids de commit (punta A, punta B y merge-base). Si no cambian, el resultado sigue valiendo y no se recalcula.
- **Operación en curso** (rebase o merge a medias, HEAD separado durante un rebase): los pares de ese worktree quedan "pendiente (operación en curso)" hasta que termina, para no calcular una punta que se mueve por cada commit reaplicado.

### 5. Cola, prioridad y presupuesto de CPU

- **Fuera del camino caliente**: la predicción corre en un pool propio del daemon, separado del observador y del recomputo de ADR-GRP-010. Nunca consume el presupuesto del motor de ADR-GRP-011 (≤ 300 ms p95). SPIKE-CKP-001 mide ese p95 con la predicción en plena carga.
- **Cola coalescente por par**: un par tiene como mucho un trabajo pendiente y las entradas nuevas sustituyen a las anteriores. Un trabajo en curso cuyas entradas cambiaron se descarta al terminar, sin publicarse como actual.
- **Prioridad**: (1) los pares contra la base del worktree que acaba de cambiar; (2) sus pares con worktrees que tienen sesión presente; (3) el resto. En el cálculo inicial, primero los pares con sesión presente.
- **Presupuesto** (⚠️ **ASSUMPTION** hasta SPIKE-CKP-001): hasta 2 trabajos en paralelo (o 1 con 4 núcleos o menos); hilos con prioridad baja del SO (QoS *utility* en macOS; equivalentes en Linux y Windows: **Pendiente: etapa de validación multiplataforma**); tiempo máximo por par de 2 s y memoria máxima por trabajo. Si se excede, el par queda "no calculable (límite excedido)" y no se reintenta hasta que cambien sus entradas.

### 6. Estados y antigüedad

Cada par se publica con uno de estos estados y con la **hora de cálculo** de su resultado. La antigüedad la deriva el cliente (BR-CKP-CALC-003):

| Estado | Cuándo |
|---|---|
| `calculando` | Aún no hay resultado: cálculo inicial, par nuevo o base movida |
| `actual` | El resultado corresponde a las entradas vigentes |
| `recalculando` | Hay un resultado anterior y uno nuevo en curso. El anterior se muestra como desactualizado, nunca como actual |
| `pendiente` | Base no confirmada o pendiente, u operación en curso |
| `no calculable` | Base inexistente, objeto ausente, repo superficial, límite excedido o error, con su motivo |

### 7. Límites declarados (R-CKP-2)

La vista los declara siempre (BR-CKP-CALC-002) y el contrato los publica como datos, no como texto libre:

- El solape incluye lo sin commitear; el conflicto previsto solo lo commiteado.
- **No se aplican** los drivers de merge, los `.gitattributes` (`merge=`, `text`/`eol`, `conflict-marker-size`) ni los filtros (LFS: se fusiona el puntero, no el contenido).
- Renombrados: detección con el límite y el umbral de la configuración leída del repo (leer configuración no ejecuta nada). Por encima del límite, sin detección, declarado. La paridad con `merge-ort` (renombrados de directorio, merge-base virtual en historias cruzadas) es lo que mide el SPIKE.
- El par worktree–worktree aproxima "se integra uno y después el otro" e ignora lo que la base reciba entre medias.
- Solo el repo local; nada del remoto (Q12 de motor-local).

### 8. Publicación

- El motor publica el resultado por par como **estado del motor** (incluido en la instantánea) y como **evento** de cambio del stream, con la secuencia del motor, de modo que la TUI, `raptor conflicts` y `check_conflicts` del MCP (F-001-05) leen lo mismo (BR-CKP-CONS-001). Un ⚡ nuevo es lo que dispara ConflictAlert y el toast en la TUI (BR-CKP-WF-007); el motor no decide la alerta.
- Rutas y nombres de rama son texto no confiable (SEC-12, ADR-GRP-005 § 5). Las respuestas del MCP llevan archivos y rangos, nunca contenido.
- **Forma del contrato** (tipos, nombres de campos, consulta bajo demanda, evento de lote): **pendiente, dueño: worker del canal (TS-GRP-004)**. Este ADR no la fija ni edita TS-GRP-004 ni `api-contract-ipc.md`.

### 9. Registro para el KPI (Q-CKP-21, BR-CKP-CONS-005)

- El **daemon**, único escritor del perfil (ADR-GRP-005), registra en el **almacén por repo** (ADR-GRP-006 § 4) la **primera aparición** de cada ⚡ por (par, archivo) y cada conflicto real, con su hora. Solo metadatos: identificador del par, ramas, ruta y horas. Nunca hunks ni contenido.
- El par se identifica por los worktrees y las ramas en ese momento, para que el registro sobreviva al borrado del worktree.
- **Conflicto real**: depende de que el motor publique el estado en conflicto, con rutas sin fusionar y `MERGE_HEAD`/`onto` (DEP-CKP-14, enmiendas de ADR-GRP-010 § 4 y ADR-GRP-013 § 1). No se resuelve aquí.
- Retención de 90 días con purga diaria, con la misma forma que ADR-GRD-006 § 3. Los conflictos en huecos de observación o en el remoto se guardan marcados y quedan fuera del cociente. La consulta es local, por el canal.
- ⚠️ **ASSUMPTION** a validar por el PO: un ⚡ del par (W1, W2) en el archivo F cuenta como "detectado antes" para un conflicto de W2 contra la base en F si la punta de W1 ya estaba integrada en la base. Sin esta equivalencia, el criterio literal de "mismo par" infravalora el KPI.
- Con esto queda resuelta la parte KPI de **DEP-CKP-11**. Las preferencias de la TUI (Q-CKP-17) siguen abiertas. (Resueltas el 2026-10-04 en ADR-GRP-006, Enmienda (2026-10-04, Cockpit).)

### 10. Dónde vive el código

| Pieza | Crate | Por qué |
|---|---|---|
| Merge en seco y rutas cambiadas entre dos commits: funciones tipadas que reciben ids de commit y devuelven conflictos, rutas y hunks | `crates/git` | Única capa que toca repos (ADR-GRP-009). En (a) no tiene `Command::new`; en (b) sería un **módulo de invocación autorizado nuevo** |
| Predictor: pares, solape, prefiltro, cola, caché, estados y registro KPI | `crates/core`, módulo de predicción | Lógica del motor (ADR-GRP-002 lista "conflictos" en `core`; ADR-GRP-005 § 1) |
| Tipos publicados | `crates/api` | Contrato (pendiente de TS-GRP-004) |
| Presentación y alerta | `apps/cli` (TUI, `raptor conflicts`), `apps/mcp` | Solo consumen lo publicado (ADR-GRP-005, ADR-GRP-013) |

El predictor no importa ninguna capa de escritura (Time Machine, Guardrails ni ejecutor de operaciones), igual que el observador (ADR-GRP-009, Validación 5).

### 11. Nota sobre NFR-07

El BRD justifica Git ≥ 2.38 con `merge-tree --write-tree`. Con (a), la predicción no usa el Git CLI y esa justificación deja de ser cierta. **El mínimo no cambia** (Q28 de motor-local): lo sostienen la resolución y la allowlist de ADR-GRP-009 y las capas de escritura de la Time Machine, Guardrails y el ejecutor. Reformular el texto de NFR-07 en el BRD queda **pendiente para el PO**; este ADR no lo edita. Con (b), la justificación se mantiene, pero en 2.38 no existen `--merge-base` ni el modo por lotes `--stdin` (⚠️ **ASSUMPTION**: llegaron en 2.40 y 2.42; lo confirma el SPIKE), así que cada par sería un proceso.

## Alternativas consideradas

| Alternativa | A favor | En contra | Resultado |
|---|---|---|---|
| **(a) Merge en memoria con `gix`** | Cero escrituras por construcción; sin procesos hijo (un `Command::new` menos); sin red; un solo proceso con cachés de packs calientes para los 55 pares | Paridad con `merge-ort` por demostrar (renombrados de directorio, merge-base virtual); API reciente; exige cerrar a mano drivers, filtros y atributos | **Preferida**, a confirmar por SPIKE-CKP-001 |
| **(b) `merge-tree` sobre almacén de trabajo en el perfil** | Fidelidad de `merge-ort`; mecanismo nombrado en el BRD | Proceso por par; posible refresco de mtime en el repo vía alternates; posible descarga perezosa en *partial clone*; contenido del usuario en el perfil; nuevo módulo de invocación y entorno ampliado; dependencia de objetos que el `gc` del usuario puede borrar (ADR-TMC-001, B) | **Respaldo**, solo si (a) falla en fidelidad o rendimiento y (b) pasa el arnés |
| **(c) `merge-tree` contra el repo** | El más simple y fiel | Escribe en `.git/objects` y ejecuta drivers del repo (ADR-GRP-009 § 2, NFR-01, SEC-09) | **Rechazada** |
| Merge con `merge-tree` sobre el almacén de la Time Machine | Reutiliza un repo del perfil | El almacén puede no tener aún los últimos commits; mezcla responsabilidades; la capa de escritura de la Time Machine no se importa fuera de su módulo | Descartada |
| `libgit2` (`git2`) en memoria | Merge en memoria maduro | Dependencia C fuera del stack (ADR-GRP-001), segunda implementación de Git y otra superficie de seguridad | Descartada |
| Solo solape, sin merge en seco | Trivial | No da hunks ni distingue conflicto de solape; no cumple BR-06 ni la demo (Q-CKP-25) | Descartada; queda como degradación si el SPIKE falla en todo |

## Enmiendas que implica cada opción (no aplicadas)

Se listan para que el orquestador las aplique tras SPIKE-CKP-001. **Este ADR no edita ningún otro ADR.**

**Estado (2026-10-04)**: aplicadas las de la columna (a) como "Enmienda (2026-10-04, Cockpit)" en ADR-GRP-006, 009, 010, 011 y 013 y en `non-functional.md`, y cerrado el § 10.2 del overview. Las de la columna (b) quedan escritas como condicionadas a SPIKE-CKP-001 en ADR-GRP-006 y ADR-GRP-009, sin aplicar. TS-GRP-004 / `api-contract-ipc.md`: pendiente, dueño: worker del canal. BRD NFR-07: pendiente para el PO.

| ADR / documento | Con (a) | Con (b) |
|---|---|---|
| ADR-GRP-009 § 1 | Excepción acotada a "ninguna API de escritura de `gix`": escritura de objetos **solo a la memoria del proceso** (`with_object_memory`) en el módulo de merge en seco; sin drivers, filtros ni atributos del disco | Sin cambio |
| ADR-GRP-009 § 2 (tabla) | Fila nueva "merge en memoria con `gix`: permitido"; fila nueva "drivers de merge y procesos de filtro de `gix-merge`: prohibido"; `merge-tree --write-tree` sigue prohibido | La fila de `merge-tree` pasa a "permitido solo con `--git-dir` en el almacén de trabajo del perfil"; fila de refresco de objetos en alternates |
| ADR-GRP-009 § 3 | Sin cambio | Allowlist: `merge-tree` con argv fijo; entorno ampliado con el aislamiento de la configuración global y de sistema |
| ADR-GRP-009 Validación 5 | Comprobación estática: el módulo de merge en seco usa siempre la instancia en memoria | Nuevo módulo de invocación autorizado en la lista |
| ADR-GRP-009 Validación 7 | Repo canario ampliado con `merge.*.driver`, `filter.*.process` y `.gitattributes` con `merge=` | Igual, más *partial clone* con remoto *promisor* y captura de red |
| ADR-GRP-006 § 4 | Tabla del registro KPI en el almacén por repo, con purga a 90 días | Lo mismo, más la carpeta del almacén de trabajo (`<datos>/ckp/<id-repo>/`) con contenido del usuario: 0700/0600, fuera de las copias de seguridad, vaciada tras cada lote, con cuota y en diagnóstico |
| ADR-GRP-010 § 4 / ADR-GRP-013 § 1 | El conjunto completo de rutas sin commitear por worktree queda en memoria y lo consume el predictor (sin persistir ni cambiar el contrato) | Igual |
| ADR-GRP-011 § 2 y § 4 | Nota: la predicción queda fuera de NFR-04, con objetivo propio de ≤ 5 s p95; escenario nuevo en INF-GRP-002 | Igual |
| `non-functional.md` (SEC-09) | Añadir drivers de merge a la lista de programas que nunca se ejecutan | Igual |
| TS-GRP-004 / `api-contract-ipc.md` | Forma del estado y del evento de predicción: **pendiente, dueño: worker del canal (TS-GRP-004)** | Igual |
| BRD NFR-07 | Reformular la justificación de 2.38 (pendiente para el PO) | Sin cambio |
| Overview § 10.2 | Cerrar el pendiente remitiendo a este ADR | Igual |

## Consecuencias

- ✅ Cierra DEP-CKP-1 a falta del SPIKE: un único predictor en el motor alimenta la TUI, la CLI y el MCP.
- ✅ Con (a), la frontera de ADR-GRP-009 se mantiene con su criterio binario: cero diferencias en el repo y cero procesos hijo.
- ✅ El recálculo por par, con caché por ids de commit y prefiltro, deja el coste en función de lo que cambió y no del número total de pares.
- ✅ Los hunks como rangos permiten alertar y medir el KPI sin sacar contenido de la máquina.
- ⚠️ **Fidelidad**: sin drivers, sin atributos y con un motor de merge distinto del de Git hay falsos positivos y negativos. **Mitigación**: límites declarados en la vista, métrica complementaria de "⚡ que no ocurrieron" (Q-CKP-21) y umbral de paridad en el SPIKE.
- ⚠️ **API reciente de `gix-merge`**: puede cambiar entre versiones. **Mitigación**: versión fijada en el workspace; la paridad la cubre un test con un corpus fijo que se repite al actualizar `gix`.
- ⚠️ **CPU** con 10 agentes que commitean a la vez. **Mitigación**: cola coalescente, prioridad baja, tope de concurrencia y antigüedad visible. Ningún resultado viejo se presenta como actual.
- ⚠️ **Repos hostiles** (archivos enormes, miles de conflictos, árboles profundos). **Mitigación**: tiempo, memoria y topes por par, con resultado "no calculable" o "truncado".
- ⚠️ El criterio "mismo par" del KPI puede infravalorar la detección. **Mitigación**: la equivalencia propuesta en § 9 queda como supuesto para el PO.
- ⚠️ Si se activa (b), el perfil guarda temporalmente contenido del usuario fuera de `tm/`, y el motor gana un proceso hijo por par.

## Validación

1. **Repo intacto (INF-GRP-001)**: escenario "predicción" con 10 worktrees y los 55 pares calculados, con ejecución de control. Cero diferencias en la huella, incluido el mtime de packs, objetos sueltos y directorios de `.git/objects`. Bloquea el merge de la historia que implemente el predictor.
2. **Cero ejecución (SEC-09)**: repo canario con `merge.<x>.driver`, `filter.<x>.process`/`clean`, `.gitattributes` con `merge=<x>` y `core.fsmonitor`, todos apuntando a un script que deja un marcador. El marcador nunca aparece. Auditoría dinámica de `exec`: con (a), el predictor no lanza ningún proceso.
3. **Sin red**: un *partial clone* con objetos ausentes da "no calculable" con 0 conexiones.
4. **Fidelidad**: corpus fijo de escenarios y de merges reales reejecutados con `git merge` en clones temporales. Paridad de archivos en conflicto por encima del umbral que fije SPIKE-CKP-001, y 0 falsos negativos en la demo del BRD § 13 (Q-CKP-25).
5. **Frescura**: escenario de INF-GRP-002, del fin del commit a la predicción publicada, ≤ 5 s p95 con 10 worktrees y 100K commits. Es aviso hasta que el SPIKE confirme la cifra y gate después. El p95 del motor (≤ 300 ms) no empeora durante una ráfaga de predicciones.
6. **Estados**: tests de `calculando`, `recalculando`, `pendiente` (base no confirmada, operación en curso) y `no calculable` (base inexistente, límite excedido). Un resultado de entradas viejas nunca se publica como `actual`.
7. **KPI**: registro de la primera aparición por (par, archivo), purga a 90 días y exclusión de huecos. Privacidad: un repo con contenido marcado no deja ese contenido en el almacén.
8. **Salida (SEC-12)**: una ruta con secuencias de control sale escapada en la TUI y en `raptor conflicts`; el MCP no devuelve contenido.

Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

## Referencias

- Requerimiento: [context.md del Cockpit](../../requirements/features/cockpit/context.md) (Q-CKP-5, 6, 21, 25, 27; S-CKP-1; R-CKP-2, R-CKP-9; DEP-CKP-1, 11, 14) y [business-rules.md](../../requirements/features/cockpit/business-rules.md) (BR-CKP-CALC-002, CALC-003, WF-005, WF-007, CONS-005, EDGE-001).
- BRD: BR-06, NFR-01, NFR-03, NFR-05, NFR-07, KPI § 9, demo § 13.
- ADRs: ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRP-013, ADR-GRD-006 (forma del registro y la purga), ADR-TMC-001 (alternates descartados).
- Enablers: [SPIKE-CKP-001](../../requirements/features/cockpit/technical-stories/SPIKE-CKP-001-prediccion-5s.md), INF-GRP-001, INF-GRP-002, TS-GRP-004 (contrato, pendiente).
- gitoxide: docs.rs de `gix` 0.88.0 (`Repository::merge_trees`, `with_object_memory`, `merge::tree::Outcome`) y de `gix-merge` 0.21.0 (`tree::Options`, `blob::Platform`), consultados el 2026-10-04.
- Git: `git-merge-tree(1)`, `gitrepository-layout(5)` (`objects/info/alternates`), `gitattributes(5)` (atributo `merge`).
