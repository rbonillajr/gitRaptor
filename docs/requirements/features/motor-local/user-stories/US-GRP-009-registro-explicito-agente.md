---
id: US-GRP-009
title: "El desarrollador o el agente declaran qué agente trabaja en un worktree"
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
    - US-GRP-002
    - US-GRP-007
tags:
  - motor-local
  - registro-agentes
  - otro-agente
---

# US-GRP-009: El desarrollador o el agente declaran qué agente trabaja en un worktree

## Descripción

**Como** desarrollador orquestador (o el propio agente), **quiero** registrar qué agente trabaja en un worktree, sea Claude Code o cualquier otro, **para** que su trabajo se le atribuya y no se confunda con el mío.

**Valor**: ningún agente queda fuera; Codex y Cursor quedan cubiertos como "otro agente" hasta su soporte completo (Q32).

## Reglas cubiertas

BR-VAL-001 · BR-VAL-002 · BR-CONS-003 · BR-WF-001 (presente hasta retirar el registro, Q41) · BR-CONS-004 (registrar a un agente ya detectado confirma su sesión, Q39) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-002; US-GRP-007 (en serie: reutiliza el modelo de sesión que fija 007; el contrato compartido lo fija la Dev Spec). Va antes que US-GRP-008: su Dev Spec fija el contrato de "quién hizo un evento" (agente con su origen o "sin atribuir") que 008 reutiliza.
- **Resuelta**: P16 (si la sesión confirmada pasa a origen "registrado"). **Decisión de Rene (2026-10-07)**: sí. El escenario de confirmación solo exige lo decidido.
- **Externas**: el registro hecho por el propio agente llega por el Servidor MCP (F-001-05) y la CLI expone el del desarrollador; esta historia define la capacidad del motor.
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Escenario: Registrar Claude Code crea una sesión con origen registrado**

Dado el repo "demo" observado con el worktree "feat-login" sin ninguna sesión
Cuando el desarrollador registra "Claude Code" en "feat-login"
Entonces el estado muestra una sesión de "Claude Code" en "feat-login" en estado "Activo" con origen "registrado"

**Escenario: Registrar un agente ya detectado confirma su sesión**

Dado una única sesión en "feat-login", de "Claude Code" con origen "detectado"
Cuando "Claude Code" se registra a sí mismo en "feat-login"
Entonces "feat-login" sigue con una sola sesión, la misma de "Claude Code"
  Y "feat-login" no figura como compartido

**Esquema del escenario: Un agente sin soporte completo se acepta como otro agente**

Dado el repo "demo" observado con el worktree "feat-login"
Cuando "<quien>" registra el agente "<agente>" en "feat-login"
Entonces el estado muestra una sesión de "otro agente: <agente>" en "feat-login" con origen "registrado"
  Y un commit posterior en "feat-login" tiene como actor "otro agente: <agente>"
  Y el motor informa que el agente se observa y se le atribuye su actividad, sin funciones específicas

Ejemplos:
| quien | agente |
| el desarrollador | Codex |
| el propio agente | Codex |
| el desarrollador | Cursor |
| el desarrollador | Copilot |

**Esquema del escenario: El registro en un destino inválido se rechaza**

Dado el repo "demo" observado y el repo "otro" no observado
Cuando el desarrollador registra "Claude Code" en "<destino>"
Entonces el motor rechaza el registro indicando "<motivo>"
  Y no se crea ninguna sesión

Ejemplos:
| destino | motivo |
| un directorio que no es un worktree | ese directorio no es un worktree de ningún repo observado |
| un worktree del repo "otro" | el repo no está observado |

**Escenario: Retirar el registro termina la sesión**

Dado "otro agente: Codex" registrado en "feat-login"
Cuando pasa más del umbral de inactividad sin actividad en "feat-login"
Entonces su sesión figura como "Inactivo", no como "Terminado"
Cuando el desarrollador retira el registro de "Codex" en "feat-login"
Entonces la sesión pasa a "Terminado"

## Estado de la implementación (2026-10-08)

Implementado en: PR #106.

Notas (fuera del alcance de esta ficha o sin bloquearla):
- Las herramientas MCP `register_agent`/`unregister_agent` son de US-MCP-006.
- Perfil `mcp` por solicitante (S-01): sin implementar.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
