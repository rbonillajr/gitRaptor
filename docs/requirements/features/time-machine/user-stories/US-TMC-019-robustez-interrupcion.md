---
id: US-TMC-019
title: "El repo sigue recuperable aunque GitRaptor muera a mitad de un snapshot o de un undo"
type: us
status: draft
priority: high
created: 2026-10-03
updated: 2026-10-03
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-001, US-TMC-002, US-TMC-009]
covers: [BR-TMC-EDGE-003]
blocked_by: []
tags: [time-machine, robustez, nfr-12]
---

# US-TMC-019: El repo sigue recuperable aunque GitRaptor muera a mitad de un snapshot o de un undo

## Descripción

**Como** desarrollador orquestador
**Quiero** que una interrupción en mitad de un snapshot, un undo o una restauración deje mi repo recuperable
**Para** no perder trabajo justo en las operaciones de riesgo (NFR-01, NFR-12)

**Valor**: los fallos ocurren en el peor momento; la Time Machine tiene que aguantarlos.

## Reglas cubiertas

BR-TMC-EDGE-003 (interrupción a mitad de operación) — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Interrupción durante un snapshot**

Dado un snapshot de "feat-login" en curso
Cuando el proceso de GitRaptor muere antes de terminarlo
Entonces el worktree "feat-login" no cambia
  Y ese snapshot incompleto no figura como punto para restaurar

**Escenario: Interrupción durante un undo**

Dado un undo en curso en "feat-login"
Cuando el proceso de GitRaptor muere a mitad del undo
Entonces "feat-login" se puede recuperar al estado previo al undo
  Y al volver a arrancar, el solicitante recibe el aviso de lo ocurrido

**Escenario: Interrupción durante una restauración**

Dado una restauración en curso de "feat-login"
Cuando el proceso de GitRaptor muere a mitad de la restauración
Entonces "feat-login" se puede recuperar al estado previo a la restauración

**Escenario: Tras recuperar una interrupción, el undo funciona con normalidad**

Dado que un undo de "feat-login" se interrumpió y el repo se recuperó al estado previo
Cuando un solicitante sin atribuir vuelve a pedir deshacer desde "feat-login"
Entonces se deshace la última operación de "feat-login"

**Escenario: Sin interrupciones previas no hay aviso de recuperación**

Dado que GitRaptor se cerró sin ninguna operación a medias
Cuando vuelve a arrancar
Entonces el solicitante no recibe ningún aviso de recuperación
  Y el historial de la Time Machine está completo

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-001, US-TMC-002, US-TMC-009.
- **Externas**: ninguna. Las pruebas de caos (NFR-12) matan el proceso en puntos distintos de cada operación.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
