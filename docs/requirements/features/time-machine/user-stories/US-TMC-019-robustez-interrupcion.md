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
covers: [BR-TMC-EDGE-003, BR-TMC-CONS-004, D-TMC-25]
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

BR-TMC-EDGE-003 (interrupción a mitad de operación; fallo detectado sin rollback automático, TQ-10) · BR-TMC-CONS-004 (excepción: liberar el bloqueo propio al arrancar) · D-TMC-25 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Interrupción durante un snapshot**

Dado un snapshot de "feat-login" en curso
Cuando el proceso de GitRaptor muere antes de terminarlo
Entonces el worktree "feat-login" no cambia
  Y ese snapshot incompleto no figura como punto para restaurar

**Escenario: Interrupción durante un undo o una restauración**

Dado un undo o una restauración en curso en "feat-login"
Cuando el proceso de GitRaptor muere a mitad de la operación
Entonces "feat-login" se puede recuperar al estado previo a esa operación
  Y al volver a arrancar, el solicitante recibe el aviso de lo ocurrido

**Escenario: Un fallo detectado a mitad de un undo lo deja interrumpido, sin vuelta atrás automática**

Dado un undo en curso en "feat-login"
Cuando un archivo de "feat-login" no se puede escribir porque otro programa lo tiene bloqueado
Entonces el undo se detiene y figura como "interrumpido" en el historial de la Time Machine
  Y la Time Machine no deshace por su cuenta lo que ya aplicó
  Y el solicitante recibe el aviso de que un undo devuelve "feat-login" al estado previo

**Escenario: Al arrancar se libera solo el bloqueo de Git propio**

Dado que GitRaptor murió dejando un bloqueo de Git creado por la Time Machine en "feat-login"
  Y otro programa mantiene su propio bloqueo de Git en "feat-pagos"
Cuando GitRaptor vuelve a arrancar
Entonces el bloqueo de "feat-login" se libera sin cambiar ningún archivo
  Y el bloqueo de "feat-pagos" sigue en su sitio

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

- Recuperación al arrancar del daemon según ADR-TMC-003 § 6 (TS-TMC-002); nunca reanuda ni revierte por su cuenta.
- Aviso pendiente entregado al siguiente cliente del worktree y visible en el timeline; sin interrupciones no hay aviso.
- Verificación: muerte forzada en cada transición y paso con INF-TMC-001.

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-001, US-TMC-002, US-TMC-009.
- **Externas**: ninguna. Las pruebas de caos (NFR-12) matan el proceso en puntos distintos de cada operación.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
