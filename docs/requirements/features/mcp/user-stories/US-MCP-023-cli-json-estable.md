---
id: US-MCP-023
title: "Un agente que trabaja por Bash lee la salida de raptor con un contrato JSON estable y versionado"
type: us
status: draft
priority: high
milestone: M3
created: 2026-11-10
updated: 2026-11-12
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
  context:
    - CTX-MCP-001
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-004
    - US-MCP-005
    - US-MCP-020
ado:
  id: null
  url: null
covers: [BR-MCP-CONS-008]
blocked_by: []
tags: [mcp, agentes, cli, json, contrato, superficie-agentes, hito-m3]
---

# US-MCP-023: Un agente que trabaja por Bash lee la salida de raptor con un contrato JSON estable y versionado

## Descripción

**Como** desarrollador orquestador, **quiero** que todos los subcomandos de `raptor` que usa un agente den con `--json` una salida que declara la versión de su contrato, que los errores también salgan en JSON con el mismo código, motivo y acción que el MCP, y que `agent withdraw`, `daemon status` y `guard exec` tengan `--json`, **para** que un agente que trabaja por Bash entienda el estado y los rechazos sin adivinar formatos y no se rompa cuando GitRaptor cambie de versión.

**Valor**: la guía para agentes (US-MCP-020) enseña el camino por CLI; sin un contrato estable ese camino falla en silencio. Hoy ninguna salida `--json` declara versión, un error con `--json` sale como texto y tres subcomandos de uso por agentes no tienen `--json` ([inventario](../research/inventario-json-cli-2026-11.md)).

**Prioridad e hito**: Must, M3 (beta). Decisión de Rene Bonilla del 2026-11-10 (P-MCP-32).

## Reglas cubiertas

BR-MCP-CONS-008 (versión del contrato en toda salida, errores en JSON con código estable, cambio incompatible = versión mayor, vista del perfil `mcp` para un agente) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-MCP-005 (códigos de rechazo estables, plantilla fija, motivo y acción) y US-MCP-004 (contenido de la vista del agente).
- **Relación**: US-MCP-020 enseña a usar este contrato; US-MCP-024 y US-MCP-025 lo usan.
- **Transversal (lo define el Arquitecto)**: dónde se documenta el esquema y cómo se detecta un cambio incompatible antes de publicar.

## Criterios de Aceptación

**Esquema del escenario: Toda salida JSON declara la versión del contrato**

Dado el repo "shop" observado y habilitado para el MCP
Cuando un agente ejecuta "<subcomando>" con `--json`
Entonces la salida es JSON y declara la versión del contrato

Ejemplos:
| subcomando |
| raptor status |
| raptor sessions |
| raptor timeline |
| raptor guard log |
| raptor undo --agent claude-1 |

**Esquema del escenario: Los tres subcomandos que faltaban tienen salida JSON**

Dado el repo "shop" observado y habilitado para el MCP
Cuando un agente ejecuta "<subcomando>" con `--json`
Entonces la salida es JSON con la versión del contrato y "<contenido>"

Ejemplos:
| subcomando | contenido |
| raptor agent withdraw | el registro retirado |
| raptor daemon status | si el motor está vivo |
| raptor guard exec -- git commit | el rechazo de comando reservado, en JSON, con el mismo código y acción (sigue reservado; un agente no lo ejecuta) |

**Escenario: Un error con --json sale en JSON**

Dado el motor de GitRaptor caído y sin poder arrancar
Cuando un agente ejecuta `raptor status --json` en "shop"
Entonces la salida es JSON con la versión del contrato, el código "engine-unavailable", el motivo y la acción
  Y el código de salida es distinto de 0
  Y no hay texto suelto fuera del JSON

**Escenario: Por la CLI un agente no ve más que por el MCP**

Dado el repo "otro" observado pero no habilitado para el MCP
Cuando un agente ejecuta `raptor status --json` en "otro"
Entonces la salida es el rechazo "repo-not-enabled" con su acción, en JSON
  Y no contiene ningún dato de "otro"

**Escenario: Un cambio incompatible sube la versión mayor**

Dado la versión 1 del contrato con el campo de rama en la salida de `raptor status --json`
Cuando una versión nueva de GitRaptor quita o renombra ese campo
Entonces su salida declara la versión mayor 2
  Y una versión que solo añade un campo nuevo sigue declarando la versión mayor 1

**Escenario: El contrato está documentado para agentes**

Dado una versión publicada de GitRaptor
Cuando el desarrollador busca el contrato JSON de la CLI
Entonces encuentra, para cada subcomando de uso por agentes, sus campos, su versión y los códigos de error que puede devolver

## Requisitos Técnicos

Arquitecto, 2026-11-10: [ADR-MCP-001, Enmienda (2026-11-10, superficie para agentes)](../../../../architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md#enmienda-2026-11-10-superficie-para-agentes), puntos C1, C3 y D.

- **Sobre**: el mismo en todas las salidas `--json`:
  - éxito: `{"schema_version": "MAJOR.MINOR", "data": …}`;
  - error: `{"schema_version": …, "error": {code, message, params, action}}`, por stdout, con stderr vacío y salida ≠ 0.
- **Versión**: una sola versión para todo el contrato de la CLI, declarada como constante en `crates/api` junto a los códigos de § 5. Empieza en `1.0`.
- **Esquemas**: se generan de los tipos de `crates/api`, con instantánea y prueba de compatibilidad en CI (cambio incompatible sin subir el mayor, o campo nuevo sin subir el menor, falla). La referencia para agentes se genera de esos esquemas.
- **Códigos**: el catálogo estable de § 5. Los códigos solo de la CLI entran en el mismo catálogo.
- **Condición de entrada (C1)**: el escenario "por la CLI un agente no ve más que por el MCP" exige SEC-MCP-01 (perfil `mcp` por solicitante) en `main`. La implementa [TS-GRP-010](../../motor-local/technical-stories/TS-GRP-010-perfil-mcp-por-solicitante.md), que bloquea el escenario y se mezcla antes que esta historia (decisión del orquestador, 2026-11-10: S-01 pasa a M3).
- **`guard exec` (C3)**: sigue reservado. Un agente con `--json` recibe el rechazo de comando reservado en JSON.
- **Para la Dev Spec**: JSON compacto o con formato; errores de `clap` en JSON; códigos de salida por familia.

## Diseño y Dev Spec

- **Diseño:** no aplica (salida para máquinas; la regla de `--json` ya está en el design system, tabla de la CLI).
- **Dev Spec:** **sí** (decisión del Arquitecto, 2026-11-10). Pendiente: `../dev-specs/US-MCP-023-dev-spec.md`.
