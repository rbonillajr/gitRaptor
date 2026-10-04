---
id: ADR-TMC-006
title: "ADR-TMC-006 — Presupuesto de rendimiento del snapshot previo (NFR-04)"
type: adr
status: accepted
accepted: 2026-10-03
created: 2026-10-03
updated: 2026-10-04
date: 2026-10-03
domain: GRP
feature: time-machine
supersedes: []
superseded_by: null
deciders: [Rene Bonilla]
related:
  adrs: [ADR-GRP-001, ADR-GRP-011, ADR-TMC-001, ADR-TMC-004]
  stories: [US-TMC-001, US-TMC-004, US-TMC-020, SPIKE-TMC-001]
description: "Overhead del snapshot previo menor de 200 ms en p95 en un repo mediano que fija SPIKE-TMC-001, repartido por etapa y medido con el banco de INF-GRP-002"
tags: [adr, time-machine, rendimiento, presupuesto, p95, nfr-04, d-tmc-21, spike]
published: true
---

# ADR-TMC-006 — Presupuesto de rendimiento del snapshot previo (NFR-04)

**Status**: Aceptado · **Fecha**: 2026-10-03 · **Decisores**: Rene Bonilla · **Feature**: Time Machine (F-001-03)

**Decisión de Rene Bonilla (2026-10-03)**: TQ-4 → (a) el escalón 3 (gitoxide solo en el almacén) queda preaprobado si SPIKE-TMC-001 lo exige.

> **Enmienda (2026-10-04, SPIKE-TMC-001)**: el spike lo exigió. El escalón 3 queda **activado** y el 2 pasa al diseño base (§ 5). El reparto (§ 2) y el repo de referencia (§ 3) llevan cifras medidas en macOS. Detalle al final.

## Contexto

NFR-04 pide un overhead del snapshot menor de 200 ms por operación en repos medianos. "Repo mediano" se fija en el spike (a) del BRD (D-TMC-21; Known Risk 3 aceptado). Un repo mayor no se salta el snapshot (US-TMC-020, escenario 3). Motor-local ya mide la frescura por etapa con reloj monótono y un banco en CI (ADR-GRP-011, INF-GRP-002), y su presupuesto de 300 ms no se puede reclamar.

**Pregunta**: ¿qué se mide exactamente, cómo se reparten los 200 ms y qué pasa si no se cumplen?

## Decisión

### 1. Qué se mide

- **Overhead** = tiempo que el snapshot previo garantizado añade a una operación protegida: desde que el daemon acepta la petición hasta que el snapshot es válido (ref en el almacén y fila `completo`). No incluye la validación de permisos ni la operación en sí.
- La **espera en la cola del escritor del almacén** cuenta dentro del overhead. El previo tiene prioridad sobre la captura por observación (ADR-TMC-004 § 2).
- **p95** en las máquinas de referencia de ADR-GRP-011 § 4, con el almacén ya sembrado, medido con 1 y con 10 worktrees activos (US-TMC-020, escenarios 1 y 2). La siembra inicial (ADR-TMC-001 § 3) queda fuera: es un estado explícito ("preparando la Time Machine").
- Las capturas por observación no tienen gate de latencia, pero no pueden empeorar el p95 del motor (ADR-TMC-004 § 2).

### 2. Reparto

Cifras medidas en macOS por SPIKE-TMC-001 (Enmienda 2026-10-04). Cada valor es el p95 de la variante con los escalones 2 y 3 sobre el perfil `M`, con el delta de referencia; ⚠️ **ASSUMPTION** en Linux y Windows hasta que los mida el gate de CI.

| Etapa | Presupuesto p95 | Medido en macOS (p95, dos pasadas) |
|---|---|---|
| Admisión y paso por el canal hasta el componente Time Machine | ≤ 10 ms | No medido (no hay daemon) |
| Detección de cambios desde la última captura (rutas del motor + caché de stat, § 5) | ≤ 5 ms | 0,7–0,9 ms (3,5 ms con 10 worktrees) |
| Anclaje de commits pendientes (normalmente ya anclados por la observación) | ≤ 5 ms | 0 ms |
| Lectura y escritura de blobs nuevos en el almacén | ≤ 90 ms | 84–92 ms (92 en la primera pasada: daría aviso de etapa, no fallo) |
| Construcción de árboles y commit del almacén | ≤ 45 ms | 35–37 ms (42–46 ms con 100 archivos de texto) |
| Ref del almacén y fila del oplog, con sus barreras de durabilidad | ≤ 25 ms | 19–30 ms (tres barreras de ~4 ms sin carga) |
| **Suma de etapas** | **≤ 180 ms** | |
| Margen que ninguna etapa reclama (ruido de la máquina) | 20 ms | |
| **Límite NFR-04** (el gate falla al alcanzarlo) | **< 200 ms** | **142–146 ms** |

