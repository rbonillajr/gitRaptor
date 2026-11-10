---
id: US-WTL-003
title: "El desarrollador ve qué agentes esperan su decisión, cuáles esperan el CI y cuáles se colgaron"
type: us
status: draft
priority: high
milestone: M3
blocked_by: []
created: 2026-11-10
updated: 2026-11-10
feature: worktree-lifecycle
related:
  context:
    - CTX-WTL-001
  adrs:
    - ADR-GRP-012
  stories:
    - US-CKP-001
    - US-CKP-002
    - US-DSK-004
  rules:
    - BR-WTL-009
    - BR-WTL-016
  specs: []
ado:
  id: null
  url: null
tags:
  - worktree-lifecycle
  - estado-del-agente
  - cockpit
  - github-actions
  - must
---

# US-WTL-003: El desarrollador ve qué agentes esperan su decisión, cuáles esperan el CI y cuáles se colgaron

## Descripción

**Como** desarrollador orquestador, **quiero** ver cada sesión de agente en uno de cinco estados (activo, esperando una decisión humana, esperando el CI, colgado o terminado) con las que me necesitan arriba, **para** atender primero al agente que está parado por mí y dejar de depender de scripts propios de supervisión.

**Valor**: BR-04 y BR-06 (Must). Amplía los estados de ADR-GRP-012 (activo, inactivo, terminado). En la CLI con salida JSON, la TUI y la app.

## Criterios de Aceptación

**Escenario: Esperando una decisión humana sube a la zona de atención**

Dado "claude-1" trabajando en "feat-a" y "claude-2" en "feat-b"
Cuando "claude-2" se detiene a pedir una decisión del humano
Entonces el cockpit muestra "claude-2" como «esperando una decisión humana» en la zona de atención, por encima de "claude-1" «activo»

**Escenario: Esperando el CI**

Dado "claude-3" sin actividad desde hace 20 minutos en "feat-c"
  Y una ejecución de GitHub Actions en curso para la rama "feat-c"
Cuando el cockpit actualiza el estado
Entonces "claude-3" aparece como «esperando el CI» y no como «colgado»

**Escenario: Colgado tras 15 minutos sin actividad**

Dado "claude-4" en "feat-d" con la sesión viva
  Y 15 minutos sin actividad del agente ni cambios en "feat-d"
  Y ninguna ejecución de CI en curso para "feat-d"
Cuando el cockpit actualiza el estado
Entonces "claude-4" aparece como «colgado» en la zona de atención

**Escenario: Umbral configurable**

Dado el umbral de «colgado» configurado en 30 minutos
  Y "claude-4" con 20 minutos sin actividad y sin CI en curso
Cuando el cockpit actualiza el estado
Entonces "claude-4" no aparece como «colgado»

**Escenario: Nunca activo si está colgado ni terminado si sigue vivo**

Dado "claude-5" cuya sesión sigue viva y lleva 16 minutos sin actividad y sin CI
Cuando el desarrollador consulta la flota
Entonces "claude-5" no figura como «activo» ni como «terminado»

**Escenario: Terminado**

Dado "claude-6" cuya sesión acabó
Cuando el cockpit actualiza el estado
Entonces "claude-6" aparece como «terminado» y sale de la zona de atención

**Escenario: Solo lectura del CI**

Dado "claude-3" «esperando el CI»
Cuando el desarrollador consulta su estado
Entonces GitRaptor lee el resultado de GitHub Actions sin lanzar, reintentar ni cancelar ninguna ejecución

**Escenario: El CI no se puede leer**

Dado un repo cuyo estado de GitHub Actions no se puede leer
Cuando un agente lleva 15 minutos sin actividad
Entonces el cockpit indica junto a ese agente que el estado del CI no se pudo leer
  Y nunca lo muestra como «esperando el CI»

## Criterios de Certificación

- Validar los cinco estados con al menos una sesión en cada uno · *(método: prueba automatizada con sesiones simuladas y repos temporales)*.
- Confirmar con el orquestador del proyecto que el estado del cockpit sustituye al supervisor y al watchdog de `bitacora/orquestador/` durante una jornada de dogfooding · *(método: demo en dogfooding)*.

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño (estados en la fila del cockpit y en la app).

## Dependencias

- **De User Stories**: Relacionado US-CKP-001 (flota en vivo), US-CKP-002 (zona de atención), US-DSK-004 · Bloquea US-WTL-002, US-WTL-011.
- **Bloqueo**: enmienda de ADR-GRP-012 pendiente, a cargo del Arquitecto (los estados «esperando una decisión humana», «esperando el CI» y «colgado» amplían su detección). Hasta tenerla, la historia no arranca.
- **Externas**: GitHub Actions, solo lectura; en M3 no se lee otro CI.
- **Cross-cutting / transversales**: cómo se mide la actividad del agente y del repo para el umbral de 15 minutos: transversal (lo define el Arquitecto).
