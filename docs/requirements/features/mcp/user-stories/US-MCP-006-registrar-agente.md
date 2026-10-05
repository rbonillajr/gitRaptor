---
id: US-MCP-006
title: "Un agente sin soporte completo se declara por MCP y su trabajo queda a su nombre"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
    - ADR-GRP-005
    - ADR-GRP-013
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-003
    - US-GRP-007
    - US-GRP-009
ado:
  id: null
  url: null
covers: [BR-MCP-WF-005, BR-MCP-VAL-004, BR-MCP-AUTH-002]
blocked_by: []
tags: [mcp, register-agent, atribucion, sin-atribuir, ola-1]
---

# US-MCP-006: Un agente sin soporte completo se declara por MCP y su trabajo queda a su nombre

## Descripción

**Como** desarrollador orquestador, **quiero** que un agente que GitRaptor no detecta solo (Codex, Cursor u otro) se registre por MCP en su worktree, y que sin registro solo pueda leer, **para** que toda escritura de un agente quede atribuida a alguien y nunca a "sin atribuir".

**Valor**: cualquier agente queda observado y atribuido (BR-02); "sin atribuir" no escribe (Q-MCP-4).

## Reglas cubiertas

BR-MCP-WF-005 (registro y retiro; confirma la sesión ya detectada) · BR-MCP-VAL-004 (parte: nombre de agente validado, ni reservado ni de otro agente presente) · BR-MCP-AUTH-002 ("sin atribuir" solo lee y se registra) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-003. De motor-local: US-GRP-009 (capacidad de registro explícito en el motor; espera esta historia para el auto-registro del agente) y US-GRP-007 (modelo de sesión).
- **Externas**: ADR-MCP-001 (DEP-MCP-1, no existe); bloqueo de arquitectura. Validación de nombres y nombres reservados: ADR-GRP-005 § 6.6.
- **Relación**: las escrituras que exigen solicitante atribuido (US-MCP-008 en adelante) vuelven a comprobar BR-MCP-AUTH-002 con su propia herramienta.

## Criterios de Aceptación

**Escenario: Un agente conectado a mano se registra en su worktree**

Dado Codex conectado a mano al servidor MCP desde "shop-feat-c" y resuelto como "sin atribuir"
Cuando Codex pide `register_agent` con el nombre "codex"
Entonces "shop-feat-c" tiene una sesión "codex" con origen "registrado"
  Y esa sesión aparece en `status` y en el timeline del repo

**Escenario: Registrarse donde ya se le detectaba confirma la misma sesión**

Dado Claude Code detectado como "claude-1" en "shop-feat-a"
Cuando el agente pide `register_agent` con el nombre "claude-1"
Entonces "shop-feat-a" sigue teniendo una sola sesión de "claude-1"
  Y su origen visible es "registrado"

**Esquema del escenario: Un nombre no permitido no registra nada**

Dado un agente "sin atribuir" en "shop-feat-a", donde "claude-2" tiene una sesión presente
Cuando pide `register_agent` con el nombre "<nombre>"
Entonces el registro se rechaza con el motivo "<motivo>"
  Y las sesiones de "shop-feat-a" no cambian

Ejemplos:
| nombre | motivo |
| human | nombre no permitido |
| claude-2 | nombre no permitido |
| un nombre que supera el tope de longitud | texto no válido |
| un nombre con caracteres de control | texto no válido |

**Escenario: Un agente retira su propio registro**

Dado "codex" registrado en "shop-feat-c"
Cuando "codex" pide `unregister_agent`
Entonces "shop-feat-c" ya no tiene la sesión registrada de "codex"

**Escenario: Un agente no retira el registro de otro**

Dado "codex" registrado en "shop-feat-a" y "claude-1" detectado en el mismo worktree sin registro propio
Cuando "claude-1" pide `unregister_agent`
Entonces la petición se rechaza con el motivo "solo puedes retirar tu registro"
  Y "codex" sigue registrado

**Escenario: Sin atribuir no hace nada más que leer y registrarse**

Dado un cliente MCP resuelto como "sin atribuir" en "shop-feat-a"
Cuando pide `unregister_agent`
Entonces la petición se rechaza con el motivo "no se pudo identificar al agente" y la acción "usa register_agent"
  Y `status` le sigue respondiendo

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica (respuesta de herramienta; mensajes según la guía de contenido del design system, DSYS-GRP-001).
- **Dev Spec:** pendiente (Arquitecto, tras ADR-MCP-001).