- Los p95 por etapa **no se suman**: el gate de NFR-04 se aplica al p95 del total, y cada etapa por encima de su presupuesto solo genera un aviso.
- **Camino rápido**: si nada cambió desde la última captura, el snapshot reutiliza su árbol (ADR-TMC-004 § 1) y solo paga la detección y el registro.
- **Delta de referencia**: hasta 100 archivos cambiados y 20 MB de contenido nuevo desde la última captura (sin cambios tras el spike). Un delta mayor sigue protegido y se reporta aparte: con 1.000 archivos, 357–367 ms; un archivo de 1 GB sin seguimiento, unos 7 s (la CLI y la TUI muestran progreso, US-TMC-020).

### 3. Repo mediano (D-TMC-21)

**Repo de referencia = perfil `M` del generador determinista de SPIKE-TMC-001**, aprobado por Rene Bonilla el 2026-10-04 (D-TMC-21): 10.001 archivos con seguimiento, 316 MB de working tree (3 % de binarios de unos 0,8 MB y 3.000 archivos ignorados), 50.000 commits y 627 MiB de historial. En una muestra de 18 repos públicos queda en torno al p65–p90. El perfil `L` (40.000 archivos, 1 GB, 150.000 commits) queda fuera de la referencia y sirve para el escenario 3 de US-TMC-020. El banco de US-TMC-020 y el de INF-GRP-002 reutilizan el mismo generador: misma semilla, mismos commits, y se genera en un minuto sin descargar nada.

### 4. Medición y gate

- Cada snapshot lleva sus tiempos por etapa en el oplog (diagnóstico local, NFR-03), con el reloj monótono común de ADR-GRP-011 § 3.
- **Banco**: el de INF-GRP-002, ampliado con un escenario "operación protegida con trabajo sin commitear" sobre el repo de referencia, con 1 y con 10 worktrees activos (los mismos de § 1). El trabajo vive en la Dev Spec de US-TMC-020 (dueña del NFR); no hay un INF nuevo.
- **Gates**: p95 total ≥ 200 ms en el repo de referencia, entonces **el CI falla**; una etapa por encima de su presupuesto con el total dentro, aviso con la etapa nombrada; p95 del motor con la Time Machine activa por encima de 300 ms, el CI falla (gate de ADR-GRP-011).

### 5. Si no se cumple (en este orden)

> **Enmienda (2026-10-04, SPIKE-TMC-001)**: los escalones 2 y 3 son **obligatorios** y forman parte del diseño base; el 1 no se adopta. Medición en macOS con el delta de referencia: línea base 1.956 ms; escalón 1, 456–564 ms; escalones 1 y 2, 455–468 ms; escalones 2 y 3, **142–146 ms**. Con `git status` como detección, el camino rápido cuesta 75 ms. Con el escalón 2, 4 ms.

1. ~~Procesos de Git persistentes por almacén (un proceso que recibe objetos en flujo), en lugar de uno por paso.~~ **No se adopta**: un solo `fast-import` no pasa de unos 45 MB/s y no cumple el delta de referencia; el escalón 3 lo supera.
2. **Detección de cambios apoyada en el estado del motor (diseño base).** La detección usa las rutas que el motor ya publicó por worktree desde la marca de la última captura (ADR-GRP-010), más una caché de stat. Reglas:
   - **La continuidad se da por rota mientras no se demuestre.** El motor responde con las rutas, la marca y un sí o un no de continuidad. Ante cualquier duda (hueco, modo degradado por sondeo, reinicio del daemon, desbordamiento de eventos o marca desconocida), la detección es **completa**.
   - Un cambio en las reglas de ignore (`.gitignore`, `info/exclude`, `core.excludesFile`) también rompe la continuidad: destapa archivos sin generar un evento.
   - Un archivo cuyo `mtime` es igual o posterior a la marca (entrada *racy*) cuenta como cambiado.
   - **La detección completa** recorre el working tree con gitoxide, en solo lectura y sin filtros. Nunca con el `git status` del CLI (ADR-GRP-009 § 1). Es más lenta, pero el previo nunca se salta y anota en el oplog que la detección fue completa. El gate se mide con continuidad.
   - El recorrido completo corre además como **verificación periódica**, fuera de la ruta crítica. Si encuentra diferencias, invalida la marca y lo registra como diagnóstico.
   - **Requisito al motor** (TS-GRP-002/003): exponer, por worktree, "rutas cambiadas desde la marca X" con la marca de continuidad. Es una nota de integración pendiente (ver el overview de la feature, § 7.2).
3. **Escritura en el almacén con gitoxide (activado).** Va en el proceso del daemon, con los blobs en paralelo, los árboles con el editor de gitoxide y la ref. Solo toca el almacén, nunca el repo del usuario. Es la excepción a ADR-GRP-001 **preaprobada por Rene** (TQ-4 → a), activada por SPIKE-TMC-001 el 2026-10-04. Dónde vive y con qué reglas: ADR-TMC-002 § 1 y § 2. Lo que escribe en el repo del usuario (undo, redo y restauración) y el mantenimiento del almacén siguen con Git CLI.

## Alternativas consideradas

