---
id: US-WTL-013
title: "El desarrollador lanza un agente de Claude Code en un worktree nuevo, con la skill de GitRaptor instalada, con una sola orden"
type: us
status: ready
priority: high
milestone: M3
blocked_by:
  - US-WTL-001
  - US-MCP-020
created: 2026-11-10
updated: 2026-11-10
feature: worktree-lifecycle
related:
  context:
    - CTX-WTL-001
  business:
    - BRD-GRP-001
  adrs:
    - ADR-GRP-012
    - ADR-GRD-007
  stories:
    - US-WTL-001
    - US-MCP-019
    - US-MCP-020
    - US-MCP-021
    - US-CKP-018
  rules:
    - BR-WTL-004
    - BR-WTL-005
    - BR-WTL-016
  specs: []
ado:
  id: null
  url: null
tags:
  - worktree-lifecycle
  - raptor-new
  - lanzar-agente
  - skill
  - must
---

# US-WTL-013: El desarrollador lanza un agente de Claude Code en un worktree nuevo, con la skill de GitRaptor instalada, con una sola orden

## Descripción

**Como** desarrollador orquestador, **quiero** que `raptor new <rama> --agent claude -- "tarea"` cree el worktree (US-WTL-001), instale en él la skill de GitRaptor para agentes y lance Claude Code con la tarea, **para** sumar un agente a la flota con una sola orden, sin juntar Orca, worktrunk y pasos a mano.

**Valor**: BR-07 (Must). Segunda mitad de la antigua US-WTL-001 (partida el 2026-11-10). En la app, la misma acción es «Nuevo agente». GitRaptor lanza el agente **una sola vez y a petición explícita del humano**, según la enmienda de BRD-GRP-001 § 6.4 (2026-11-10); nunca a petición de un agente ni por el MCP (BR-WTL-004, NFR-02). Solo Claude Code (D2).

## Criterios de Aceptación

**Escenario: Una orden crea el worktree, instala la skill y lanza el agente**

Dado un repo con semilla vigente y un `.worktreeinclude` que lista `target/`
Cuando el desarrollador ejecuta `raptor new feat-login --agent claude -- "añade el login"`
Entonces existe el worktree "feat-login" con su copia (US-WTL-001)
  Y la skill de GitRaptor para agentes está disponible en ese worktree
  Y Claude Code trabaja en "feat-login" con la tarea "añade el login"
  Y la flota muestra a Claude Code en "feat-login"

**Escenario: Un agente no puede lanzar otro agente**

Dado un agente conectado por el MCP o ejecutando la CLI desde su sesión
Cuando pide `raptor new feat-x --agent claude -- "tarea"`
Entonces la petición se rechaza y queda auditada, y no se crea ni se lanza nada
  Y crear el worktree sin lanzar agente (US-MCP-019) sigue disponible para él como hasta ahora

**Escenario: Agente sin soporte completo**

Dado que el MVP solo lanza Claude Code
Cuando el desarrollador ejecuta `raptor new feat-d --agent codex -- "tarea"`
Entonces el worktree "feat-d" se crea con su copia, ningún agente se lanza y GitRaptor muestra la orden para lanzar Codex en "feat-d"

**Escenario: Claude Code no arranca**

Dado que Claude Code no está instalado o no arranca
Cuando el desarrollador ejecuta `raptor new feat-f --agent claude -- "tarea"`
Entonces el worktree "feat-f" queda creado, GitRaptor informa que el agente no se lanzó y muestra la orden para lanzarlo a mano

**Escenario: Un solo lanzamiento**

Dado el agente ya lanzado en "feat-login"
Cuando el agente termina o se cuelga
Entonces GitRaptor no lo relanza ni le envía otro prompt

**Escenario: La skill ya instalada con el mismo contenido**

Dado "feat-login" que ya tiene la skill con la marca de la versión actual
Cuando el desarrollador repite el lanzamiento
Entonces no se reescribe la skill

**Escenario: Quitar la skill deja el worktree como estaba**

Dado el worktree "feat-login" con la skill de GitRaptor instalada al lanzar
Cuando el desarrollador quita la skill
Entonces el worktree queda igual que antes de instalarla

## Criterios de Certificación

- Verificar que desde un agente simulado (CLI, JSON-RPC directo y MCP) `raptor new --agent` se rechaza y no lanza ningún proceso · *(método: prueba automatizada)*.
- Validar la orden completa (copia, skill y agente) en un repo real de macOS · *(método: demo en dogfooding)*.

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2._ Operación reservada según la Enmienda 2026-11-10 de ADR-GRD-007 (un agente no puede lanzarla). La skill es la de US-MCP-020 (copia por el mismo instalador y marca de versión y hash que US-MCP-021 y 022; P-MCP-39).

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño (CLI y «Nuevo agente» en la app).

## Dependencias

- **De User Stories**: Requiere **US-WTL-001** (worktree con copia) y **US-MCP-020** (la guía y su fuente embebida). Relacionado US-MCP-019, US-MCP-021 (el instalador que `raptor new` reutiliza en M4), US-CKP-018 (preparar sin lanzar). Bloquea US-WTL-004.
- **Enmiendas**: BRD-GRP-001 § 6.4 y ADR-GRD-007, aplicadas el 2026-11-10.
- **Cross-cutting / transversales**: cómo se pasa la tarea al agente sin guardarla ni mostrarla en el log (privacidad de ADR-GRP-012): transversal (lo define el Arquitecto).
