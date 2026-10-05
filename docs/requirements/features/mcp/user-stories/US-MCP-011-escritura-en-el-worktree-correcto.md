---
id: US-MCP-011
title: "La escritura de un agente cae en el worktree que espera, o no ocurre"
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
    - ADR-CKP-002
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-009
    - US-TMC-015
ado:
  id: null
  url: null
covers: [BR-MCP-VAL-005, BR-MCP-EDGE-006, BR-MCP-AUTH-003, BR-MCP-EDGE-008, BR-MCP-EDGE-002]
blocked_by: [ADR-MCP-001, ADR-CKP-002]
tags: [mcp, safe-commit, expect-worktree, subagente, precondiciones, ola-3]
---

# US-MCP-011: La escritura de un agente cae en el worktree que espera, o no ocurre

## Descripción

**Como** desarrollador orquestador, **quiero** que un agente pueda declarar en qué worktree espera escribir y que una escritura se rechace si el worktree no coincide o está en un estado confuso, **para** que un subagente nunca escriba en el worktree de su padre ni sobre un rebase a medias.

**Valor**: cierra el riesgo R-MCP-2 (subagente en otro worktree); 0 escrituras fuera del worktree del llamante (KPI de Q-MCP-18).

## Reglas cubiertas

BR-MCP-VAL-005 (parte: `expect_worktree` solo estrecha) · BR-MCP-EDGE-006 (subagente en otro worktree) · BR-MCP-AUTH-003 (parte escritura: solo en el worktree propio) · BR-MCP-EDGE-008 (operación en curso primero, luego HEAD separado) · BR-MCP-EDGE-002 (parte commit: con la base pendiente, `safe_commit` sigue) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-009. De la Time Machine: US-TMC-015 (operación de Git en curso).
- **Externas**: ADR-MCP-001 (DEP-MCP-1, no existe): `expect_worktree`. ADR-CKP-002 (**propuesto**; DEP-MCP-2 vía ADR-CKP-002): las precondiciones de operación en curso y HEAD separado son del ejecutor y se revalidan antes de ejecutar (Q-MCP-30). Bloqueos de arquitectura.
- **Relación**: `safe_rebase` (US-MCP-018) y `create_worktree` (US-MCP-019) reutilizan `expect_worktree` y estas precondiciones. US-MCP-010 va después de esta (las dos tocan los parámetros de `safe_commit`).

## Criterios de Aceptación

**Escenario: El agente declara el worktree esperado y coincide**

Dado una sesión MCP en "shop-feat-a" con "claude-1" atribuido y "src/a.rs" modificado
Cuando pide `safe_commit` de "src/a.rs" esperando el worktree "shop-feat-a"
Entonces el commit se crea en "shop-feat-a"

**Escenario: Un subagente que espera otro worktree no escribe en el del padre**

Dado un subagente aislado en "shop-feat-b" que usa el servidor MCP de su padre, arrancado en "shop-feat-a"
Cuando el subagente pide `safe_commit` esperando el worktree "shop-feat-b"
Entonces la petición se rechaza con el motivo "el worktree no coincide: esta sesión opera en shop-feat-a"
  Y no cambia nada en "shop-feat-a" ni en "shop-feat-b"

**Escenario: Un cambio de directorio no lleva la escritura a otro worktree**

Dado una sesión MCP que arrancó en "shop-feat-a", con "src/a.rs" modificado en "shop-feat-a"
Cuando el agente cambia de directorio en su shell a "shop-feat-b" y pide `safe_commit` de "src/a.rs" sin indicar worktree
Entonces el commit se crea en "shop-feat-a"
  Y "shop-feat-b" no cambia
  Y la respuesta nombra "shop-feat-a" como el worktree en el que actuó

**Esquema del escenario: Un worktree en un estado confuso no admite commits**

Dado "shop-feat-a" con "<estado>"
Cuando "claude-1" pide `safe_commit` de "src/a.rs"
Entonces la petición se rechaza con el motivo "<motivo>" y la acción "<acción>"
  Y no se crea ningún commit

Ejemplos:
| estado | motivo | acción |
| un rebase de Git a medias | hay un rebase en curso en este worktree | resuélvelo o pide al desarrollador que lo resuelva |
| un rebase de Git a medias, que también deja HEAD separado | hay un rebase en curso en este worktree | resuélvelo o pide al desarrollador que lo resuelva |
| HEAD separado | HEAD separado | crea una rama o cámbiate a una |

**Escenario: Con la rama base pendiente, el commit sigue**

Dado el repo "shop" con la rama base pendiente de confirmar
Cuando "claude-1" pide `safe_commit` de "src/a.rs" con un mensaje válido
Entonces el commit se crea en "shop-feat-a"
  Y Guardrails decide con su conjunto mínimo

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto, tras ADR-MCP-001 y ADR-CKP-002).
