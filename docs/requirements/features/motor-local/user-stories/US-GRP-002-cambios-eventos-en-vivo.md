---
id: US-GRP-002
title: "El desarrollador ve los cambios y eventos de Git de sus worktrees casi al instante"
type: us
status: implemented
priority: high
created: 2026-10-03
updated: 2026-10-08
feature: motor-local
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  stories:
    - US-GRP-001
tags:
  - motor-local
  - observacion
  - eventos
---

# US-GRP-002: El desarrollador ve los cambios y eventos de Git de sus worktrees casi al instante

## Descripción

**Como** desarrollador orquestador, **quiero** que cada cambio de archivo y cada evento de Git de mis worktrees quede reflejado con su momento y su actor, **para** saber lo que pasa mientras pasa, sin refrescar nada a mano.

**Valor**: la base de "qué cambió, cuándo y quién" para el Cockpit y la Time Machine.

## Reglas cubiertas

BR-CONS-003 ("sin atribuir" por defecto) — ver [business-rules.md](../business-rules.md). Objetivos NFR-04 (frescura) y NFR-05 (escala) del contexto.

## Dependencias

- **Historias**: US-GRP-001.
- **Externas**: Cockpit F-001-02 comparte el presupuesto de frescura de NFR-04 (< 500 ms de extremo a extremo); el reparto lo fija el plan técnico.
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Escenario: Un cambio de archivo se refleja casi al instante**

Dado el repo "demo" observado con el worktree "feat-login" sin cambios
Cuando se modifica el archivo "login.txt" en "feat-login"
Entonces el estado del motor refleja "login.txt" como modificado en "feat-login" dentro de la parte del motor del presupuesto de frescura de NFR-04 (< 500 ms de extremo a extremo; el reparto con el Cockpit lo fija el plan técnico)

**Esquema del escenario: Cada evento de Git queda registrado con su momento y su actor**

Dado el repo "demo" observado sin ningún agente presente
Cuando ocurre un evento "<evento>" en el worktree "feat-login"
Entonces el historial de eventos del motor contiene un evento "<evento>" en "feat-login" con su fecha y hora
  Y su actor es "sin atribuir"
  Y el estado de "feat-login" refleja el resultado del evento

Ejemplos:
| evento |
| commit |
| cambio de rama |
| creación de rama |
| borrado de rama |
| creación de worktree |
| borrado de worktree |
| rebase |
| merge |
| push |

**Escenario: Ningún evento se presenta como hecho por un agente sin atribución**

Dado el repo "demo" observado sin ningún agente presente ni registrado
Cuando el desarrollador hace un commit en "feat-login"
Entonces el evento del commit no tiene ningún agente como actor
  Y figura como "sin atribuir"

**Escenario: Diez worktrees activos a la vez se siguen sin perder eventos**

Dado el repo "demo" observado con 10 worktrees
Cuando se hace un commit en cada uno de los 10 worktrees en el mismo minuto
Entonces el historial de eventos contiene los 10 commits, cada uno en su worktree
  Y cada cambio se refleja dentro de la parte del motor del presupuesto de frescura de NFR-04 (< 500 ms de extremo a extremo; el reparto con el Cockpit lo fija el plan técnico)

## Estado de la implementación (2026-10-08)

Implementado en: PR #57, #75.

- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
