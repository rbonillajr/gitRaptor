---
id: ADR-TMC-006
title: "ADR-TMC-006 — Presupuesto de rendimiento del snapshot previo (NFR-04)"
type: adr
status: accepted
accepted: 2026-10-03
created: 2026-10-03
updated: 2026-10-03
date: 2026-10-03
domain: GRP
feature: time-machine
supersedes: []
superseded_by: null
deciders: [Rene Bonilla]
related:
  adrs: [ADR-GRP-001, ADR-GRP-011, ADR-TMC-001, ADR-TMC-004]
  stories: [US-TMC-001, US-TMC-004, US-TMC-020]
description: "Overhead del snapshot previo menor de 200 ms en p95 en un repo mediano que fija SPIKE-TMC-001, repartido por etapa y medido con el banco de INF-GRP-002"
tags: [adr, time-machine, rendimiento, presupuesto, p95, nfr-04, d-tmc-21, spike]
published: true
---

# ADR-TMC-006 — Presupuesto de rendimiento del snapshot previo (NFR-04)

**Status**: Aceptado · **Fecha**: 2026-10-03 · **Decisores**: Rene Bonilla · **Feature**: Time Machine (F-001-03)

**Decisión de Rene Bonilla (2026-10-03)**: TQ-4 → (a) el escalón 3 (gitoxide solo en el almacén) queda preaprobado si SPIKE-TMC-001 lo exige.

## Contexto

NFR-04 pide un overhead del snapshot menor de 200 ms por operación en repos medianos. "Repo mediano" se fija en el spike (a) del BRD (D-TMC-21; Known Risk 3 aceptado). Un repo mayor no se salta el snapshot (US-TMC-020, escenario 3). Motor-local ya mide la frescura por etapa con reloj monótono y un banco en CI (ADR-GRP-011, INF-GRP-002), y su presupuesto de 300 ms no se puede reclamar.

**Pregunta**: ¿qué se mide exactamente, cómo se reparten los 200 ms y qué pasa si no se cumplen?

## Decisión

### 1. Qué se mide

- **Overhead** = tiempo que el snapshot previo garantizado añade a una operación protegida: desde que el daemon acepta la petición hasta que el snapshot es válido (ref en el almacén y fila `completo`). No incluye la validación de permisos ni la operación en sí.
- **p95** en las máquinas de referencia de ADR-GRP-011 § 4, con el almacén ya sembrado, medido con 1 y con 10 worktrees activos (US-TMC-020, escenarios 1 y 2). La siembra inicial (ADR-TMC-001 § 3) queda fuera: es un estado explícito ("preparando la Time Machine").
- Las capturas por observación no tienen gate de latencia, pero no pueden empeorar el p95 del motor (ADR-TMC-004 § 2).

### 2. Reparto

⚠️ **ASSUMPTION**: cifras de diseño; SPIKE-TMC-001 las mide y este ADR se actualiza.

| Etapa | Presupuesto p95 |
|---|---|
| Admisión y paso por el canal hasta el componente Time Machine | ≤ 10 ms |
| Detección de cambios desde la última captura (stat del working tree, índice y refs) | ≤ 50 ms |
| Lectura y escritura de blobs nuevos en el almacén | ≤ 60 ms |
| Construcción de árboles con el índice temporal del almacén | ≤ 30 ms |
| Anclaje de commits pendientes (normalmente ya anclados por la observación) | ≤ 10 ms |
| Ref del almacén y fila del oplog con sincronización a disco | ≤ 20 ms |
| **Suma de etapas** | **≤ 180 ms** |
| Margen que ninguna etapa reclama (ruido de la máquina) | 20 ms |
| **Límite NFR-04** (el gate falla al alcanzarlo) | **< 200 ms** |

- **Camino rápido**: si nada cambió desde la última captura, el snapshot reutiliza su árbol (ADR-TMC-004 § 1) y solo paga la detección y el registro.
- **Delta de referencia**: ⚠️ **ASSUMPTION**: el reparto supone hasta 100 archivos cambiados y 20 MB de contenido nuevo desde la última captura. Un delta mayor sigue protegido y se reporta aparte.

### 3. Repo mediano (D-TMC-21)

Lo fija **SPIKE-TMC-001** con números (archivos con seguimiento, tamaño del working tree, commits y tamaño del historial) a partir de un corpus de repos públicos medidos en las máquinas de referencia. Hipótesis de partida: ⚠️ **ASSUMPTION**: unos 10.000 archivos con seguimiento, 300 MB de working tree y 50.000 commits. Rene aprueba la cifra al cerrar el spike.

### 4. Medición y gate

- Cada snapshot lleva sus tiempos por etapa en el oplog (diagnóstico local, NFR-03), con el reloj monótono común de ADR-GRP-011 § 3.
- **Banco**: el de INF-GRP-002, ampliado con un escenario "operación protegida con trabajo sin commitear" sobre el repo de referencia, con 1 y con 10 worktrees activos (los mismos de § 1). El trabajo vive en la Dev Spec de US-TMC-020 (dueña del NFR); no hay un INF nuevo.
- **Gates**: p95 total ≥ 200 ms en el repo de referencia, entonces **el CI falla**; una etapa por encima de su presupuesto con el total dentro, aviso con la etapa nombrada; p95 del motor con la Time Machine activa por encima de 300 ms, el CI falla (gate de ADR-GRP-011).

### 5. Si no se cumple (en este orden)

1. Procesos de Git persistentes por almacén (un proceso que recibe objetos en flujo), en lugar de uno por paso.
2. Detección de cambios apoyada en el estado en memoria del motor y la caché de stat, con el recorrido completo solo como verificación periódica.
3. Escritura en el almacén con gitoxide (solo en el almacén, nunca en el repo del usuario). Excepción a ADR-GRP-001 **preaprobada por Rene** (TQ-4 → a): se activa solo si los escalones 1 y 2 no bastan, y se anota en este ADR.

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
- ⚠️ Mientras SPIKE-TMC-001 no cierre, US-TMC-020 sigue bloqueada y las cifras son hipótesis.
- ⚠️ El ruido de los runners compartidos puede volver inestable el gate. **Mitigación**: la de ADR-GRP-011 § 4 (runner dedicado o gate sobre la mediana con el p95 como aviso).
- ⚠️ En Windows el coste de lanzar procesos es mayor. **Mitigación**: primer escalón de § 5.

## Validación

1. SPIKE-TMC-001 entrega la definición de repo mediano y el p95 por etapa en los tres SO.
2. Gate de CI en verde para US-TMC-020 con 1 y con 10 worktrees activos.
3. Sensibilidad: un retardo artificial en la escritura de blobs que pasa el total de 200 ms hace fallar el gate; uno que solo pasa una etapa produce aviso.

## Referencias

- **Reglas y decisiones**: NFR-04, NFR-05; D-TMC-21. Known Risk 3. BRD § 13, spike (a).
- **ADRs**: ADR-GRP-001, ADR-GRP-011; ADR-TMC-001, ADR-TMC-004.
- **Enablers**: SPIKE-TMC-001; INF-GRP-002 (se amplía desde la Dev Spec de US-TMC-020).
