---
id: US-MCP-020
title: "Un agente de Claude Code sabe cómo trabajar con GitRaptor sin que el desarrollador se lo explique en cada sesión"
type: us
status: ready
priority: high
milestone: M3
created: 2026-11-10
updated: 2026-11-21
domain: GRP
epic: E-001
feature: mcp
related:
  adrs:
    - ADR-MCP-001
  context:
    - CTX-MCP-001
  specs:
    - DS-US-MCP-020
  rules:
    - BR-MCP-001
  stories:
    - US-MCP-005
    - US-MCP-012
    - US-MCP-016
    - US-TMC-011
    - US-MCP-021
    - US-MCP-023
    - US-MCP-024
    - US-WTL-013
ado:
  id: null
  url: null
covers: [BR-MCP-CONS-009]
blocked_by: [US-MCP-023]
tags: [mcp, agentes, skill, guia, superficie-agentes, hito-m3]
---

# US-MCP-020: Un agente de Claude Code sabe cómo trabajar con GitRaptor sin que el desarrollador se lo explique en cada sesión

## Descripción

**Como** desarrollador orquestador, **quiero** que mis agentes de Claude Code tengan una guía de GitRaptor (skill y bloque de instrucciones) que les diga cuándo preguntar si van a chocar, qué hacer cuando los bloquean, cómo deshacer lo suyo y qué significa cada rechazo, **para** que respeten los bloqueos en lugar de rodearlos y yo no tenga que repetir esas instrucciones en cada prompt.

**Valor**: hoy el agente recibe un rechazo con motivo y acción (US-MCP-005), pero nada le dice de antemano que no debe buscar otra vía. La guía se carga solo cuando el agente la necesita, así que no ocupa contexto en cada sesión.

**Prioridad e hito**: Must, M3 (beta). Decisión de Rene Bonilla del 2026-11-10 (P-MCP-32). Solo Claude Code (D2); Codex y Cursor, después de v0.1.0.

**Decisiones que la acotan** (2026-11-10): la guía viaja con el repo y el equipo la versiona (P-MCP-33); está en inglés, como las descripciones de las herramientas (P-MCP-36); el bloque de instrucciones va en `AGENTS.md` cuando existe y `CLAUDE.md` lo importa, y si no, en `CLAUDE.md` (P-MCP-34; lo escribe US-MCP-022).

## Reglas cubiertas

BR-MCP-CONS-009 (guía coherente con el contrato: cuatro temas, un código = una entrada, los dos caminos abiertos) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias en `main`**: US-MCP-005 (códigos de rechazo estables, que la guía explica), US-MCP-016 (`check_conflicts`, para "antes de integrar") y US-TMC-011 (`raptor undo --agent` por la CLI).
- **Bloqueo**: **US-MCP-023** (salida `--json` estable de la CLI, mismo hito, en construcción). La guía enseña el camino por la CLI con el contrato que US-MCP-023 deje en `main`; esta historia no fija ese esquema. Se acompañan en M3.
- **Pendiente, sin bloquear**: US-MCP-012 (undo de lo propio por MCP) no está en `main`. Mientras no esté, la guía enseña el undo solo por la CLI y no ofrece el undo por MCP (escenario «El undo por MCP solo aparece cuando la versión lo ofrece»).
- **Relación**: US-MCP-021 y US-MCP-022 instalan la guía y el bloque en el repo (M4); US-MCP-024 añade el camino por CLI de "¿voy a chocar?" (después de v0.1.0).
- **Bloquea**: US-WTL-013 (`raptor new --agent` instala esta guía en el worktree que crea, tal como la deja esta historia).
- **Cómo llega la guía al repo en M3**, antes de que exista US-MCP-021: **decidido (P-MCP-39, 2026-11-10)**, copia documentada. La historia añade un subcomando de solo lectura, sin reserva, que imprime la guía embebida con su marca de versión y hash, y la receta para dejarla en `.claude/skills/gitraptor/SKILL.md`; GitRaptor la deja así en su propio repo. En M4, `configure-agents` reconoce esa copia por la marca.
- **Condición de la parte CLI, cumplida**: TS-GRP-010 (perfil `mcp` por solicitante, S-01) está implementada en `main`, así que la guía puede enseñar la CLI como camino del agente.
- **Transversal (lo define el Arquitecto)**: dónde vive la guía (plugin de Claude Code o carpeta de skills del repo) y cómo se comprueba que sus códigos coinciden con el contrato.

## Criterios de Aceptación

**Escenario: La guía dice al agente que pregunte antes de integrar**

