---
id: ADR-TMC-004
title: "ADR-TMC-004 — Cobertura en dos niveles: snapshot previo garantizado, captura por observación y previo vía hook"
type: adr
status: accepted
accepted: 2026-10-03
created: 2026-10-03
updated: 2026-10-08
date: 2026-10-03
domain: GRP
feature: time-machine
supersedes: []
superseded_by: null
deciders: [Rene Bonilla]
related:
  adrs: [ADR-GRP-005, ADR-GRP-010, ADR-GRP-011, ADR-GRP-012, ADR-GRP-013, ADR-TMC-001, ADR-TMC-002, ADR-TMC-003, ADR-TMC-006]
  stories: [US-TMC-001, US-TMC-004, US-TMC-005, US-TMC-006, US-TMC-020]
description: "Toda operación de GitRaptor pasa por la operación protegida del daemon (sin snapshot no hay operación); el Git crudo se captura a partir de los eventos del motor con coalescencia y fuera de su presupuesto; los hooks de Guardrails pueden pedir un previo"
tags: [adr, time-machine, cobertura, snapshot-previo, captura-continua, hooks, guardrails, d-tmc-10, br-tmc-cons-003]
published: true
---

# ADR-TMC-004 — Cobertura en dos niveles: snapshot previo garantizado, captura por observación y previo vía hook

**Status**: Aceptado · **Fecha**: 2026-10-03 · **Decisores**: Rene Bonilla · **Feature**: Time Machine (F-001-03)

**Decisión de Rene Bonilla (2026-10-03)**: TQ-3 → (a) la operación protegida entrega las operaciones de usuario al ejecutor del daemon; TQ-5 → (b) cuotas con hueco "sin espacio" y reserva del snapshot previo.

## Contexto

D-TMC-10 y BR-TMC-CONS-003 fijan dos niveles. (a) **Garantizado**: snapshot previo antes de toda operación lanzada por GitRaptor (CLI, TUI, MCP) y de cada undo, redo y restauración; si el snapshot falla, la operación no se ejecuta (BR-TMC-CONS-001). (b) **Por observación**: lo hecho con Git crudo o en el editor se captura a partir de lo que observa el motor y, cuando existan, con un snapshot previo vía los hooks de Guardrails (F-001-04), sin depender de ellos. La Time Machine no instala hooks (Q22). Cada punto del timeline declara su nivel. El motor publica cambios con un debounce de 75 ms y un presupuesto de 300 ms que nadie puede reclamar (ADR-GRP-010, ADR-GRP-011).

**Pregunta**: ¿cómo se garantiza el nivel (a) por construcción, con qué cadencia se captura el nivel (b) sin saturar la máquina ni el presupuesto del motor, y qué contrato se ofrece a Guardrails?

## Decisión

### 1. Nivel (a): la operación protegida es el único camino de escritura

- Toda operación que modifica el repo y lanza una superficie de GitRaptor entra por un único comando del canal del daemon, la **operación protegida**: (1) intención en el oplog, (2) snapshot previo del ámbito, (3) ejecución, (4) registro (ADR-TMC-002 § 5, ADR-TMC-003 § 3). La ejecución de una operación de usuario la hace el ejecutor del daemon de F-001-02/05, con los hooks del usuario; la Time Machine solo aporta 1, 2 y 4. No existe otra vía de escritura en el contrato de `crates/api`: un cliente no puede ejecutar una operación sin snapshot porque no tiene con qué.
- Undo, redo y restauración son operaciones protegidas de la propia Time Machine.
- **Reutilización**: si desde la última captura válida del ámbito no cambió nada, el snapshot previo reutiliza su árbol y solo añade una fila con nivel `previo_garantizado`. Es el camino rápido de ADR-TMC-006 (4 ms en macOS). Para saber que no cambió nada usa las rutas que el motor publicó desde la marca de esa captura y una caché de stat; sin continuidad, hace detección completa (ADR-TMC-006 § 5, escalón 2; Enmienda 2026-10-04).
- **Fallo**: disco lleno, almacén no disponible, tiempo máximo agotado o daemon caído, entonces la operación se aborta con el motivo y el repo no cambia (US-TMC-001, escenario 4). El snapshot previo tiene una reserva de disco propia que la captura por observación no puede consumir (SEC-TMC-12). Un repo mayor que el de referencia **no** se salta el snapshot: tarda más (US-TMC-020, escenario 3).

