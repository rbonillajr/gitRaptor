---
id: US-MCP-016
title: "Un agente sabe con quién va a chocar antes de que choque"
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
    - ADR-CKP-001
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-003
    - US-MCP-005
ado:
  id: null
  url: null
covers: [BR-MCP-CALC-004]
blocked_by: [ADR-MCP-001, ADR-CKP-001, DEP-MCP-6]
tags: [mcp, check-conflicts, prediccion, lectura, ola-4]
---

# US-MCP-016: Un agente sabe con quién va a chocar antes de que choque

## Descripción

**Como** agente de IA que trabaja en un worktree, **quiero** pedir `check_conflicts` y recibir la predicción de conflictos que publica el motor para mi worktree, sin contenido de archivos, **para** coordinarme o cambiar de enfoque antes de que un merge o un rebase choque.

**Valor**: el pilar Cockpit (predicción de conflictos) llega también al agente (BR-14, BR-06).

## Reglas cubiertas

BR-MCP-CALC-004 (contenido de `check_conflicts`: lo publicado por el motor; por defecto, los pares del llamante; nunca diff) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-003 (ámbito y allowlist), US-MCP-005 (respuestas acotadas). No depende de ninguna escritura: se puede adelantar en cuanto se levante DEP-MCP-6.
- **Habilitador**: TS-CKP-001, predictor de conflictos que publica la predicción (DEP-CKP-1; propuesta, rama docs/arch-cockpit).
- **Externas**: ADR-CKP-001 (**propuesto**: mecanismo de la predicción). DEP-MCP-6 (predicción publicada en la interfaz del motor). ADR-MCP-001 (DEP-MCP-1, no existe). Todos son bloqueos de arquitectura.

## Criterios de Aceptación

**Escenario: El agente ve el conflicto previsto con otro worktree**

Dado el motor publicó un conflicto previsto entre "shop-feat-a" y "shop-feat-b" en "src/pago.rs", líneas 40 a 58, hace 12 segundos
Cuando el agente de "shop-feat-a" pide `check_conflicts`
Entonces la respuesta contiene ese par con el nivel "conflicto previsto", la ruta, el rango de líneas y la antigüedad
  Y no contiene el contenido de esas líneas ni ningún diff

**Escenario: Por defecto, solo los pares del propio worktree**

Dado el motor publicó solapes entre "shop-feat-a" y "shop-feat-b" y entre "shop-feat-c" y "shop-feat-d"
Cuando el agente de "shop-feat-a" pide `check_conflicts` sin opciones
Entonces la respuesta contiene solo el par con "shop-feat-b"
Cuando lo pide con la opción de todo el repo
Entonces la respuesta contiene los dos pares

**Esquema del escenario: La respuesta declara lo que el motor todavía no sabe**

Dado la predicción de "shop-feat-a" está "<estado>"
Cuando el agente pide `check_conflicts`
Entonces la respuesta declara "<estado>"
  Y no inventa ningún par

Ejemplos:
| estado |
| calculando |
| pendiente |

**Escenario: La respuesta declara los límites de la predicción**

Dado el motor publicó la predicción de "shop-feat-a" con un límite declarado
Cuando el agente pide `check_conflicts`
Entonces la respuesta incluye ese límite tal como lo publicó el motor

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto, tras ADR-MCP-001, ADR-CKP-001 y DEP-MCP-6).
