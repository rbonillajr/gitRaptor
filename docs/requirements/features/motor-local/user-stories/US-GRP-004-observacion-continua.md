---
id: US-GRP-004
title: "El desarrollador encuentra lo ocurrido aunque no tuviera GitRaptor abierto"
type: us
status: draft
priority: high
created: 2026-10-03
updated: 2026-10-03
feature: motor-local
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  stories:
    - US-GRP-002
    - US-GRP-007
    - US-GRP-009
tags:
  - motor-local
  - continuidad
---

# US-GRP-004: El desarrollador encuentra lo ocurrido aunque no tuviera GitRaptor abierto

## Descripción

**Como** desarrollador orquestador, **quiero** que la actividad de mis repos se capture aunque no tenga abierta ninguna superficie de GitRaptor y que sobreviva a los reinicios, **para** que la Time Machine tenga una historia sin huecos.

**Valor**: cerrar la terminal o reiniciar la máquina no hace perder quién hizo qué.

## Reglas cubiertas

BR-CONS-005 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-002; US-GRP-007 (estado de sesiones detectadas que sobrevive al reinicio); US-GRP-009 (registros y atribuciones que sobreviven al reinicio).
- **Externas**: ninguna.
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Escenario: La actividad se captura sin ninguna superficie abierta**

Dado el repo "demo" observado
  Y no hay ninguna superficie de GitRaptor abierta (ni TUI, ni CLI, ni MCP)
Cuando se hacen 2 commits en el worktree "feat-login"
  Y después se consulta el estado del motor
Entonces el historial de eventos contiene los 2 commits en "feat-login" con la fecha y hora en que ocurrieron

**Escenario: Lo observado sobrevive a que el motor deje de ejecutarse y vuelva a arrancar**

Dado el repo "demo" observado con un historial de eventos atribuidos
  Y "otro agente: Codex" registrado en "feat-login"
  Y una sesión terminada de "Claude Code" en "feat-api"
Cuando el motor deja de ejecutarse y vuelve a arrancar
Entonces el historial de eventos y sus atribuciones son los mismos que antes
  Y "otro agente: Codex" sigue registrado en "feat-login"
  Y la sesión de "Claude Code" en "feat-api" sigue en "Terminado"

**Escenario: La observación se reanuda sola al volver a arrancar el motor**

Dado el repo "demo" observado
Cuando el motor deja de ejecutarse y vuelve a arrancar sin que el desarrollador abra ninguna superficie de GitRaptor
  Y después se hace un commit en "feat-login"
Entonces el commit aparece en el historial de eventos de "feat-login"
