---
id: US-TMC-019
title: "El repo sigue recuperable aunque GitRaptor muera a mitad de un snapshot o de un undo"
type: us
status: implemented
priority: high
created: 2026-10-03
updated: 2026-10-09
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
- **Dev Spec:** sin Dev Spec; el plano es el [Brief de US-TMC-019](../../../../dev-briefs/us-tmc-019-interruption-robustness.md) (Brief y contrato de `/implement`).

## Dependencias

- **Historias**: US-TMC-001, US-TMC-002, US-TMC-009.
- **Externas**: ninguna. Las pruebas de caos (NFR-12) matan el proceso en puntos distintos de cada operación.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.

## Estado de la implementación (2026-10-09)

Implementado en: PR #247.

- **Hecho:** los seis escenarios, como tests de proceso con el `raptor` real en repos y perfiles temporales: `apps/cli/tests/tm_interruption.rs` (macOS y Linux). Los cortes son puntos de fallo con nombre (feature `chaos`) y `SIGKILL`, sin `sleep` fijos, y la huella de INF-GRP-001 comprueba los dos worktrees.
  - **Escenario 1:** nuevos puntos `capture:pending` y `capture:ref` en las capturas de observación y de hook. El snapshot cortado queda `discarded` y sin ref, no figura en el timeline y una restauración a él se rechaza sin cambiar nada.
  - **Escenario 2:** un undo o una restauración cortados a mitad quedan `interrupted`, y `raptor undo` devuelve el estado previo. El aviso pendiente se entrega por fin con el método nuevo `timemachine.notices`; antes se anotaba en el oplog y nadie lo leía. Llega una sola vez y solo a un cliente que pasa la puerta del `.git` (`tm_scope_for`) de ese worktree, nunca a otro worktree, a otro repo ni a una carpeta que el repo no registra. `raptor undo`, `restore` y `timeline` lo muestran por stderr, en inglés y en español.
  - **Escenarios 3 a 5:** ya los cumplían la recuperación (TS-TMC-002) y el aplicador (TS-TMC-003); quedan fijados como regresión.
  - **Escenario 6:** un arranque limpio no da aviso, y el timeline y el repo quedan igual.
- **Decisiones del orquestador (2026-10-09), validadas por Arquitecto y PO y aprobadas por el coordinador:** D2, método `timemachine.notices` (no se ofrece por MCP); D3, entrega al menos una vez; D4, superficies de la CLI; D5, un fallo detectado se informa en la respuesta sin aviso pendiente. Están en la enmienda «(2026-10-09, US-TMC-019)» de [ADR-TMC-003](../../../../architecture/decisions/ADR-TMC-003-oplog-diario-recuperacion.md). Sin migración del oplog.
- **Pendiente** (fuera de esta historia):
  - El aviso en la TUI y por MCP. Lo heredan US-CKP-014 (Deshacer en la TUI) y US-MCP-012 (undo por MCP) como dependencia.
  - El aviso en el stream en vivo.
  - Los puntos de caos en `finish_manual`.
  - El aviso de un lock propio conservado por identidad desconocida.
  - L-01 de `/security-review` (Low): el aviso lo consume el primer cliente del worktree, aunque sea un agente que corre `raptor timeline`, y la persona puede no verlo. No hay pérdida de datos: el timeline sigue mostrando `interrupted` y `raptor undo` vuelve al previo. Pendiente: marcarlo entregado solo cuando el solicitante sea la persona.
  - Windows: *Pendiente: etapa de validación multiplataforma*. Los tests de proceso son solo Unix, como `tm_chaos`.
