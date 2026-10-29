---
id: US-DSK-005
title: "El usuario ve crecer la rama de cada agente como un carril sobre la rama base"
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
    - US-CKP-022
    - US-DSK-004
  specs:
    - DS-US-CKP-022
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-1
  - grafo
  - must
---

# US-DSK-005: El usuario ve crecer la rama de cada agente como un carril sobre la rama base

## Descripción

**Como** usuario con varios agentes en paralelo, **quiero** ver un carril por agente que nace en la rama base y crece con cada commit, **para** entender de un vistazo cuánto se ha alejado cada agente y cuándo conviene integrar su trabajo.

**Valor**: BR-27 (UI-1, Must); BR-05. Nivel **UI-1**.

## Reglas

- Las del grafo del Cockpit: BR-CKP-CALC-004 (carril desde el merge-base, ventana acotada), BR-CKP-EDGE-001 (sin carriles cuando no hay base) y "sin atribuir" cuando el motor no sabe qué agente hizo un commit (Q-CKP-4).
- El grafo de UI-1 es el de los agentes; el grafo completo de un cliente Git (todas las ramas y etiquetas) es UI-2 (US-DSK-012).

## Dependencias

- **Reutiliza**: US-CKP-022 y su Dev Spec DS-US-CKP-022; `graph.lanes` (DEP-CKP-2) y, si existe, `graph.commit-actor`.
- **Historias**: US-DSK-004 (colores y nombres de agente de la flota).
- **Qué falta en el motor**: nada propio si US-CKP-022 está implementada; si no, la consulta de carriles llega con ella.

## Criterios de Aceptación

**Escenario: Un carril por agente desde su merge-base**

Dado "main" como base y "feat-pagos" de "claude-1" y "feat-login" de "claude-2" con commits propios
Cuando el usuario abre el grafo de "tienda"
Entonces hay un carril por rama desde su merge-base con "main", cada uno con el nombre de su agente

**Escenario: El carril crece en vivo**

Dado el grafo de "tienda" a la vista
Cuando "claude-2" crea un commit en "feat-login"
Entonces el carril de "feat-login" muestra el commit nuevo sin que el usuario recargue

**Escenario: Commit sin agente conocido**

Dado que el motor no sabe qué agente hizo un commit de "feat-pagos"
Cuando el usuario consulta ese commit
Entonces su autor figura como "sin atribuir"

**Escenario: La rama base no existe**

Dado "develop" configurada como base y sin esa rama en el repo
Cuando el usuario abre el grafo
Entonces no hay carriles
  Y el usuario sabe que la rama base "develop" no se encuentra

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** [design-flow.md](../design/design-flow.md) (DSF-DSK-001), grafo de carriles; paleta por carril de DSYS-GRP-001.
- **Dev Spec:** pendiente.