### 2. Nivel (b): captura por observación

- **Fuente**: dentro del daemon, la Time Machine se suscribe a los eventos que el motor **ya publicó** (ADR-GRP-010 § 4). Nunca lee nada en la ruta crítica del motor y nunca lo retrasa: el presupuesto de ADR-GRP-011 no se reparte con la Time Machine.
- **Disparadores por worktree**:
  - **Cambios de archivos**: captura cuando el worktree lleva `Q` sin cambios, o como mucho cada `M` durante una actividad continua. `Q` = 1 s y `M` = 5 s, valores internos **confirmados por SPIKE-TMC-001 en macOS** (Enmienda): una captura cuesta una mediana de 23 ms, y 10 worktrees en actividad continua suponen un 5 % de un núcleo. En Linux y Windows siguen como ⚠️ **ASSUMPTION**.
  - **Eventos de Git** (HEAD, ramas, índice, worktrees): captura inmediata del estado resultante y anclaje de los commits implicados en el almacén (ADR-TMC-001 § 3).
- **Coalescencia y contrapresión**: una captura en curso por worktree y un escritor del almacén por repo; si se acumula trabajo, se conserva solo la petición más reciente por worktree. Captura incremental: solo se leen y guardan las rutas cambiadas desde la captura anterior, según las rutas del motor con su marca de continuidad. Lo que el motor no notificó lo detecta la verificación periódica completa con gitoxide, fuera de la ruta crítica, o la detección completa cuando no hay continuidad (ADR-TMC-006 § 5; Enmienda).
- **Prioridad del snapshot previo** (Enmienda 2026-10-04, E8). Con un escritor único por almacén, el previo garantizado esperaba detrás de las capturas por observación: hasta 934 ms en la línea base del spike. Reglas:
  - Las capturas solo **serializan el tramo de ref + oplog**. Los blobs se escriben fuera del cerrojo: son objetos direccionados por contenido y no tienen conflicto.
  - Si hay un previo en cola, la captura por observación **aborta el blob en curso** (un archivo de 50 MB tarda unos 0,4 s) y se repite después.
  - **Espera máxima del previo detrás del escritor: ~10 ms** (valor de diseño que fija la Dev Spec de TS-TMC-001).
  - El mantenimiento (`repack`) corre fuera del cerrojo (ADR-TMC-007 § 4).
- **Consistencia**: una captura guarda la marca del motor al empezar. Si durante la lectura llega un evento de Git de ese worktree, la captura se descarta y se repite, para que nunca mezcle el estado de antes y el de después de una operación.
- **Fallo**: una captura que falla no crea punto; el timeline muestra el cambio sin punto recuperable (US-TMC-004, escenario 4).
- **Cuotas**: al alcanzar la cuota del repo o el mínimo de espacio libre, la captura por observación se detiene con un hueco "sin espacio" declarado (SEC-TMC-12; TQ-5 → b). Cifras confirmadas por SPIKE-TMC-001 en macOS (Enmienda).
- **Huecos y modo degradado**: en un hueco no hay capturas (BR-TMC-EDGE-002). En modo degradado (sondeo, ADR-GRP-010 § 5), las capturas siguen al sondeo.
- **Riesgo residual R2** (aceptado en el context): lo editado entre la última captura y una operación destructiva de Git crudo, a lo sumo `Q`/`M`, puede perderse.

### 3. Nivel (b) con hook: contrato para Guardrails

- La Time Machine ofrece un comando de la CLI que un hook de Guardrails puede invocar antes de una operación de Git crudo. El daemon toma un snapshot del worktree del hook con nivel `previo_hook` y responde con `completo` o `fallido`, dentro de un tiempo máximo. Qué hace el hook con un fallo (seguir o detener la operación) lo decide Guardrails.
- El solicitante de ese snapshot se atribuye por ascendencia del proceso, igual que en ADR-TMC-005. Hay un límite de snapshots `previo_hook` por repo y por cliente (SEC-TMC-12).
- **Sin recursión**: las escrituras de la Time Machine desactivan los hooks (ADR-TMC-002 § 2), así que un snapshot nunca dispara otro.
- **Sin dependencia**: sin hooks, el Git crudo queda en el nivel (b) por observación (US-TMC-005, escenario 2). La Time Machine no instala, no exige y no comprueba hooks (Q22).

### 4. Nivel declarado en cada punto

