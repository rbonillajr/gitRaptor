---
id: US-DSK-004
title: "El usuario ve en vivo qué agente trabaja en cada copia de su repo y cuál pide atención"
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
    - US-CKP-001
    - US-CKP-002
    - US-CKP-026
    - US-GRP-007
    - US-GRP-012
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-1
  - flota
  - must
---

# US-DSK-004: El usuario ve en vivo qué agente trabaja en cada copia de su repo y cuál pide atención

## Descripción

**Como** usuario que deja a varios agentes trabajando, **quiero** una tarjeta por copia de trabajo con su agente, su estado, lo que cambió y cuánto se aleja de la base, actualizada sola, **para** saber sin preguntar a cada agente cuál terminó, cuál sigue y cuál necesita que yo mire.

**Valor**: BR-27 (UI-1, Must); BR-04. Nivel **UI-1**. Patrón P4 del [benchmark](../research/benchmark-2026-10.md).

## Reglas

- Las mismas reglas de presentación de la flota que la TUI: BR-CKP-WF-001, el orden de atención de US-CKP-002 y la autoría de US-CKP-026 (ver [business-rules del Cockpit](../../cockpit/business-rules.md)).
- Las sesiones terminadas no llenan la vista (Q-CKP-3).
- Un repo sin agentes es un estado válido: se ven las copias de trabajo sin agente (Q-DSK-13).

## Dependencias

- **Reutiliza**: US-CKP-001 (flota en vivo), US-CKP-002 (orden de atención), US-CKP-026 (autoría), US-GRP-007 (sesiones de Claude Code), US-GRP-012 (ahead/behind); `scope.subscribe`, `scope.snapshot`, `events.subscribe`.
- **Historias**: US-DSK-003 (estado del motor).
- **Qué falta en el motor**: nada nuevo para leer; DEP-DSK-2 (identidad de la app). transversal (lo define el Arquitecto).

## Criterios de Aceptación

**Escenario: Una tarjeta por copia de trabajo**

Dado "tienda" con "feat-pagos" trabajada por "claude-1" y "feat-login" por "claude-2"
Cuando el usuario abre "tienda"
Entonces ve una tarjeta por copia de trabajo con su agente, su estado, sus archivos cambiados y su ahead/behind contra la base

**Escenario: Cambio en vivo**

Dado la flota de "tienda" a la vista
Cuando "claude-1" crea un commit en "feat-pagos"
Entonces la tarjeta de "feat-pagos" refleja el commit sin que el usuario recargue

**Escenario: Primero lo que pide atención**

Dado "feat-login" con un conflicto previsto y "feat-pagos" sin novedades
Cuando el usuario mira la flota
Entonces "feat-login" aparece antes que "feat-pagos" con el motivo de atención

**Escenario: Repo sin agentes**

Dado "notas" observado sin ninguna sesión de agente
Cuando el usuario abre "notas"
Entonces ve su copia de trabajo principal con su estado
  Y sabe que no hay agentes trabajando en ese repo

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** [design-flow.md](../design/design-flow.md) (DSF-DSK-001), vista de la flota.
- **Dev Spec:** pendiente.