Dado la guía de GitRaptor disponible en una sesión de Claude Code en el repo "shop"
Cuando el agente la consulta antes de integrar su rama
Entonces la guía le indica que pregunte con quién va a chocar, por la herramienta de conflictos del MCP o por la CLI con salida JSON
  Y le indica que, si la respuesta nombra a otro agente, lo comunique al desarrollador antes de integrar

**Escenario: Ante un bloqueo, la guía manda leer el motivo y no rodearlo**

Dado "claude-1" recibe el rechazo "policy-denied" al commitear en "shop-feat-a"
Cuando consulta la guía
Entonces la guía le indica que lea el motivo y siga la acción del rechazo
  Y le indica que no intente la misma operación por otra vía, ni con Git directo ni con otro comando

**Escenario: La guía enseña a deshacer solo lo propio**

Dado la guía de GitRaptor disponible para "claude-1"
Cuando el agente busca cómo deshacer su último cambio
Entonces encuentra el undo de lo propio por la CLI con su nombre de agente, `raptor undo --agent claude-1`
  Y la guía le advierte que un undo que choca con trabajo de otro se detiene y no se fuerza

**Escenario: El undo por MCP solo aparece cuando la versión lo ofrece**

Dado una versión de GitRaptor cuyo servidor MCP no ofrece el undo de lo propio
Cuando el agente busca en la guía cómo deshacer
Entonces la guía no le indica ningún undo por MCP
  Y en una versión cuyo servidor MCP sí lo ofrece, la guía lo nombra junto al de la CLI

**Esquema del escenario: Cada código de rechazo tiene su explicación, en inglés**

Dado el código de rechazo estable "<código>" del contrato de GitRaptor
Cuando se busca en la guía
Entonces la guía dice, en inglés, qué significa y qué debe hacer el agente

Ejemplos:
| código |
| policy-denied |
| repo-not-enabled |
| confirmation-required |
| warnings-not-acknowledged (llega con US-MCP-018; la guía lo explica desde esa versión) |
| engine-unavailable |

**Escenario: Una versión no sale con un código sin explicar**

Dado un código de rechazo nuevo en el contrato, sin entrada en la guía
Cuando se prepara una versión de GitRaptor
Entonces la preparación falla y nombra el código sin entrada
  Y lo mismo ocurre si la guía nombra un código que el contrato ya no tiene

**Escenario: La guía no cierra el camino del MCP**

Dado Claude Code con la guía de GitRaptor y el servidor "gitraptor" registrado
Cuando el agente pide la lista de herramientas de GitRaptor
Entonces recibe el catálogo completo del MCP, el mismo que sin la guía

## Requisitos Técnicos

Arquitecto, 2026-11-10: [ADR-MCP-001, Enmienda (2026-11-10, superficie para agentes)](../../../../architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md#enmienda-2026-11-10-superficie-para-agentes), puntos A, C1 y F.

- **Fuente**: texto en inglés embebido en el binario como constante, sin texto del repo. Los códigos que nombra salen del catálogo de `crates/api` (F).
- **Puerta**: una prueba en CI compara en los dos sentidos los códigos de la guía con los códigos estables. Falla en el PR, no solo en la release (Validación 3 de la enmienda).
- **Destino**: skill de proyecto `.claude/skills/gitraptor/SKILL.md` y bloque en `AGENTS.md`/`CLAUDE.md` (P-MCP-34). Cada pieza lleva una marca con la versión y el hash del contenido, formato compartido con US-MCP-021/022.
- **Condición de entrada (C1)**: la guía solo enseña la CLI como camino del agente cuando SEC-MCP-01 (perfil `mcp` por solicitante) esté en `main` (ya lo está desde TS-GRP-010, PR #368) y US-MCP-023 también; hasta entonces enseña el MCP.
- **El MCP no cambia**: `instructions` sigue constante y `tools/list` es el mismo con guía y sin ella.
- **Cómo llega al repo en M3**: copia documentada con un comando de solo lectura que imprime la guía (P-MCP-39, decidida el 2026-11-10; ver [context.md](../context.md)).

## Diseño y Dev Spec

- **Diseño:** no aplica (texto para agentes; sigue la guía de contenido del design system).
- **Dev Spec:** **sí, ligera** (decisión del Arquitecto, 2026-11-10): [DS-US-MCP-020](../dev-specs/US-MCP-020-dev-spec.md), `spec-ready` (2026-11-21). Dos compuertas de la guía: la parte CLI del primer escenario depende de US-MCP-024 y el undo por MCP, de US-MCP-012 publicada; hasta entonces la guía enseña solo lo disponible. T008 (activar el camino CLI) espera a US-MCP-023.