| Nivel | Cuándo | Qué promete |
|---|---|---|
| `previo_garantizado` | Antes de una operación protegida (GitRaptor, undo, redo, restauración) | El estado exacto previo a la operación |
| `previo_hook` | Antes de una operación de Git crudo con hooks de Guardrails | El estado previo, si el hook se ejecutó y el snapshot se completó |
| `observacion` | Capturas del motor (ediciones, Git crudo sin hook) | El último estado capturado; puede no ser el inmediato anterior |
| `manual` | Captura pedida con la operación `snapshot` del catálogo (herramienta MCP `snapshot`; Enmienda (2026-10-05, MCP)) | El estado del worktree en ese momento, con la etiqueta del solicitante |

Un evento de Git sin punto propio se muestra con su nivel real; nunca se presenta como protegido (BR-TMC-CONS-003).

## Alternativas consideradas

| Alternativa | En contra | Veredicto |
|---|---|---|
| **Snapshot dentro del ciclo del motor** (cada lote de 75 ms) | Rompe el presupuesto de 300 ms de ADR-GRP-011 y multiplica las capturas en una ráfaga | Descartada |
| **Capturar solo antes de operaciones** | No cubre el Git crudo sin hooks (D-TMC-10) | Descartada |
| **Watcher propio de la Time Machine** | Duplica el observador, los límites de inotify y la reconciliación de ADR-GRP-010 | Descartada (reutiliza el motor) |
| **Instalar hooks propios para tener previo garantizado** | Contradice Q22 | Descartada |
| **Que el cliente haga el snapshot y luego la operación** | Varios escritores y ninguna garantía de orden si el cliente falla | Descartada |

## Consecuencias

- ✅ BR-TMC-CONS-001 se cumple por construcción: el contrato no tiene una vía de escritura sin snapshot previo.
- ✅ La captura no compite con el motor por su presupuesto y reutiliza su observación, sus filtros de ignorados y su reconciliación.
- ✅ El snapshot previo garantizado suele ser casi gratis, porque reutiliza la última captura.
- ⚠️ R2 sigue presente con Git crudo sin hooks. **Mitigación**: cadencia corta, captura inmediata tras cada evento de Git y cobertura declarada.
- ⚠️ CPU y disco de las capturas con 10 worktrees activos. **Mitigación**: coalescencia, captura incremental y gate de INF-GRP-002 con la Time Machine activa (el p95 del motor no puede empeorar).
- ⚠️ El Cockpit y el MCP dependen de este contrato para sus operaciones: su ADR no puede añadir otra vía de escritura (pendiente de integración).

## Validación

1. **Garantía**: con el almacén lleno o sin permisos, una operación protegida no se ejecuta y el repo queda igual (US-TMC-001, US-TMC-002).
2. **Sin vía alternativa**: test de contrato de `crates/api`: ningún mensaje del canal modifica el repo fuera de la operación protegida.
3. **Observación**: edición en el editor y archivo nuevo sin seguimiento, entonces el punto aparece con nivel `observacion` en ≤ `Q` + tiempo de captura; un `reset --hard` posterior deja restaurable el último estado capturado (US-TMC-004).
4. **Consistencia**: un `checkout` en mitad de una captura descarta la captura y la repite; ningún punto mezcla los dos estados.
5. **Presupuesto del motor**: INF-GRP-002 con la Time Machine activa y una ráfaga de 1.000 archivos: el p95 del motor sigue ≤ 300 ms.
6. **Hook**: un hook simulado que invoca el comando produce un punto `previo_hook`; si el snapshot falla, el punto no figura como previo (US-TMC-005).
7. **Prioridad** (Enmienda): con 9 worktrees capturando y un archivo de 50 MB en curso, un previo espera ≤ 10 ms al escritor (p95).

## Referencias

- **Reglas**: BR-TMC-CONS-001, BR-TMC-CONS-002, BR-TMC-CONS-003, BR-TMC-EDGE-002; D-TMC-9, D-TMC-10. Q22. Riesgo R2.
- **ADRs**: ADR-GRP-005, ADR-GRP-010, ADR-GRP-011, ADR-GRP-012, ADR-GRP-013; ADR-TMC-001, ADR-TMC-002, ADR-TMC-003, ADR-TMC-006.
- **Enablers**: TS-TMC-001, TS-TMC-004, SPIKE-TMC-001. **Features**: F-001-02, F-001-04, F-001-05.

