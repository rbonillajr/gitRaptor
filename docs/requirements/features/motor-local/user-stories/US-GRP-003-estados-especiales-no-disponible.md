---
id: US-GRP-003
title: "El desarrollador sabe qué worktree está en un estado especial o ya no existe"
type: us
status: draft
priority: high
created: 2026-10-03
updated: 2026-10-09
feature: motor-local
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  stories:
    - US-GRP-001
    - US-GRP-009
tags:
  - motor-local
  - worktrees
  - edge-cases
---

# US-GRP-003: El desarrollador sabe qué worktree está en un estado especial o ya no existe

## Descripción

**Como** desarrollador orquestador, **quiero** ver qué worktree tiene un rebase o un merge a medias y cuál dejó de existir, sin perder la vista del resto, **para** decidir dónde intervenir antes de que un agente siga sobre un estado roto.

**Valor**: un worktree borrado o a medias no rompe ni falsea la vista de los demás.

## Reglas cubiertas

BR-EDGE-001 · BR-EDGE-002 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-001; US-GRP-009 (para el escenario de sesiones de un worktree borrado).
- **Externas**: ninguna.
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Esquema del escenario: Un estado especial de Git se reporta tal cual y no se toca**

Dado el repo "demo" observado
  Y el worktree "feat-pagos" queda en el estado "<estado>"
Cuando se consulta el estado del motor
Entonces "feat-pagos" figura en el estado "<estado>"
  Y el motor no completa, continúa ni aborta esa situación: el repo sigue en "<estado>"

Ejemplos:
| estado |
| rebase en curso |
| merge en curso |
| HEAD separado |
| conflictos sin resolver |

**Escenario: Un worktree sin carpeta figura como no disponible y el resto sigue**

Dado el repo "demo" observado con 10 worktrees
Cuando desaparece la carpeta del worktree "feat-login"
Entonces "feat-login" figura como no disponible
  Y un cambio de archivo posterior en cualquiera de los otros 9 worktrees se sigue reflejando en el estado

**Escenario: Las sesiones de un worktree retirado terminan**

Dado el worktree "feat-login" con el agente "Codex" registrado
Cuando se retira el worktree "feat-login"
Entonces la sesión de "Codex" en "feat-login" pasa a "Terminado"

**Escenario: Un repo inaccesible no interrumpe la observación de los demás**

Dado los repos "demo" y "otro" observados
Cuando el repo "otro" se mueve o se borra de su ubicación
Entonces "otro" figura como no disponible
  Y "demo" se sigue observando con normalidad

**Escenario: Un worktree retirado de Git deja de figurar y queda constancia**

Dado el repo "demo" observado con el worktree "feat-login"
Cuando se retira el worktree "feat-login" de Git
Entonces "feat-login" deja de figurar en el estado
  Y el historial de eventos muestra su retiro

> **Enmienda (2026-10-09, PO, desde DS-US-GRP-003)**: "borrar" se separa en dos casos. Si desaparece la carpeta y Git aún registra el worktree, figura como no disponible. Si se retira de Git (`git worktree remove` o `prune`), deja de figurar y queda su evento de retiro; solo entonces terminan sus sesiones.
