---
id: US-WTL-004
title: "Los worktrees que Claude Code crea y borra por su cuenta quedan bajo la red de seguridad de GitRaptor"
type: us
status: draft
priority: high
milestone: M3
blocked_by:
  - US-WTL-001
  - US-WTL-002
created: 2026-11-10
updated: 2026-11-10
feature: worktree-lifecycle
related:
  context:
    - CTX-WTL-001
  adrs:
    - ADR-GRP-012
    - ADR-GRD-001
  stories:
    - US-WTL-001
    - US-WTL-002
  rules:
    - BR-WTL-001
    - BR-WTL-002
    - BR-WTL-006
    - BR-WTL-007
    - BR-WTL-010
  specs: []
ado:
  id: null
  url: null
tags:
  - worktree-lifecycle
  - claude-code
  - worktreecreate
  - worktreeremove
  - guardrails
  - must
---

# US-WTL-004: Los worktrees que Claude Code crea y borra por su cuenta quedan bajo la red de seguridad de GitRaptor

## Descripción

**Como** desarrollador orquestador, **quiero** que el worktree que Claude Code crea o borra por su cuenta pase por GitRaptor, registrado como suyo, con snapshot al borrarlo y bajo Guardrails, **para** que ningún worktree de agente quede fuera del Deshacer ni de las políticas del repo.

**Valor**: BR-07 y BR-11 (Must); NFR-01. Lo activa el humano; nunca concede permisos al agente.

## Criterios de Aceptación

**Escenario: El humano lo activa y Claude Code crea un worktree**

Dado el registro de worktrees de Claude Code activado por el desarrollador en el repo
  Y un `.worktreeinclude` que lista `target/`
Cuando Claude Code "claude-1" crea un worktree por su cuenta
Entonces el worktree sale con una rama con nombre y `target/` copiado desde la semilla
  Y la flota lo muestra como worktree de "claude-1"

**Escenario: Claude Code borra su worktree terminado**

Dado el worktree "agent-x" registrado de "claude-1", con 1 archivo sin commitear y sin sesión viva en él
Cuando Claude Code pide borrarlo
Entonces GitRaptor guarda antes un snapshot con la rama y el archivo sin commitear
  Y el worktree se borra y se puede restaurar durante 30 días

**Escenario: Borrado de un worktree con agente vivo**

Dado el worktree "agent-y" con una sesión de agente esperando el CI
Cuando Claude Code pide borrarlo
Entonces el borrado se rechaza indicando qué agente lo ocupa

**Escenario: Guardrails en el worktree del agente**

Dado una política del repo que prohíbe el force-push
Cuando Claude Code intenta un force-push desde un worktree que creó por su cuenta
Entonces la operación se bloquea como en cualquier otro worktree

**Escenario: El registro no concede permisos**

Dado el registro activado
Cuando Claude Code crea un worktree
Entonces el agente no gana ningún permiso que no tuviera antes

**Escenario: Sin activar, la configuración de Claude Code no cambia**

Dado un repo donde el desarrollador no activó el registro
Cuando GitRaptor observa el repo
Entonces la configuración de Claude Code queda sin cambios
  Y desactivarlo después la deja igual que antes de activarlo

**Escenario: El registro falla al borrar**

Dado el registro activado
Cuando Claude Code pide borrar un worktree y GitRaptor no puede registrarlo ni guardar el snapshot
Entonces el borrado no se ejecuta y se rechaza

## Criterios de Certificación

- Validar que el 100 % de los worktrees creados y borrados por Claude Code a través de sus eventos de creación y borrado quedan registrados y con snapshot al borrarse · *(método: prueba automatizada con repos temporales)*.

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño (activación por el humano).

## Dependencias

- **De User Stories**: Requiere US-WTL-001 (creación con rama y copia) y US-WTL-002 (descarte reversible y agente vivo protegido).
- **Bloqueo**: enmienda de ADR-GRP-012 pendiente, a cargo del Arquitecto (usar los eventos de worktree de Claude Code para registrar, snapshot y Guardrails, con rechazo del borrado si el registro falla). Hasta tenerla, la historia no se diseña.
- **Externas**: eventos `WorktreeCreate` y `WorktreeRemove` de Claude Code.
- **Cross-cutting / transversales**: cómo se toca la configuración de Claude Code de forma reversible: transversal (lo define el Arquitecto).
