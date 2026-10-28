---
id: US-TMC-011
title: "El desarrollador deshace solo lo que hizo un agente en un periodo"
type: us
status: draft
priority: high
created: 2026-10-03
updated: 2026-10-28
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-002, US-TMC-007, US-TMC-012, US-TMC-013, US-GRP-007, US-GRP-009, US-GRP-010]
  specs: [DS-US-TMC-011]
covers: [BR-TMC-WF-002, BR-TMC-VAL-001, BR-TMC-EDGE-002, BR-TMC-AUTH-001, D-TMC-2, D-TMC-22, D-TMC-23, D-TMC-31, D-TMC-32, D-TMC-33, D-TMC-34, D-TMC-35, D-TMC-36]
blocked_by: []
tags: [time-machine, undo, undo-por-agente]
---

# US-TMC-011: El desarrollador deshace solo lo que hizo un agente en un periodo

## Descripción

**Como** desarrollador orquestador
**Quiero** usar `raptor undo --agent <id> --since <duración>` para deshacer solo las operaciones de ese agente en el periodo
**Para** revertir el trabajo de un agente que se equivocó sin perder lo que hicieron los demás ni yo

**Valor**: es la promesa central de la Time Machine y la demo de validación del MVP (BRD § 13).

## Reglas cubiertas

BR-TMC-WF-002 (undo por agente con atribución vigente; destino por archivo y rama, D-TMC-31; selector, D-TMC-32; periodo, D-TMC-33; commit intercalado, D-TMC-34; barrera, D-TMC-35; worktree, D-TMC-36) · BR-TMC-VAL-001 · BR-TMC-EDGE-002 (nada de huecos) · BR-TMC-AUTH-001 (solicitante, D-TMC-23) · D-TMC-2, D-TMC-22 (P17 cerrada el 2026-10-27, Q50 de motor-local) — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

"claude-1", "claude-2" y "claude-9" son sesiones de Claude Code en el mismo worktree, y el undo las nombra por su sesión; "codex-1" es otro agente registrado. Todas las peticiones se hacen desde ese worktree.

**Escenario: Deshacer lo de un agente en los últimos 20 minutos**

Dado que en los últimos 20 minutos "claude-1" hizo dos commits, uno con su edición de "api.rs"
  Y un cambio "sin atribuir" modificó "README.md"
  Y el solicitante del undo queda sin atribuir
Cuando pide deshacer lo de "claude-1" en los últimos 20 minutos
  Y lo confirma de forma interactiva
Entonces se revierten los dos commits de "claude-1", también su edición de "api.rs"
  Y el cambio en "README.md" sigue intacto

**Esquema del escenario: Lo de otro agente en la misma rama**

Dado que en los últimos 20 minutos "claude-1" hizo dos commits
  Y "claude-2" hizo un commit en otros archivos de la misma rama <cuándo>
  Y un solicitante sin atribuir pide deshacer lo de "claude-1" en ese periodo
Cuando lo confirma de forma interactiva
Entonces <resultado>

Ejemplos:

| cuándo | resultado |
|--------|-----------|
| antes del primer commit de "claude-1" | se revierten los commits de "claude-1" y el de "claude-2" sigue aplicado |
| entre los dos commits de "claude-1" | el undo se detiene por solape en la rama, muestra el commit de "claude-2" y el repo no cambia |

**Esquema del escenario: Se usa la atribución vigente en el momento del undo**

Dado una sesión detectada como "Claude Code" que el desarrollador corrigió a "codex-1"
  Y ninguna otra sesión de "Claude Code" en ese periodo
  Y la corrección <estado de la corrección>
Cuando un solicitante sin atribuir pide deshacer lo de "Claude Code" en el periodo de esa sesión
  Y confirma de forma interactiva lo que se le muestre
Entonces <resultado>

Ejemplos:

| estado de la corrección | resultado |
|-------------------------|-----------|
| sigue vigente | ninguna operación de esa sesión se deshace |
| se retiró antes del undo | las operaciones de esa sesión se deshacen, porque vuelven a figurar como de "Claude Code" |

**Escenario: Lo ocurrido en un hueco nunca entra**

Dado que "claude-1" estaba registrado antes de un hueco de observación y en el hueco aparecieron commits
Cuando un solicitante sin atribuir pide deshacer lo de "claude-1" en un periodo que incluye el hueco
Entonces los commits del hueco siguen aplicados

**Esquema del escenario: Una petición por agente que no procede no cambia nada**

Dado <situación>
Cuando <solicitante> pide deshacer lo de <agente> en los últimos 20 minutos
Entonces el repo no cambia
  Y el solicitante recibe el motivo: <motivo>

Ejemplos:

| situación | solicitante | agente | motivo |
|-----------|-------------|--------|--------|
| commits de "claude-2" en los últimos 20 minutos | "claude-1" | "claude-2" | un agente no puede deshacer lo de otro |
| "claude-9" sin operaciones en los últimos 20 minutos | un solicitante sin atribuir | "claude-9" | ese agente no tiene nada que deshacer en este worktree en ese periodo |

**Escenario: Un undo sin agente no baja por debajo de un undo por agente**

Dado un commit "sin atribuir" anterior a los commits de "claude-1"
  Y que se deshizo lo de "claude-1" y ese undo no se ha rehecho
Cuando un solicitante sin atribuir pide un undo sin agente que alcanzaría el commit "sin atribuir"
Entonces la petición se rechaza con el motivo
  Y se le ofrece rehacer el undo por agente o restaurar un punto del timeline
  Y el repo no cambia

## Requisitos Técnicos

- Conjunto = operaciones con atribución vigente al agente en el periodo; excluye "sin atribuir", otros agentes y huecos (BR-TMC-WF-002).
- Destino por ruta y por ref: estado anterior al primer cambio del agente, sin tocar cambios de otros actores; el solape detiene (ADR-TMC-005 § 5).
- P17 de motor-local cerrada el 2026-10-27 (Q50): al retirar una corrección, sus eventos vuelven a la atribución detectada; cuenta la atribución vigente en el momento del undo (D-TMC-22 deja de bloquear).

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** [DS-US-TMC-011](../dev-specs/US-TMC-011-undo-por-agente.md)

## Dependencias

- **Historias**: US-TMC-002, US-TMC-007, US-TMC-012, US-TMC-013; US-GRP-007, US-GRP-009 y US-GRP-010 de motor-local.
- **Externas**: ninguna. Estuvo bloqueada por P17 de motor-local (D-TMC-22) hasta el 2026-10-27, cuando Q50 la cerró: al retirar una corrección, los eventos vuelven a la atribución detectada, y el undo usa la atribución vigente en su momento.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