## Enmienda (2026-10-04, SPIKE-TMC-001)

Aplicada desde § 7 de [SPIKE-TMC-001-resultados.md](../../requirements/features/time-machine/research/SPIKE-TMC-001-resultados.md), medido **solo en macOS**. El `status` sigue en `accepted`. Decisión del orquestador (2026-10-04), validada por el Arquitecto, que pidió ajustes y están incorporados.

| Cambio | Dónde | Fuente |
|---|---|---|
| Reutilización y captura incremental con las rutas del motor y su marca de continuidad (escalón 2 en el diseño base); detección completa con gitoxide sin continuidad; verificación periódica | § 1, § 2 | E2; ADR-TMC-006 § 5 |
| Prioridad del previo en el escritor único: solo se serializa ref + oplog, la captura aborta el blob en curso y el previo espera ~10 ms como máximo | § 2, Validación 7 | E8; Resultados § 5.3; revisión del Arquitecto |
| `Q` = 1 s, `M` = 5 s y cuotas confirmados en macOS | § 2 | Resultados § 6 |

## Enmienda (2026-10-05, MCP)

Decisión del orquestador (2026-10-05), validada por Arquitecto y PO. Origen: DEP-MCP-2 (CTX-MCP-001), pendiente de ADR-CKP-002 ("API de captura y niveles", dueño: Time Machine) y D-14 del índice de historias del MCP.

- **Captura manual**: el ejecutor (ADR-CKP-002, operación `snapshot`) pide a la Time Machine una captura del worktree del solicitante con nivel declarado **`manual`** (§ 4). Usa la API de captura del módulo `timemachine`, no su capa de escritura (ADR-TMC-002 § 1). No escribe en el repo ni lanza `git`.
- **Etiqueta**: texto corto del solicitante (≤ 64 caracteres, sin controles), guardado y devuelto como texto no confiable (SEC-12). Nunca se usa como nombre de ref ni de archivo.
- **Cupo y rate limit propios** (SEC-TMC-12, ADR-MCP-001 § 6): ≤ 20 por (solicitante, worktree) en una ventana móvil de 24 h y 5 por minuto. Con el cupo lleno, o con la cuota de disco de SEC-TMC-12 llena, la captura **se rechaza** (`quota-exceeded`) con la hora a la que se libera un hueco; **nunca** se borra un punto para hacer sitio.
- **No es una operación protegida**: no lleva previo garantizado (no escribe en el repo), no espera al cerrojo de escritura del repo y **no entra en la pila de `undo`**; si entrara, un `undo` después de un `snapshot` no haría nada.
- **Nunca desplaza al previo garantizado** (NFR-01): la captura manual no consume la reserva de disco del previo garantizado (§ 1) y, en el escritor único, tiene la prioridad de una captura por observación (§ 2), no la del previo.
- **Retención**: los puntos `manual` siguen ADR-TMC-007 como cualquier otro punto; la purga los cuenta para liberar cuota.
- **Solicitante y registro**: el solicitante del plan, congelado en el oplog (ADR-TMC-003 § 5), con canal `mcp`. El timeline lo muestra con su nivel y su etiqueta.
- **Dueña**: la API la implementa la historia dueña de `snapshot` (US-MCP-008) sobre el módulo de la Time Machine, coordinada con su feature.
- **Validación añadida**: 21 capturas manuales del mismo agente en un worktree en 24 h → la 21.ª se rechaza con la hora de liberación y las 20 siguen; un `undo` tras un `snapshot` deshace la operación anterior, no el snapshot; con el disco en el mínimo, un previo garantizado posterior se completa; una etiqueta con U+202E sale escapada en el timeline.

## Enmienda (2026-10-06, US-TMC-004)

Aplicada desde la [Dev Spec de US-TMC-004](../../requirements/features/time-machine/dev-specs/US-TMC-004-captura-continua-git-crudo.md), que implementa el nivel (b). **Decisión del orquestador (2026-10-06), validada por el Arquitecto** (aprobada con ajustes, incorporados). El `status` sigue en `accepted`.

