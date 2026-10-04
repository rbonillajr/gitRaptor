---
id: ADR-TMC-004
title: "ADR-TMC-004 — Cobertura en dos niveles: snapshot previo garantizado, captura por observación y previo vía hook"
type: adr
status: proposed
created: 2026-10-03
updated: 2026-10-03
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

**Status**: Propuesto · **Fecha**: 2026-10-03 · **Decisores**: Rene Bonilla · **Feature**: Time Machine (F-001-03)

## Contexto

D-TMC-10 y BR-TMC-CONS-003 fijan dos niveles. (a) **Garantizado**: snapshot previo antes de toda operación lanzada por GitRaptor (CLI, TUI, MCP) y de cada undo, redo y restauración; si el snapshot falla, la operación no se ejecuta (BR-TMC-CONS-001). (b) **Por observación**: lo hecho con Git crudo o en el editor se captura a partir de lo que observa el motor y, cuando existan, con un snapshot previo vía los hooks de Guardrails (F-001-04), sin depender de ellos. La Time Machine no instala hooks (Q22). Cada punto del timeline declara su nivel. El motor publica cambios con un debounce de 75 ms y un presupuesto de 300 ms que nadie puede reclamar (ADR-GRP-010, ADR-GRP-011).

**Pregunta**: ¿cómo se garantiza el nivel (a) por construcción, con qué cadencia se captura el nivel (b) sin saturar la máquina ni el presupuesto del motor, y qué contrato se ofrece a Guardrails?

## Decisión

### 1. Nivel (a): la operación protegida es el único camino de escritura

- Toda operación que modifica el repo y lanza una superficie de GitRaptor entra por un único comando del canal del daemon, la **operación protegida**: (1) intención en el oplog, (2) snapshot previo del ámbito, (3) ejecución, (4) registro (ADR-TMC-002 § 5, ADR-TMC-003 § 3). La ejecución de una operación de usuario la hace el ejecutor del daemon de F-001-02/05, con los hooks del usuario; la Time Machine solo aporta 1, 2 y 4. No existe otra vía de escritura en el contrato de `crates/api`: un cliente no puede ejecutar una operación sin snapshot porque no tiene con qué.
- Undo, redo y restauración son operaciones protegidas de la propia Time Machine.
- **Reutilización**: si desde la última captura válida del ámbito no cambió nada (stat del working tree, índice y refs), el snapshot previo reutiliza su árbol y solo añade una fila con nivel `previo_garantizado`. Es el camino rápido de ADR-TMC-006.
- **Fallo**: disco lleno, almacén no disponible, tiempo máximo agotado o daemon caído, entonces la operación se aborta con el motivo y el repo no cambia (US-TMC-001, escenario 4). El snapshot previo tiene una reserva de disco propia que la captura por observación no puede consumir (SEC-TMC-12). Un repo mayor que el de referencia **no** se salta el snapshot: tarda más (US-TMC-020, escenario 3).

### 2. Nivel (b): captura por observación

- **Fuente**: dentro del daemon, la Time Machine se suscribe a los eventos que el motor **ya publicó** (ADR-GRP-010 § 4). Nunca lee nada en la ruta crítica del motor y nunca lo retrasa: el presupuesto de ADR-GRP-011 no se reparte con la Time Machine.
- **Disparadores por worktree**:
  - **Cambios de archivos**: captura cuando el worktree lleva `Q` sin cambios, o como mucho cada `M` durante una actividad continua. ⚠️ **ASSUMPTION**: `Q` = 1 s y `M` = 5 s, valores internos que SPIKE-TMC-001 mide y ajusta.
  - **Eventos de Git** (HEAD, ramas, índice, worktrees): captura inmediata del estado resultante y anclaje de los commits implicados en el almacén (ADR-TMC-001 § 3).
- **Coalescencia y contrapresión**: una captura en curso por worktree y un escritor del almacén por repo; si se acumula trabajo, se conserva solo la petición más reciente por worktree. Captura incremental: solo se leen y guardan las rutas cambiadas desde la captura anterior, más un recorrido de stat que detecta lo que el motor no notificó.
- **Consistencia**: una captura guarda la marca del motor al empezar. Si durante la lectura llega un evento de Git de ese worktree, la captura se descarta y se repite, para que nunca mezcle el estado de antes y el de después de una operación.
- **Fallo**: una captura que falla no crea punto; el timeline muestra el cambio sin punto recuperable (US-TMC-004, escenario 4).
- **Cuotas**: al alcanzar la cuota del repo o el mínimo de espacio libre, la captura por observación se detiene con un hueco "sin espacio" declarado (SEC-TMC-12; cifras en TQ-5).
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

## Referencias

- **Reglas**: BR-TMC-CONS-001, BR-TMC-CONS-002, BR-TMC-CONS-003, BR-TMC-EDGE-002; D-TMC-9, D-TMC-10. Q22. Riesgo R2.
- **ADRs**: ADR-GRP-005, ADR-GRP-010, ADR-GRP-011, ADR-GRP-012, ADR-GRP-013; ADR-TMC-001, ADR-TMC-002, ADR-TMC-003, ADR-TMC-006.
- **Enablers**: TS-TMC-001, TS-TMC-004, SPIKE-TMC-001. **Features**: F-001-02, F-001-04, F-001-05.
