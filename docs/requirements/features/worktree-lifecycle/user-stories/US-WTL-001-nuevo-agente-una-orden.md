---
id: US-WTL-001
title: "El desarrollador suma un agente de Claude Code a la flota con una sola orden"
type: us
status: draft
priority: high
milestone: M3
blocked_by:
  - US-CKP-018
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
  stories:
    - US-CKP-018
    - US-MCP-019
  rules:
    - BR-WTL-001
    - BR-WTL-002
    - BR-WTL-003
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
  - semilla
  - copy-on-write
  - must
---

# US-WTL-001: El desarrollador suma un agente de Claude Code a la flota con una sola orden

## Descripción

**Como** desarrollador orquestador, **quiero** crear con `raptor new <rama> --agent claude -- "tarea"` un worktree con su rama, sus artefactos de compilación copiados desde una semilla quieta y Claude Code trabajando en la tarea, **para** lanzar varios agentes a la vez sin llenar el disco ni juntar Orca, worktrunk y pasos a mano.

**Valor**: BR-07 (Must). En la app de escritorio, la misma acción es «Nuevo agente» (alcance según CTX-DSK-001). Plataforma: macOS primero; Linux, Windows y discos sin copy-on-write: Pendiente: etapa de validación multiplataforma.

## Criterios de Aceptación

**Escenario: Worktree con copia y agente lanzado en una orden**

Dado un repo con semilla de `main` del toolchain y perfil actuales, sin nadie trabajando en ella
  Y un `.worktreeinclude` que lista `target/` y `node_modules/`
Cuando el desarrollador ejecuta `raptor new feat-login --agent claude -- "añade el login"`
Entonces existe un worktree en la rama con nombre "feat-login" con `target/` y `node_modules/` copiados desde la semilla
  Y Claude Code trabaja en ese worktree con la tarea "añade el login"
  Y la skill de GitRaptor para agentes está disponible en ese worktree
  Y la flota muestra a Claude Code en "feat-login"

**Escenario: Primera vez en un repo, la semilla se crea desde main**

Dado un repo sin semilla
Cuando el desarrollador ejecuta `raptor new feat-a --agent claude -- "tarea"`
Entonces GitRaptor crea la semilla desde `main` y después el worktree "feat-a" con la copia
  Y el desarrollador puede reconstruir la semilla cuando quiera con `raptor seed refresh`

**Escenario: Sin .worktreeinclude no se copia ningún ignorado**

Dado un repo sin `.worktreeinclude`
Cuando el desarrollador ejecuta `raptor new feat-b --agent claude -- "tarea"`
Entonces el worktree "feat-b" se crea sin ningún archivo ignorado copiado y el agente se lanza igual

**Escenario: Solo se copia lo que el repo lista**

Dado un `.worktreeinclude` que lista `target/` y una semilla que también tiene `.claude/runs/` y `.env`
Cuando el desarrollador crea el worktree "feat-c"
Entonces "feat-c" recibe `target/` y no recibe `.claude/runs/` ni `.env`

**Escenario: Rama ya sacada en otro worktree**

Dado la rama "feat-login" sacada en el worktree "../wt-login"
Cuando el desarrollador ejecuta `raptor new feat-login --agent claude -- "tarea"`
Entonces la creación se rechaza indicando que "feat-login" ya está en "../wt-login"
  Y no se crea ningún worktree ni se lanza ningún agente

**Escenario: Nunca HEAD separado**

Dado cualquier creación con `raptor new`
Cuando el worktree queda creado
Entonces su HEAD apunta a una rama con nombre, nunca a un commit suelto

**Escenario: Agente sin soporte completo**

Dado que el MVP solo lanza Claude Code
Cuando el desarrollador ejecuta `raptor new feat-d --agent codex -- "tarea"`
Entonces el worktree "feat-d" se crea con su copia, ningún agente se lanza y GitRaptor muestra la orden para lanzar Codex en "feat-d"

**Escenario: Ningún agente ni el MCP lanza un agente**

Dado un agente conectado por el MCP
Cuando pide crear un worktree
Entonces puede preparar el worktree como hasta ahora, sin lanzar ningún agente en él

**Escenario: Semilla de otro toolchain o perfil**

Dado una semilla generada con un toolchain distinto del actual
Cuando el desarrollador ejecuta `raptor new feat-e --agent claude -- "tarea"`
Entonces ningún artefacto de esa semilla se copia a "feat-e"

**Escenario: Semilla con trabajo en curso**

Dado una semilla en la que hay una sesión de agente
Cuando el desarrollador ejecuta `raptor new feat-g --agent claude -- "tarea"`
Entonces ningún artefacto se copia desde ella a "feat-g"

**Escenario: Claude Code no arranca**

Dado que Claude Code no está instalado o no arranca
Cuando el desarrollador ejecuta `raptor new feat-f --agent claude -- "tarea"`
Entonces el worktree "feat-f" queda creado, GitRaptor informa que el agente no se lanzó y muestra la orden para lanzarlo a mano

**Escenario: Quitar la skill deja el worktree como estaba**

Dado el worktree "feat-login" con la skill de GitRaptor instalada al lanzar
Cuando el desarrollador quita la skill
Entonces el worktree queda igual que antes de instalarla

## Criterios de Certificación

- Validar que crear un worktree con copia desde una semilla quieta tarda como máximo lo medido con worktrunk (7–10 s en el repo de GitRaptor) y que el `df` no baja el tamaño completo de `target/` · *(método: medición en macOS, como el diario del 2026-11-09)*.
- Verificar que ningún ignorado fuera del `.worktreeinclude` llega al worktree nuevo · *(método: prueba automatizada con repo temporal)*.

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño (CLI y «Nuevo agente» en la app).

## Dependencias

- **De User Stories**: Requiere US-CKP-018 (crear worktree sobre la base confirmada) · Relacionado US-MCP-019 (preparar sin lanzar por el MCP) · Bloquea US-WTL-004, US-WTL-006, US-WTL-007, US-WTL-009.
- **Bloqueo**: enmienda de BRD-GRP-001 § 6.4 pendiente de aplicar (GitRaptor lanza un agente una vez, a petición explícita del humano). Hasta aplicarla, la historia no arranca.
- **Externas**: skill de GitRaptor para agentes (feature mcp, trabajo W160), aún sin historia con id.
- **Preguntas abiertas que esta historia no fija**: 5b (qué hace `raptor new` con una semilla de otro toolchain o perfil), 11 (disco sin reflink), 14 (reutilizar una rama existente que no está sacada), 16 (ignorados que nunca se copian aunque el repo los liste). Se añaden escenarios cuando Rene las cierre.
- **Cross-cutting / transversales**: dónde vive la semilla y cuánto ocupa (Pregunta 9) y cómo se copia con copy-on-write: transversal (lo define el Arquitecto).