| Cambio | Dónde | Fuente |
|---|---|---|
| **Calma por estado** antes de cada captura: un worktree está en calma cuando el tamaño de su reflog de `HEAD` y su `HEAD` en disco son los que el motor leyó en su último lote persistido, y ningún `git` tiene su índice. Solo sin reflog se usa el respaldo por tiempo (150 ms sin cambios en el directorio Git). Sustituye a "la marca al empezar" como garantía: un `git` terminado antes de la captura ya está en la marca, aunque su notificación llegue tarde | § 2, Consistencia | Bloqueante B1 del Arquitecto |
| **Consistencia por guarda**: la captura compara al empezar y en su punto de validez el reflog de `HEAD`, `HEAD`, la identidad del índice y `index.lock` de cada worktree. Si cambió, se descarta sin fila (`Discarded`) y se repite. Concreta "se descarta si llega un evento de Git durante la lectura" sin depender de la latencia del motor | § 2, Consistencia; Validación 4 | B1 |
| **Ancla posterior** a cada operación protegida y undo (captura `observation` con `cause_operation`): delimita su eco en el motor (ADR-TMC-003, Enmienda 2026-10-06) | § 1, § 2 | D5 |
| **Detección completa** en cada captura: las rutas del motor con su marca de continuidad (escalón 2, ADR-TMC-006 § 5) siguen pendientes en TS-GRP-002/003 | § 2, captura incremental | D7 |
| **Cuotas diferidas**: la cuota del almacén y el hueco "sin espacio" son de US-TMC-022. Mientras tanto, la captura continua se omite (y lo registra) por debajo del suelo de espacio libre de SEC-TMC-12, máx(5 GB, 5 %), y un `ENOSPC` es una captura fallida sin punto | § 2, Cuotas | D8; ajuste 3 |
| **Un hilo de captura por daemon**, en serie; el motor solo le entrega señales por un canal y nunca espera | § 2, Coalescencia | D6 |
| **Fuera del camino del motor**: la captura por observación escribe blobs con un hilo y cede ante nueva actividad del repo (sin fila, sin contar como fallo) salvo tras 6×`M`; la primera captura de un repo espera a la siembra de sus packs (ADR-TMC-001 § 3), en segundo plano con un hilo. El banco del motor falló sin ello y pasa con ello (macOS) | § 2, Coalescencia y contrapresión | D12, D13; segunda validación del Arquitecto |

**Límites declarados**: sin reflog de `HEAD` (`core.logAllRefUpdates=false` o el backend reftable) no hay evento `reset` y la calma es por tiempo; `git checkout -- .`, `git restore` y `git clean` no escriben reflog ni índice y no son eventos (su efecto queda en la siguiente captura y lo anterior sigue restaurable desde el almacén); R2 se mantiene.

**Validación añadida**: los escenarios de US-TMC-004 y el e2e del criterio 3 de M1 (un agente simulado hace `git reset --hard` y lo recupera con `raptor undo`); el banco del motor (INF-GRP-002) con la captura continua activa.

## Enmienda (2026-10-08, US-MCP-008)

**Decisión del orquestador (2026-10-08), validada por Arquitecto y PO** (etiqueta y suelo por security-expert, L1, L2 y S2). Origen: [DS-US-MCP-008](../../requirements/features/mcp/dev-specs/US-MCP-008-dev-spec.md), nota N2.

- **Validación de la etiqueta (corrige la Validación añadida de la Enmienda (2026-10-05, MCP))**: la prueba "una etiqueta con U+202E sale escapada en el timeline" no se sostiene, porque U+202E es un control bidi y la etiqueta se **rechaza** (`invalid-text`). La prueba equivalente usa una etiqueta con un selector de variación (U+FE0F) o U+034F, que la validación admite y que el escape sustituye al mostrarla. La validación del daemon manda (`check_snapshot_label`): 1 a 64 caracteres, longitud en bytes (≤ 256) comprobada antes de recorrer caracteres, sin controles ni caracteres invisibles o de uso privado, sin espacios al principio o al final.
- **Suelo de espacio de la captura manual**: suelo de SEC-TMC-12 más una reserva para el previo garantizado, máx(1 GB, tamaño estimado del worktree) (⚠️ **ASSUMPTION**); se comprueba bajo el cerrojo manual justo antes de capturar y cruzarlo a mitad de captura la aborta (`discarded`, nada borrado). Un previo garantizado posterior nunca espera al cerrojo manual.
- **Prioridad**: una captura manual cede ante un previo y termina dentro de `DEFAULT_PRIOR_DEADLINE`; usa el tope de 50 MB por archivo y los hilos de observación (§ 2).
