---
id: US-TMC-020
title: "El desarrollador y sus agentes no notan el coste de los snapshots"
type: us
status: draft
priority: medium
created: 2026-10-03
updated: 2026-10-04
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-001, US-TMC-004]
covers: [D-TMC-21]
blocked_by: []
tags: [time-machine, rendimiento, nfr-04]
---

# US-TMC-020: El desarrollador y sus agentes no notan el coste de los snapshots

## Descripción

**Como** desarrollador orquestador
**Quiero** que guardar un snapshot añada menos de 200 ms a cada operación en un repo mediano
**Para** no tener motivos para desactivar la red de seguridad (NFR-04)

**Valor**: un snapshot lento frena a los agentes y el usuario acaba apagándolo (R1).

## Reglas cubiertas

NFR-04 · D-TMC-21 ("repo mediano" = perfil `M`, D-TMC-21, cerrada el 2026-10-04) — ver [context.md](../context.md)

## Criterios de Aceptación

**Escenario: Overhead dentro del objetivo**

Dado un repo mediano de referencia con trabajo sin commitear en un worktree
Cuando se lanza desde GitRaptor una operación que lo modifica
Entonces el snapshot previo añade menos de 200 ms a la operación

**Escenario: Con 10 worktrees activos**

Dado el repo de referencia con 10 worktrees activos con cambios
Cuando se lanza desde GitRaptor una operación en uno de ellos
Entonces el snapshot previo sigue añadiendo menos de 200 ms

**Escenario: Un repo fuera de la referencia no deja de estar protegido**

Dado un repo mayor que el de referencia
Cuando se lanza desde GitRaptor una operación que lo modifica
Entonces la operación sigue precedida de su snapshot previo aunque tarde más de 200 ms
Y en la CLI y en la TUI el desarrollador sabe que la operación sigue en curso mientras se guarda el snapshot previo

## Requisitos Técnicos

- Presupuesto y medición de ADR-TMC-006: p95 < 200 ms del snapshot previo, con almacén sembrado, en el repo de referencia: el **perfil `M`** del generador de SPIKE-TMC-001 (10.001 archivos, 316 MB de working tree, 50.000 commits, 627 MiB de historial; D-TMC-21). El escenario 3 se prueba con el **perfil `L`** (40.000 archivos, 1 GB, 150.000 commits).
- El diseño base ya incluye los escalones 2 y 3 de ADR-TMC-006 § 5 (detección con el estado del motor y escritura del almacén con gitoxide): sin ellos el gate no se cumple (SPIKE-TMC-001 § 5.1). Reparto por etapa: el de la Enmienda de ADR-TMC-006 § 2.
- Ampliar el banco de INF-GRP-002 con el escenario de operación protegida con trabajo sin commitear, con 1 y 10 worktrees; gate de CI y aviso por etapa.
- **Progreso** (E11 de SPIKE-TMC-001): si el snapshot previo supera ~1 s (un archivo de 1 GB tarda unos 7 s), la CLI y la TUI muestran progreso. El umbral es un valor de diseño y el aspecto queda pendiente de diseño. El MCP queda fuera: su respuesta llega al terminar.
- Linux y Windows no se midieron en el spike: el gate de CI de esta historia es el que lo cierra en los tres SO.

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-001, US-TMC-004.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
