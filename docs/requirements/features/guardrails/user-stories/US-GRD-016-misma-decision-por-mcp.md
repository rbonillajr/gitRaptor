---
id: US-GRD-016
title: "Un agente que usa las herramientas MCP recibe la misma decisión que con Git directo"
type: us
status: partially-implemented
priority: high
created: 2026-10-04
updated: 2026-10-29
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
  specs:
    - DS-US-GRD-016
tags:
  - guardrails
  - capa-mcp
  - estado-proteccion
---

# US-GRD-016: Un agente que usa las herramientas MCP recibe la misma decisión que con Git directo

## Descripción

**Como** desarrollador orquestador, **quiero** que las herramientas MCP de GitRaptor rechacen antes de ejecutar lo que Guardrails deniega, con el mismo motivo que la capa de hooks, **para** que un agente no encuentre un camino más laxo según cómo opere.

**Valor**: dos capas, una sola decisión (BRD BR-12).

## Reglas cubiertas

BR-CONS-002 (misma decisión y motivo en las dos capas) · BR-WF-002 (estados "Solo MCP" y "Completa" y sus transiciones por la allowlist) · BR-AUTH-004 (ninguna herramienta MCP edita la configuración ni decide en la cola) · Q-GRD-52 (una operación sin herramienta MCP se cumple con "ninguna herramienta la ejecuta" y la misma decisión con las dos capas) · Q-GRD-53 (el escenario 3 usa un commit en rama protegida) · Q-GRD-54 (los hooks inactivos son un diagnóstico, no un cambio de estado) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-001 (decisión y mínimo seguro), US-GRD-004 (estados de protección por la vía de hooks), US-GRD-007 (permisos del equipo, para el catálogo completo).
- **Externas**: ya no está bloqueada por el Servidor MCP F-001-05: ADR-MCP-001 está aceptado, la allowlist existe (US-MCP-002) y `safe_commit` está en main (PR #289). El esquema del catálogo lo fija ADR-GRP-007. Quedan pendientes US-MCP-018 (`safe_rebase`) y US-MCP-019 (`create_worktree`): cada una trae su fila de extremo a extremo de este esquema antes de liberarse (Q-GRD-52); hasta entonces rebase y crear worktree se verifican por el plan del ejecutor con capa `mcp`.
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

Dado el repo "demo" en estado "Completa", con "release/1.0" protegida por el equipo
Cuando Claude Code intenta un commit en "release/1.0" por la herramienta MCP y después con Git directo
Entonces las dos veces se deniega con la misma regla y el mismo nivel
  Y el registro guarda una entrada por cada intento, con su capa

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

- El plan del ejecutor aplica los permisos del equipo de las operaciones que sabe que ejecuta, con la misma función que los hooks; `safe_commit` ya decide con ella y deja su entrada con capa `mcp`.
- El rechazo `policy-denied` de las herramientas MCP nombra cada regla con su nivel.
- El estado de protección tiene cuatro valores, derivados en el daemon con la allowlist del MCP, y cada entrada o salida de la allowlist queda en el registro.
- Toda herramienta MCP de escritura es una operación del catálogo que decide el ejecutor; ninguna edita la configuración ni decide en la cola.
- Las operaciones sin herramienta MCP se verifican por paridad del motor y por la ausencia de herramienta (Q-GRD-52 y Q-GRD-53, aceptadas).
- Detalle, tareas y pruebas en la [Dev Spec DS-US-GRD-016](../dev-specs/US-GRD-016-misma-decision-por-mcp.md).

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** [DS-US-GRD-016](../dev-specs/US-GRD-016-misma-decision-por-mcp.md) (2026-10-28).

## Estado de la implementación (2026-10-29)

Implementado en: PR #313 (parcial). Dev Spec: [DS-US-GRD-016](../dev-specs/US-GRD-016-misma-decision-por-mcp.md). Verificado en macOS; Linux lo cubre el CI. **Sin certificar**: la corrida de `/nassa-core:implement` se abortó en la línea base por la carga de la máquina (decisión del coordinador).

- Hecho: permisos de las operaciones conocidas en todos los planes del ejecutor (capas `mcp` y `cockpit`); `policy-denied` con `reasons: [{rule, level}]`; los cuatro estados de protección servidos con `guard.protection-layers`; filtro `layer` en `guard.log`; ninguna herramienta MCP relaja ni decide; parte A de las políticas de commit por MCP (`policy.commit-format` exacto, `policy.diff-size` falla cerrado).
- Pendiente: `policy.diff-size` exacto por MCP y la comprobación del commit real frente al previsto (parte B); la prueba de daemon de T007; Linux y Windows reales (**Pendiente: etapa de validación multiplataforma**, XP-64).
