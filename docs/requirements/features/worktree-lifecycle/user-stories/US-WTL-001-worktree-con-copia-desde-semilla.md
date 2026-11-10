---
id: US-WTL-001
title: "El desarrollador crea un worktree con rama con nombre y artefactos copiados desde una semilla quieta"
type: us
status: ready
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

# US-WTL-001: El desarrollador crea un worktree con rama con nombre y artefactos copiados desde una semilla quieta

## Descripción

**Como** desarrollador orquestador, **quiero** crear con `raptor new <rama>` un worktree con su rama con nombre y sus artefactos de compilación copiados con copy-on-write desde una semilla quieta, **para** tener cada worktree listo en segundos sin duplicar el disco ni armar a mano la copia.

**Valor**: BR-07 (Must). Primera mitad de la antigua US-WTL-001, partida el 2026-11-10 por tamaño: **lanzar el agente e instalar la skill** es [US-WTL-013](./US-WTL-013-lanzar-agente-e-instalar-skill.md), que depende de esta. `raptor new` sin `--agent` solo crea el worktree. En la app de escritorio, la acción es «Nuevo worktree» (alcance según CTX-DSK-001). Plataforma: macOS primero; Linux, Windows y discos sin copy-on-write: Pendiente: etapa de validación multiplataforma.

## Criterios de Aceptación

**Escenario: Worktree con copia desde la semilla**

Dado un repo con semilla de `main` del toolchain y perfil actuales, sin nadie trabajando en ella
  Y un `.worktreeinclude` que lista `target/` y `node_modules/`
Cuando el desarrollador ejecuta `raptor new feat-login`
Entonces existe un worktree en la rama con nombre "feat-login" con `target/` y `node_modules/` copiados desde la semilla
  Y ningún agente se lanza ni se instala ninguna skill

**Escenario: Primera vez en un repo, la semilla se crea desde main**

Dado un repo sin semilla
Cuando el desarrollador ejecuta `raptor new feat-a`
Entonces GitRaptor crea la semilla desde `main` y después el worktree "feat-a" con la copia
  Y el desarrollador puede reconstruir la semilla cuando quiera con `raptor seed refresh`

**Escenario: Sin .worktreeinclude no se copia ningún ignorado**

Dado un repo sin `.worktreeinclude`
Cuando el desarrollador ejecuta `raptor new feat-b`
Entonces el worktree "feat-b" se crea sin ningún archivo ignorado copiado

**Escenario: Solo se copia lo que el repo lista**

Dado un `.worktreeinclude` que lista `target/` y una semilla que también tiene `.claude/runs/` y `.env`
Cuando el desarrollador crea el worktree "feat-c"
Entonces "feat-c" recibe `target/` y no recibe `.claude/runs/` ni `.env`

**Escenario: Rama ya sacada en otro worktree**

Dado la rama "feat-login" sacada en el worktree "../wt-login"
Cuando el desarrollador ejecuta `raptor new feat-login`
Entonces la creación se rechaza indicando que "feat-login" ya está en "../wt-login"
  Y no se crea ningún worktree

**Escenario: Crear sin lanzar agente por el MCP sigue como hasta ahora**

Dado un agente conectado por el MCP
Cuando pide crear un worktree
Entonces puede prepararlo como hasta ahora (US-MCP-019), sin lanzar ningún agente en él

**Escenario: Nunca HEAD separado**

Dado cualquier creación con `raptor new`
Cuando el worktree queda creado
Entonces su HEAD apunta a una rama con nombre, nunca a un commit suelto

**Escenario: Semilla de otro toolchain o perfil**

Dado una semilla generada con un toolchain distinto del actual
Cuando el desarrollador ejecuta `raptor new feat-e`
Entonces ningún artefacto de esa semilla se copia a "feat-e"

**Escenario: Semilla con trabajo en curso**

Dado una semilla en la que hay una sesión de agente
Cuando el desarrollador ejecuta `raptor new feat-g`
Entonces ningún artefacto se copia desde ella a "feat-g"

## Criterios de Certificación

- Validar que crear un worktree con copia desde una semilla quieta tarda como máximo lo medido con worktrunk (7–10 s en el repo de GitRaptor) y que el `df` no baja el tamaño completo de `target/` · *(método: medición en macOS, como el diario del 2026-11-09)*.
- Verificar que ningún ignorado fuera del `.worktreeinclude` llega al worktree nuevo · *(método: prueba automatizada con repo temporal)*.

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño (CLI y «Nuevo agente» en la app).

## Dependencias

- **De User Stories**: Requiere US-CKP-018 (crear worktree sobre la base confirmada) · Relacionado US-MCP-019 (preparar sin lanzar por el MCP) · Bloquea **US-WTL-013** (lanzar el agente e instalar la skill), US-WTL-004, US-WTL-006, US-WTL-007, US-WTL-009, US-WTL-012.
- **Sin bloqueos de enmienda**: la enmienda de BRD-GRP-001 § 6.4 (lanzar un agente) se aplicó el 2026-11-10 y afecta a US-WTL-013, no a esta historia.
- **Preguntas abiertas que esta historia no fija**: 5b (qué hace `raptor new` con una semilla de otro toolchain o perfil: provisional, crea el worktree sin copia y avisa), 11 (disco sin reflink: provisional, no se copia y se avisa), 14 (reutilizar una rama existente: provisional, se reutiliza), 16 (ignorados que nunca se copian: provisional, lista fija de exclusiones). Se añaden escenarios cuando Rene las cierre; los supuestos provisionales no bloquean.
- **Cross-cutting / transversales**: dónde vive la semilla y cuánto ocupa (Pregunta 9) y cómo se copia con copy-on-write: transversal (lo define el Arquitecto).
