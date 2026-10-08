---
id: US-GRD-019
title: "El desarrollador ve quién ejecutó cada commit y a nombre de quién entró cuando no coinciden"
type: us
status: implemented
priority: medium
created: 2026-10-06
updated: 2026-10-08
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRD-018
    - US-GRD-005
    - US-GRP-002
    - US-GRP-007
    - US-GRP-009
tags:
  - guardrails
  - autoria-commits
  - auditoria
  - raptor-events
---

# US-GRD-019: El desarrollador ve quién ejecutó cada commit y a nombre de quién entró cuando no coinciden

## Descripción

**Como** desarrollador orquestador, **quiero** ver en `raptor events` qué agente ejecutó cada commit y a nombre de quién entró, **para** saber qué agente participó en un cambio que firma una persona sin abrir el historial de Git.

**Valor**: BRD BR-26 y D6 (decisión de Rene Bonilla, 2026-10-06). Separa la observación (proceso, sesión, worktree) de la autoría (autor, committer, trailer). Fija el comportamiento objetivo de la pista `inferred` de ADR-GRP-012, que sigue "pendiente de esta política" y no está ratificada: la ratifica Rene Bonilla, no esta historia.

**Alcance (decisión del orquestador, 2026-10-06, validada por el PO)**: MVP, después de US-GRD-018. El Cockpit muestra lo mismo con una historia propia de la feature Cockpit, todavía sin escribir. Exportar es BR-24 (Fase 3).

## Reglas cubiertas

BR-AUTH-005 (presentación combinada y validación de la pista `inferred` contra el trailer; sin pista con `human-author`) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-018 (política y autoría de cada commit), US-GRP-002 (eventos en vivo), US-GRP-007 y US-GRP-009 (quién ejecutó), US-GRD-005 (registro).
- **Externas**: ninguna. La historia del Cockpit que lo presenta la escribe el PO de Cockpit.
- **Transversal**: Windows, macOS y Linux; textos en inglés y español (i18n).

## Criterios de Aceptación

**Escenario: Un commit de un agente a nombre de la persona muestra las dos cosas**

Dado una sesión de Claude Code en el worktree "feat-x" de "demo"
  Y la identidad de Git del usuario es "Ana Pérez"
Cuando la sesión hace un commit con el trailer "Co-Authored-By: Claude"
Entonces `raptor events` muestra "commit de Ana Pérez con Claude Code · feat-x"
  Y la salida para máquinas lleva por separado quién lo ejecutó y su autor, committer y trailer

**Escenario: Un commit sin agente no repite la autoría**

Dado un proceso sin atribuir en el worktree "main" de "demo"
Cuando hace un commit de "Ana Pérez" sin trailer y sin pista inferida
Entonces `raptor events` muestra "commit de Ana Pérez · main" y el actor "sin atribuir"

**Esquema del escenario: La pista inferida se contrasta con el trailer**

Dado el repo "demo" con la política "agents-commit"
  Y un commit sin atribuir con la pista "inferido: Claude Code"
Cuando el commit lleva "<trailer>"
Entonces `raptor events` muestra "<presentación>"
  Y el actor del evento sigue siendo "sin atribuir"

Ejemplos:
| trailer | presentación |
| el trailer "Co-Authored-By: Claude" | la pista, marcada como coincidente con el trailer |
| ningún trailer | la pista, marcada como no confirmada |
| el trailer de otro agente | el coautor del trailer y ninguna pista |

**Escenario: Con human-author no se muestra la pista inferida**

Dado el repo "demo" con la política "human-author"
  Y un commit sin atribuir con la pista "inferido: Claude Code"
Cuando el desarrollador consulta `raptor events`
Entonces el evento muestra el actor "sin atribuir" y ninguna pista inferida

**Escenario: Un commit de un agente sin trailer muestra la diferencia**

Dado el repo "demo" con la política "flexible"
Cuando una sesión de Claude Code en "feat-x" hace un commit de "Ana Pérez" sin trailer
Entonces `raptor events` muestra que lo ejecutó Claude Code en "feat-x", que entró a nombre de "Ana Pérez" y que no lleva trailer

## Requisitos Técnicos

- **Gobierno**: ADR-GRP-012, Enmienda (2026-10-06, autoría de commits) § 2 y § 3; ADR-GRP-013, Enmienda (2026-10-06, autoría declarada); ADR-GRP-016 (capacidad `events.authorship`).
- **Forma**: `GitEventView.authorship` e `InferredAgent.trailer`, con la tabla de identidades y el parser de trailers de US-GRD-018. Detalle en la Dev Spec.

## Diseño y Dev Spec

- **Diseño:** no aplica (la presentación del Cockpit va en su propia historia).
- **Dev Spec:** [DS-US-GRD-018](../dev-specs/US-GRD-018-autoria-commits-persona-y-agente.md) (2026-10-06; compartida por US-GRD-018 y US-GRD-019, esta historia es su PR-B).

## Estado de la implementación (2026-10-08)

Implementado en: PR #144, #147, #151, #175.

- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
