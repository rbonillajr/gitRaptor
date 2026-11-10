---
id: RES-MCP-JSON-2026-11
title: "Inventario de la salida --json de la CLI raptor para agentes"
type: research
status: proposed
version: 0.1
date: 2026-11-10
author: PO (AADD) para Rene Bonilla
domain: GRP
feature: mcp
related:
  stories: [US-MCP-023, US-MCP-024]
  adrs: [ADR-MCP-001]
tags: [mcp, cli, json, agentes, inventario]
---

# Inventario de la salida `--json` de la CLI `raptor` para agentes

Leído del código de `apps/cli/src/commands/` en la rama `docs/agent-surface-stories` (base `905c66b5`, 2026-11-10), solo lectura. Es el punto de partida de [US-MCP-023](../user-stories/US-MCP-023-cli-json-estable.md); el Arquitecto lo confirma al escribir sus Requisitos Técnicos.

La regla ya existe en el design system: "`--json` en todos los comandos, con un esquema estable y versionado. Es la base para scripts y agentes" ([README, tabla de la CLI](../../../../design-system/README.md)). Hoy se cumple solo en parte.

## Subcomandos y `--json`

"Uso por un agente": lectura o escritura que un agente de la flota necesita por Bash según el enfoque del 2026-11-10. "Comando del humano": configuración o comando reservado que el agente no debe necesitar.

| Subcomando | `--json` hoy | Uso por un agente | Nota |
|---|---|---|---|
| `raptor status` (con `--resources` y `--verbose`) | Sí | Lectura: estado del repo, sesiones, protección | No incluye la predicción de conflictos. Con el motor caído, el error sale como texto en stderr aunque se pida `--json` (observado en `status`) |
| `raptor sessions` (con `--all`) | Sí | Lectura: sesiones de agentes | — |
| `raptor events` (con `--limit`, `--all`) | Sí | Lectura: eventos recientes | — |
| `raptor timeline` (con `--worktree`, `--since`, `--agent`, `--limit`) | Sí | Lectura: qué pasó en el repo | — |
| `raptor undo` (con `--agent`, `--since`) | Sí | Escritura: deshacer lo propio | Es el "para deshacer lo tuyo" de la skill |
| `raptor redo` | Sí | Escritura: rehacer lo propio | — |
| `raptor restore <id>` | Sí | Por confirmar | — |
| `raptor agent register`, `correct`, `uncorrect` | Sí | Registro del propio agente | — |
| `raptor agent withdraw` | **No** | Retiro del propio registro | Hueco: el resto de `agent` sí tiene `--json` |
| `raptor guard status`, `guard log` (con `--layer`) | Sí | Lectura: protección y bloqueos | — |
| `raptor guard exec -- <git…>` | **No** | Posible: ejecutar Git con la decisión de Guardrails | Por confirmar si un agente debe usarlo |
| `raptor guard install`, `uninstall`, `cancel`, `confirm` | No | Comando del humano | No necesita contrato para agentes |
| `raptor daemon ensure` (oculto) | Sí (obligatorio) | Interno | — |
| `raptor daemon status` | **No** | Posible: saber si el motor está vivo | Hueco menor |
| `raptor daemon stop`, `enable`, `disable` | No | Comando del humano | — |
| `raptor repo add`, `retire`, `roots`, `discovered`, `dismiss` | No | Comando del humano | — |
| `raptor mcp install`, `uninstall`, `list` | No | Comando del humano | `list` podría servir a `configure-agents` (US-MCP-021) |
| `raptor mcp enable`, `disable` | No | Comando reservado (ADR-MCP-001 § 3) | Nunca para agentes |
| `raptor tui`, `ui gallery`, `predict-worker` (oculto) | No aplica | — | — |

## Lo que falta (transversal a todos los subcomandos)

| Hueco | Evidencia | Historia |
|---|---|---|
| Ninguna salida `--json` declara una versión de esquema | Búsqueda de `schema_version`, `schemaVersion` y `"version"` sin resultados en `apps/cli/src` | US-MCP-023 |
| Los errores no salen como JSON cuando se pide `--json` | `raptor status --json` con el motor caído escribe texto en stderr (`commands/status.rs`); en el resto, sin verificar | US-MCP-023 |
| No hay documentación del esquema para agentes | Ningún documento de `docs/` describe la forma de la salida `--json`; solo la regla del design system | US-MCP-023 |
| No hay subcomando que diga con quién va a chocar el agente | La predicción solo sale por `check_conflicts` del MCP (US-MCP-016) y por la TUI; `status --json` no la incluye | US-MCP-024 |
| `agent withdraw` y `daemon status` sin `--json` | Tabla anterior | US-MCP-023 |

## Lo que se queda solo en el MCP

Las escrituras protegidas del catálogo (`safe_commit`, `safe_rebase`, `create_worktree`, `snapshot`) no tienen subcomando de la CLI y este inventario no propone crearlo: según el criterio del enfoque aprobado, una acción protegida vive en el MCP. Un agente que trabaja por Bash usa Git, y los hooks de Guardrails aplican la misma decisión (US-GRD-016).
