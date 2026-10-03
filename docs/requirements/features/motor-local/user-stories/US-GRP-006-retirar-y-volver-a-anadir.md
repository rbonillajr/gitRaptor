---
id: US-GRP-006
title: "El desarrollador recupera el historial de un repo que retiró y volvió a añadir"
type: us
status: draft
priority: medium
created: 2026-10-03
updated: 2026-10-03
feature: motor-local
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  stories:
    - US-GRP-004
    - US-GRP-005
    - US-GRP-009
tags:
  - motor-local
  - repos-observados
---

# US-GRP-006: El desarrollador recupera el historial de un repo que retiró y volvió a añadir

## Descripción

**Como** desarrollador orquestador, **quiero** retirar un repo de la observación sin perder sus datos y recuperarlos al volver a añadirlo, **para** pausar un repo sin pagar con su historial.

**Valor**: retirar un repo es reversible.

## Reglas cubiertas

BR-AUTH-001 · BR-EDGE-005 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-004; US-GRP-005 (lo ocurrido mientras estuvo retirado se trata como hueco); US-GRP-009 (atribuciones registradas que se recuperan y agente identificado que pide cambiar los repos).
- **Externas**: Servidor MCP F-001-05, por el escenario "Un agente no puede añadir ni retirar repos": el canal por el que el agente lo pide es del MCP. No bloquea la historia: el rechazo lo decide el motor y se verifica contra su capacidad.
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Escenario: Retirar un repo deja de observarlo sin borrar sus datos**

Dado el repo "demo" observado con 3 eventos en su historial
Cuando el desarrollador retira "demo" de la observación
  Y después se hace un commit en "feat-login"
Entonces ese commit no se registra
  Y los 3 eventos anteriores siguen guardados en el perfil de GitRaptor

**Escenario: Volver a añadir el repo recupera su historial**

Dado que el repo "demo" se retiró con 3 eventos en su historial y, mientras estaba retirado, se hizo un commit en "feat-login"
Cuando el desarrollador vuelve a añadir "demo"
Entonces el historial de "demo" contiene de nuevo los 3 eventos con sus atribuciones
  Y el commit hecho mientras estuvo retirado figura como "sin atribuir"

**Escenario: Un agente registrado no puede añadir ni retirar repos**

> US-GRP-001 (Must) ya exige el rechazo para cualquier agente (Q40); aquí se comprueba con un agente registrado en un repo observado.

Dado el repo "demo" observado y "otro agente: Codex" registrado en "feat-login"
Cuando ese agente pide añadir el repo "otro" o retirar "demo"
Entonces el motor rechaza la petición indicando que solo el desarrollador puede cambiar los repos observados
  Y la lista de repos observados no cambia
