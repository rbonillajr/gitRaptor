---
id: US-GRD-016
title: "Un agente que usa las herramientas MCP recibe la misma decisión que con Git directo"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRD-001
    - US-GRD-004
    - US-GRD-007
tags:
  - guardrails
  - capa-mcp
  - estado-proteccion
  - bloqueada
---

# US-GRD-016: Un agente que usa las herramientas MCP recibe la misma decisión que con Git directo

## Descripción

**Como** desarrollador orquestador, **quiero** que las herramientas MCP de GitRaptor rechacen antes de ejecutar lo que Guardrails deniega, con el mismo motivo que la capa de hooks, **para** que un agente no encuentre un camino más laxo según cómo opere.

**Valor**: dos capas, una sola decisión (BRD BR-12).

## Reglas cubiertas

BR-CONS-002 (misma decisión y motivo en las dos capas) · BR-WF-002 (estados "Solo MCP" y "Completa" y sus transiciones por la allowlist) · BR-AUTH-004 (ninguna herramienta MCP edita la configuración ni decide en la cola) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-001 (decisión y mínimo seguro), US-GRD-004 (estados de protección por la vía de hooks), US-GRD-007 (permisos del equipo, para el catálogo completo).
- **Externas**: **bloqueada** por el Servidor MCP F-001-05 (herramientas y allowlist, NFR-02) y, por el esquema del catálogo, por el ADR de formato P8 (motor-local).
- **Transversal**: Windows, macOS y Linux.

## Criterios de Aceptación

**Escenario: El force-push por MCP se rechaza antes de ejecutar**

Dado el repo "demo" en la allowlist del MCP y sin configuración de Guardrails
Cuando Claude Code pide un force-push de "feat-x" a través de una herramienta MCP
Entonces la herramienta lo rechaza sin ejecutarlo
  Y el motivo es el mismo que daría la capa de hooks para esa operación

**Esquema del escenario: Cada operación del catálogo denegada por el equipo se rechaza por la capa MCP**

Dado el repo "demo" en la allowlist del MCP, cuya configuración del equipo fija "<operación>" en "denegar"
Cuando Claude Code pide "<operación>" a través de una herramienta MCP
Entonces la herramienta la rechaza sin ejecutarla
  Y el motivo nombra el permiso y el nivel "equipo"

Ejemplos:
| operación |
| commit |
| push |
| force-push |
| reset --hard |
| borrar rama |
| rebase |
| merge |
| crear worktree |
| borrar worktree |

**Escenario: El mismo intento por las dos capas recibe la misma decisión**

Dado el repo "demo" en estado "Completa"
Cuando se intenta borrar la rama base por la herramienta MCP y después con Git directo
Entonces las dos veces se deniega con el mismo motivo y queda constancia de la capa en el registro

**Esquema del escenario: El estado de protección sigue a la allowlist**

Dado el repo "demo" en estado "<antes>"
Cuando "<cambio>"
Entonces el estado de protección de "demo" es "<después>"

Ejemplos:
| antes | cambio | después |
| Sin protección | "demo" entra en la allowlist del MCP | Solo MCP |
| Solo hooks | "demo" entra en la allowlist del MCP | Completa |
| Completa | "demo" sale de la allowlist del MCP | Solo hooks |
| Solo MCP | "demo" sale de la allowlist del MCP | Sin protección |

**Escenario: Ninguna herramienta MCP relaja ni decide**

Dado el repo "demo" en la allowlist del MCP
Cuando un agente busca entre las herramientas MCP una que edite la configuración o decida en la cola
Entonces no existe ninguna

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto, coordinado con F-001-05).
