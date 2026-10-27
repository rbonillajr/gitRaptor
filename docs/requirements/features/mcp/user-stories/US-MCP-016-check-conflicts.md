---
id: US-MCP-016
title: "Un agente sabe con quién va a chocar antes de que choque"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-27
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
  specs: [DS-US-MCP-016]
ado:
  id: null
  url: null
covers: [BR-MCP-CALC-004]
blocked_by: []
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
- **Habilitador**: TS-CKP-001, predictor de conflictos que publica la predicción (DEP-CKP-1). El PR-A está en `main` (PR #275); la publicación (T009, con la forma fijada en DS-TS-CKP-001 § Contrato de API) llega con el PR-B.
- **Externas** (actualizado el 2026-10-27): ADR-CKP-001 y ADR-MCP-001 están `accepted`, y DEP-MCP-6 está aplicada (ADR-MCP-001 § 11; Enmienda 2026-10-05, MCP, de ADR-CKP-001). Solo queda el bloqueo técnico de T009 de TS-CKP-001.

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

Resumen de [DS-US-MCP-016](../dev-specs/US-MCP-016-dev-spec.md):

- Método `mcp.conflicts` del perfil `mcp` (patrón de `mcp.status`), acotado al repo y al worktree del llamante; lee el estado publicado por el predictor (T009 de TS-CKP-001) y no calcula nada. La forma de la predicción es la de [DS-TS-CKP-001 § Contrato de API](../../cockpit/dev-specs/TS-CKP-001-predictor-conflictos.md#contrato-de-api).
- Herramienta `check_conflicts {scope?, cursor?}` en `apps/mcp/src/conflicts.rs`: por defecto los pares del worktree del llamante; `repo`, todos. Sin worktree identificable: todo el repo con `reason: caller-worktree-unknown` (P6).
- Respuesta con ⚡ primero (archivos y rangos `[start, len]` por lado), luego ⚠; `age_s` calculada por el daemon; `calculating`/`pending` con su `reason`, sin `level` ni pares inventados; `limits` como códigos.
- Páginas de hasta 50 pares con objetivo de 800 tokens y tope de 23 KiB (`fit_conflicts`); cursor opaco por conexión en `McpCursors`, sin MAC (DS-US-MCP-004 D4). Rate limit de lecturas existente.
- Nunca contenido ni diff; rutas y nombres como texto no confiable con el saneado de `for_mcp` (NFR-02, SEC-12, MCP03/MCP06/MCP10).
- Bloqueada por T009 del PR-B de TS-CKP-001 (que espera a G5).

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** [DS-US-MCP-016](../dev-specs/US-MCP-016-dev-spec.md).
