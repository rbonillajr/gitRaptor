---
id: US-WTL-009
title: "El desarrollador prepara cada worktree nuevo con su propio setup sin que un agente pueda ejecutar nada por él"
type: us
status: draft
priority: medium
milestone: M4
blocked_by:
  - US-WTL-001
created: 2026-11-10
updated: 2026-11-10
feature: worktree-lifecycle
related:
  context:
    - CTX-WTL-001
  stories:
    - US-WTL-001
  rules:
    - BR-WTL-015
    - BR-WTL-016
  specs: []
ado:
  id: null
  url: null
tags:
  - worktree-lifecycle
  - hooks-de-setup
  - nfr-02
  - should
---

# US-WTL-009: El desarrollador prepara cada worktree nuevo con su propio setup sin que un agente pueda ejecutar nada por él

## Descripción

**Como** desarrollador orquestador, **quiero** definir un hook de setup que corre al crear un worktree solo después de que yo lo apruebe, **para** dejar cada worktree listo (dependencias, configuración local) sin abrir la puerta a que un agente o el MCP ejecuten comandos.

**Valor**: BR-07 (Should, v0.1.0); NFR-02.

## Criterios de Aceptación

**Escenario: Hook aprobado corre al crear**

Dado un hook de setup definido por el desarrollador y aprobado por él
Cuando el desarrollador ejecuta `raptor new feat-a --agent claude -- "tarea"`
Entonces el hook corre en "feat-a" antes de lanzar el agente
  Y GitRaptor informa su resultado

**Escenario: Hook sin aprobar no corre**

Dado un hook de setup definido y sin aprobar
Cuando el desarrollador crea un worktree
Entonces el hook no corre y GitRaptor pide su aprobación

**Escenario: Ni el MCP ni un agente lo disparan**

Dado un hook de setup aprobado
Cuando un agente prepara un worktree por el MCP
Entonces el hook no corre

**Escenario: El hook falla**

Dado un hook aprobado que termina con error
Cuando el desarrollador crea "feat-b"
Entonces el worktree "feat-b" queda creado y GitRaptor informa el fallo del hook

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño (aprobación del hook por el humano).

## Dependencias

- **De User Stories**: Requiere US-WTL-001.
- **Pendiente para pasar a ready**: Pregunta 17 de CTX-WTL-001 (si un hook aprobado vuelve a pedir aprobación cuando cambia su contenido) y una pregunta nueva para Rene: si el hook falla, ¿se lanza igual el agente?
- **Cross-cutting / transversales**: cómo se ejecuta el hook sin shell libre ni disparo desde el MCP: transversal (lo define el Arquitecto, NFR-02).
