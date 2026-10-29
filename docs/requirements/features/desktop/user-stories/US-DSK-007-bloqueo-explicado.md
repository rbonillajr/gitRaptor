---
id: US-DSK-007
title: "El usuario entiende por qué GitRaptor frenó una operación y cómo hacerla a conciencia"
type: us
status: draft
priority: high
created: 2026-10-28
updated: 2026-10-28
feature: desktop
source: inline
related:
  context:
    - CTX-DSK-001
  stories:
    - US-CKP-019
    - US-GRD-005
    - US-GRD-006
    - US-GRD-008
    - US-DSK-002
  specs:
    - DS-US-CKP-019
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-1
  - guardrails
  - must
---

# US-DSK-007: El usuario entiende por qué GitRaptor frenó una operación y cómo hacerla a conciencia

## Descripción

**Como** desarrollador novato con un repo protegido, **quiero** ver cada operación que GitRaptor frenó, quién la intentó, qué regla la frenó y qué puedo hacer, **para** confiar en la protección sin quedarme atascado cuando la operación sí era la que yo quería.

**Valor**: BR-27 (UI-1, Must); BR-11, BR-12. Nivel **UI-1**.

## Reglas

- Las de US-CKP-019: el motivo se explica en lenguaje llano con la regla que lo frenó y quién lo intentó.
- La excepción consciente es un comando reservado al humano que relaja una regla, con anuncio, ventana cancelable y auditoría (D5, D10; ADR-GRD-007). La app no la hace con un control más débil que la terminal (Q-DSK-3).
- En UI-1 la app explica el bloqueo y entrega el comando exacto de la excepción para la terminal (recomendación de Q-DSK-9, **pendiente de Rene**).
- Un agente nunca obtiene una excepción a través de la app.

## Dependencias

- **Reutiliza**: US-CKP-019 (DS-US-CKP-019), US-GRD-005 (registro de bloqueos), US-GRD-006 (excepción consciente, `raptor guard exec`), US-GRD-008 (ramas y rutas protegidas); `guard.log`, `guard.status`, `audit.list`, `reserved.audit`.
- **Historias**: US-DSK-002 (el repo está protegido).
- **Qué falta en el motor**: si Rene elige hacer la excepción dentro de la app (Q-DSK-9 opción a), DEP-DSK-1. transversal (lo define el Arquitecto).

## Criterios de Aceptación

**Escenario: Un bloqueo explicado**

Dado "tienda" protegido con "main" como rama protegida
Cuando "claude-1" intenta un force-push a "main" y se deniega
Entonces el usuario ve el bloqueo con el agente que lo intentó, la operación, la regla que lo frenó y cuándo ocurrió

**Escenario: El usuario quiere hacer la operación a conciencia**

Dado el bloqueo del force-push de "claude-1" a "main"
Cuando el usuario decide hacer esa operación él mismo como excepción
Entonces recibe el comando exacto para hacerla como excepción consciente en una terminal
  Y cuando la ejecuta, la excepción queda registrada y la app la muestra

**Escenario: Una excepción anunciada se puede cancelar**

Dado una excepción consciente anunciada y dentro de su ventana de cancelación
Cuando el usuario la cancela
Entonces la operación no se ejecuta y la cancelación queda registrada

**Escenario: El registro de bloqueos no está disponible**

Dado que el motor no puede leer el registro de bloqueos de "tienda"
Cuando el usuario consulta los bloqueos
Entonces sabe que la lista no está disponible y que la protección sigue activa

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** [design-flow.md](../design/design-flow.md) (DSF-DSK-001), bloqueo explicado.
- **Dev Spec:** pendiente.
