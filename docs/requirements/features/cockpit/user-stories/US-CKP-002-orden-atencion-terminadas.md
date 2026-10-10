---
id: US-CKP-002
title: "La lista pone primero lo que pide atención y no se llena de sesiones viejas"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
feature: cockpit
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
  stories:
    - US-CKP-001
    - US-GRP-005
    - US-GRP-011
tags:
  - cockpit
  - worktrees-en-vivo
  - orden
  - must
---

# US-CKP-002: La lista pone primero lo que pide atención y no se llena de sesiones viejas

## Descripción

**Como** desarrollador orquestador, **quiero** que la lista suba lo que pide atención, oculte las sesiones terminadas hace tiempo y distinga a cada agente aunque haya más de ocho, **para** encontrar en segundos el worktree que necesita mi intervención.

**Valor**: BR-04 (Must). Con 10 agentes, una lista sin orden es tan inútil como no tenerla.

## Reglas cubiertas

BR-CKP-WF-001 (orden, terminadas, último agente) · BR-CKP-TIME-002 · BR-CKP-EDGE-004 (presentación del compartido) · BR-CKP-EDGE-006 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-001; US-GRP-005 (hueco de observación); US-GRP-011 (worktree compartido). La subida por ⚡ se verifica cuando exista US-CKP-006; por ⛔, con US-CKP-019.
- **Huecos del motor**: DEP-CKP-4 (último agente y fin de sesión publicados).

## Criterios de Aceptación

**Escenario: Lo que pide atención sube, detrás del principal**

Dado "feat-pagos" con la sesión de "claude-1" Inactiva y un hueco de observación, y "feat-login" con "claude-2" Activo
Cuando el desarrollador mira la lista
Entonces el orden es: principal, "feat-pagos", "feat-login"
  Y después van Inactivo, Terminado y sin agente, en ese orden

**Escenario: Una sesión terminada se oculta a las 24 horas**

Dado "claude-4" Terminado en "feat-old" el lunes a las 9:00 y ninguna sesión posterior en ese worktree
Cuando son las 9:00 del martes
Entonces la sesión de "claude-4" deja de verse por defecto
  Y la fila conserva "último agente: claude-4 (terminó hace 1 d)" (en: "last agent: claude-4 (ended 1 d ago)")
  Y con el filtro "ver terminadas" vuelve a verse

**Escenario: Una sesión nueva desplaza a la terminada y no la reactiva**

Dado "claude-4" Terminado en "feat-old" hace 2 horas
Cuando "claude-4" vuelve a trabajar en "feat-old"
Entonces la fila muestra una sesión nueva Activa
  Y la sesión terminada deja de verse por defecto

**Escenario: Un worktree compartido muestra todas sus sesiones**

Dado "feat-pagos" con "claude-1" Activo, "claude-2" Inactivo y "claude-3" Terminado
  Y "claude-3" terminó después de que empezaran las otras dos
Cuando el desarrollador mira su fila
Entonces la fila se marca "compartido" y muestra las tres sesiones

**Escenario: Una sesión presente y una Terminada no hacen compartido el worktree**

Dado "feat-pagos" con "claude-1" Inactivo y "claude-2" Terminado hace 1 hora
  Y "claude-2" terminó después de que empezara "claude-1"
Cuando el desarrollador mira su fila
Entonces la fila muestra las dos sesiones
  Y no se marca "compartido"

**Escenario: Con más de ocho agentes el color se repite, pero el nombre no**

Dado 9 agentes con sesión en "shop"
Cuando el desarrollador mira la lista con color
Entonces "claude-9" comparte color con "claude-1" y su fila muestra su nombre y su símbolo de estado

## Requisitos Técnicos

- Sin cambios en el contrato: la TUI ordena y oculta con lo que ya publica `SessionView` (`started_utc_ms`, `ended_utc_ms`, `end_cause`, `state_since_utc_ms`). Las reglas están en la Dev Spec (D1 a D13).
- El orden y la ventana de 24 h son funciones puras de la vista. El color de cada agente es un hueco estable en el estado de la interfaz.
- "Compartido" son dos o más sesiones presentes. Hasta que US-GRP-011 publique la marca, lo calcula una sola función provisional de la TUI (excepción temporal a BR-CKP-CALC-001).
- La subida por ⚡, por ⛔ y por hueco depende de US-CKP-006, US-CKP-019 y US-GRP-005. Se implementa después de US-CKP-005 y US-CKP-003.

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (paleta `agent.1..8`, símbolos ● ◐ ○ con fallback ASCII).
- **Dev Spec:** [DS-US-CKP-002](../dev-specs/US-CKP-002-orden-atencion-terminadas.md).