| Alternativa | En contra | Veredicto |
|---|---|---|
| **Medir el overhead de la operación completa** | Mezcla el coste de la operación del Cockpit o del MCP con el del snapshot | Descartada |
| **Media en lugar de p95** | Oculta las colas que sí nota un agente | Descartada |
| **Gate sin repo de referencia** | NFR-04 no es verificable sin "repo mediano" (D-TMC-21) | Descartada |
| **INF propio para el banco** | Duplicaría INF-GRP-002 y tendría una sola historia dueña (US-TMC-020) | Descartada: se amplía el banco existente |

## Consecuencias

- ✅ NFR-04 pasa a ser verificable y tiene gate en CI, con la etapa culpable nombrada.
- ✅ Reutiliza el reloj, el banco y los runners de motor-local.
- ✅ (Enmienda 2026-10-04) SPIKE-TMC-001 cerró en macOS: el repo de referencia está fijado y el reparto lleva cifras medidas. US-TMC-020 deja de estar bloqueada.
- ✅ Con los escalones 2 y 3, el coste depende del delta y casi nada del tamaño del repo: el perfil `L` cumple el delta de referencia con 127 ms.
- ⚠️ Linux y Windows siguen sin medir. Los cierra el gate de CI de US-TMC-020 en los tres SO.
- ⚠️ El escalón 2 hace que el p95 dependa de la interfaz de continuidad del motor (TS-GRP-002/003). Sin continuidad, el previo hace detección completa y sale del presupuesto.
- ⚠️ El ruido de los runners compartidos puede volver inestable el gate. **Mitigación**: la de ADR-GRP-011 § 4 (runner dedicado o gate sobre la mediana con el p95 como aviso).
- ⚠️ En Windows el coste de lanzar procesos es mayor. **Mitigación** (enmendada): el escalón 3 escribe el almacén en el proceso, sin lanzar Git. Queda por medir el coste de crear miles de objetos sueltos en NTFS y el del antivirus sobre `tm/`.

## Validación

1. SPIKE-TMC-001 entrega la definición de repo mediano y el p95 por etapa en los tres SO. **Hecho en macOS** (2026-10-04). Linux y Windows pasan explícitamente al gate de CI de US-TMC-020.
2. Gate de CI en verde para US-TMC-020 con 1 y con 10 worktrees activos.
3. Sensibilidad: un retardo artificial en la escritura de blobs que pasa el total de 200 ms hace fallar el gate; uno que solo pasa una etapa produce aviso.
4. **Continuidad** (Enmienda): con la marca rota (hueco simulado, cambio de `.gitignore` sin evento o archivo *racy*), el previo hace detección completa y no omite ningún archivo cambiado.

## Referencias

- **Reglas y decisiones**: NFR-04, NFR-05; D-TMC-21. Known Risk 3. BRD § 13, spike (a).
- **ADRs**: ADR-GRP-001, ADR-GRP-011; ADR-TMC-001, ADR-TMC-004.
- **Enablers**: SPIKE-TMC-001 ([resultados](../../requirements/features/time-machine/research/SPIKE-TMC-001-resultados.md)); INF-GRP-002 (se amplía desde la Dev Spec de US-TMC-020).

## Enmienda (2026-10-04, SPIKE-TMC-001)

Aplicada desde § 7 de [SPIKE-TMC-001-resultados.md](../../requirements/features/time-machine/research/SPIKE-TMC-001-resultados.md), medido **solo en macOS**. El `status` sigue en `accepted`. Decisión del orquestador (2026-10-04), validada por el Arquitecto, que pidió ajustes y están incorporados.

| Cambio | Dónde | Fuente |
|---|---|---|
| Escalón 3 activado (gitoxide solo en el almacén, TQ-4 → a). El escalón 1 no se adopta | § 5, Consecuencias | E1; Resultados § 5.1 |
| Escalón 2 en el diseño base: rutas del motor con marca de continuidad, que se da por rota mientras no se demuestre; detección completa con gix (nunca `git status`) ante cualquier duda; verificación periódica que invalida la marca | § 5, Validación 4 | E2; revisión del Arquitecto |
| Reparto medido: detección ≤ 5, anclaje ≤ 5, blobs ≤ 90, árboles + commit ≤ 45, ref + oplog ≤ 25 (suma 180, margen 20). Los p95 de cada etapa no se suman. El spike proponía detección ≤ 15, árboles ≤ 40 y ref + oplog ≤ 20. Se ajustó porque árboles midió 42–46 ms con 100 archivos y ref + oplog 30 ms con carga | § 2 | E3; Resultados § 5.1; revisión del Arquitecto |
| Repo de referencia = perfil `M` (D-TMC-21, aprobado por Rene), con el generador reutilizado | § 3 | E4; Resultados § 3 |
| La espera en la cola del escritor cuenta en el overhead; prioridad del previo | § 1 | E8 |
| Consecuencias y Validación: macOS hecho; Linux y Windows al gate de CI de US-TMC-020 | Consecuencias, Validación | E12; Resultados § 8 |
