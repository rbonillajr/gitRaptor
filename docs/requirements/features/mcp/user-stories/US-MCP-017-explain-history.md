---
id: US-MCP-017
title: "Un agente entiende qué pasó en su repo sin ver mensajes ni contenido"
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
    - ADR-GRP-013
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-003
    - US-MCP-005
    - US-TMC-006
    - US-TMC-007
    - TS-TMC-004
ado:
  id: null
  url: null
covers: [BR-MCP-CALC-005, BR-MCP-VAL-002]
blocked_by: []
tags: [mcp, explain-history, timeline, lectura, ola-4]
---

# US-MCP-017: Un agente entiende qué pasó en su repo sin ver mensajes ni contenido

## Descripción

**Como** agente de IA que trabaja en un worktree, **quiero** pedir `explain_history` y recibir los eventos recientes del repo con su actor, operación y cobertura, **para** entender qué cambió otro agente o qué deshizo alguien antes de seguir trabajando.

**Valor**: la Time Machine explicada al agente (BR-14) sin exponer mensajes de commit ni contenido (Q-CKP-8).

## Reglas cubiertas

BR-MCP-CALC-005 (contenido de `explain_history`: últimos 50 por defecto, filtro, paginación con tope) · BR-MCP-VAL-002 (parte: el filtro por rama no acepta expresiones de revisión ni nombres no válidos) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-003, US-MCP-005. No depende de ninguna escritura. De la Time Machine: US-TMC-006 (timeline qué, cuándo, quién), US-TMC-007 (filtros y huecos) y TS-TMC-004, que ya expone la consulta del timeline para CLI, TUI y MCP en el contrato del canal (D-12).
- **Externas**: ADR-MCP-001 (DEP-MCP-1, no existe): único bloqueo de arquitectura. La parte de timeline de DEP-MCP-6 pasa a dependencia de historia (TS-TMC-004); los eventos del motor los fija ADR-GRP-013, ya aceptado, y viajan por el canal de TS-GRP-004. Cifra de 50 eventos: supuesto S-MCP-2.

## Criterios de Aceptación

**Escenario: El agente recibe los últimos eventos sin mensajes**

Dado el repo "shop" con 80 eventos en su timeline
  Y hace 5 minutos "claude-2" hizo el commit "a1b2c3d" en "feat-b" con 3 rutas
Cuando el agente pide `explain_history` sin opciones
Entonces recibe los 50 eventos más recientes, el más reciente primero, con un cursor para los siguientes
  Y el primero indica operación commit, actor "claude-2", rama "feat-b", oid "a1b2c3d", 3 rutas, cuándo y cobertura completa
  Y ningún evento contiene el mensaje de commit ni contenido de archivos

**Escenario: El agente filtra por worktree o por rama**

Dado eventos de "shop-feat-a" y "shop-feat-b" en el timeline de "shop"
Cuando el agente pide `explain_history` filtrado por la rama "feat-b"
Entonces todos los eventos recibidos son de "feat-b"

**Escenario: Una página más grande que el tope se recorta**

Dado el repo "shop" con más eventos que el tope de página
Cuando el agente pide `explain_history` con 1.000 eventos por página
Entonces recibe como máximo el tope de eventos y un cursor

**Esquema del escenario: Un filtro de rama no válido se rechaza**

Dado el repo "shop" en la allowlist
Cuando el agente pide `explain_history` filtrado por la rama "<rama>"
Entonces la llamada se rechaza con el motivo "nombre de rama no válido"

Ejemplos:
| rama |
| --upload-pack=evil |
| HEAD~3 |
| @{-1} |
| feat..pagos |

**Escenario: Lo que el motor no observó se declara**

Dado el motor no observó "shop" entre las 10:00 y las 10:20
Cuando el agente pide `explain_history` de ese periodo
Entonces la respuesta declara el hueco de observación y la cobertura de los eventos afectados

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto, tras ADR-MCP-001 y DEP-MCP-6).
