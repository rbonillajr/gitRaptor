---
id: US-MCP-004
title: "Un agente sabe por MCP quién más trabaja en su repo, contra qué base y con qué protección"
type: us
status: partially-implemented
priority: high
created: 2026-10-04
updated: 2026-10-08
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
    - ADR-GRP-009
    - ADR-GRD-005
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-003
    - US-GRP-003
    - US-GRP-005
    - US-GRP-011
    - US-GRP-012
    - US-GRD-004
    - US-GRD-014
ado:
  id: null
  url: null
covers: [BR-MCP-CALC-003, BR-MCP-EDGE-005, BR-MCP-EDGE-002, BR-MCP-CALC-002]
blocked_by: []
tags: [mcp, status, no-disponible, rama-base, ola-1]
---

# US-MCP-004: Un agente sabe por MCP quién más trabaja en su repo, contra qué base y con qué protección

## Descripción

**Como** agente de IA que trabaja en un worktree, **quiero** que `status` me diga qué otros worktrees y sesiones hay en el repo, cómo va cada rama contra la base y qué protección está activa, **para** decidir antes de escribir si voy a pisar el trabajo de otro o si una escritura me será denegada.

**Valor**: el agente se coordina con datos del motor en vez de adivinar con `git status` crudo.

## Reglas cubiertas

BR-MCP-CALC-003 (contenido de `status`) · BR-MCP-EDGE-005 (repo o worktree no disponible, evaluado en cada llamada, después de la allowlist) · BR-MCP-EDGE-002 (parte lectura: la base no confirmada o pendiente se declara) · BR-MCP-CALC-002 (parte: paginación de las rutas modificadas) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-003. De otras features: US-GRP-011 (worktree compartido), US-GRP-012 (ahead/behind contra la base), US-GRP-003 (estados especiales y no disponible), US-GRP-005 (huecos de observación), US-GRD-004 (estado de protección y diagnósticos), US-GRD-014 (rama base confirmada).
- **Externas**: ADR-MCP-001 (DEP-MCP-1, no existe): allowlist de campos de `status`; bloqueo de arquitectura. Criterios de worktree no disponible: SEC-11, ADR-GRP-009.
- **Transversal**: los topes (200 rutas por worktree, 32 worktrees, 24 KiB por parte) y el escape del texto no confiable salen de US-MCP-005 ([DS-US-MCP-005](../dev-specs/US-MCP-005-dev-spec.md)); la paginación de las rutas modificadas es de esta historia (ajuste del PO, 2026-10-07). La rama del worktree del llamante (`branch`) ya llegó con US-MCP-005.

## Criterios de Aceptación

**Escenario: Una lista grande de rutas modificadas se recorta y lo dice**

Dado el worktree "shop-feat-a" con 3.000 archivos modificados
Cuando el agente pide `status`
Entonces la respuesta trae como máximo el tope de rutas por página
  Y declara "3.000 en total, truncado" con un cursor para la página siguiente

**Escenario: El agente ve a los demás y su propia situación**

Dado el repo "shop" en la allowlist, con la rama base "main" confirmada y la protección "Completa"
  Y "claude-1" en "shop-feat-a" y "claude-2" activo en "shop-feat-b", 2 commits por delante de "main" desde hace 3 minutos
Cuando "claude-1" pide `status`
Entonces la respuesta indica que actúa en "shop-feat-a" como "claude-1"
  Y lista los worktrees del repo con su rama, sus sesiones con actor y estado, sus rutas modificadas y su ahead/behind con la antigüedad del dato
  Y declara la rama base "main" como confirmada y la protección "Completa" con sus diagnósticos
  Y declara el estado del motor

**Esquema del escenario: La lectura declara el estado de la base sin rechazarse**

Dado el repo "shop" en la allowlist con la rama base "<estado>"
Cuando un agente pide `status`
Entonces recibe el estado del repo
  Y la respuesta declara la rama base como "<estado>"

Ejemplos:
| estado |
| no confirmada |
| pendiente de confirmar |

**Escenario: El agente sabe qué periodo no vio el motor**

Dado el motor de GitRaptor no observó "shop" entre las 10:00 y las 10:20
Cuando un agente pide `status`
Entonces la respuesta declara ese hueco de observación

**Esquema del escenario: Un repo o worktree no disponible no se opera aunque esté habilitado**

Dado el repo "shop" en la allowlist del MCP y "<situación>"
Cuando el agente de "shop-feat-a" pide `status`
Entonces la llamada se rechaza con el motivo "<motivo>" y la acción en texto
  Y la respuesta no contiene rutas, ramas ni otros datos del repo
  Y nada se escribe en la configuración global de Git

Ejemplos:
| situación | motivo |
| el worktree "shop-feat-a" se borró a mano durante la sesión | el worktree shop-feat-a ya no existe |
| el repo pasó a pertenecer a otro usuario del sistema | repo no disponible: pertenece a otro usuario |
| el worktree tiene su raíz en la carpeta personal del usuario | worktree no disponible |

**Escenario: "No habilitado" se comprueba antes que "no disponible"**

Dado el repo "shop" fuera de la allowlist y perteneciente a otro usuario del sistema
Cuando un agente pide `status` en "shop"
Entonces la llamada se rechaza con el motivo "repo no habilitado para el MCP"
  Y la respuesta no contiene ningún dato del repo

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica (respuesta de herramienta; mensajes según la guía de contenido del design system, DSYS-GRP-001).
- **Dev Spec:** [DS-US-MCP-004](../dev-specs/US-MCP-004-dev-spec.md).

Implementado en: PR #216.

**Falta**: la base "pendiente de confirmar" existe en el contrato y se prueba ahí, pero el motor no la emite hasta US-GRD-014 (G1 de la Dev Spec). Linux y Windows: *Pendiente: etapa de validación multiplataforma* (XP-39).
