---
id: US-TMC-009
title: "El desarrollador devuelve su worktree a cualquier punto del timeline"
type: us
status: draft
priority: high
created: 2026-10-03
updated: 2026-10-03
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-001, US-TMC-002, US-TMC-006, US-TMC-013]
covers: [BR-TMC-WF-003, BR-TMC-CONS-001, BR-TMC-CONS-002, BR-TMC-EDGE-002, BR-TMC-AUTH-001, D-TMC-16, D-TMC-20, D-TMC-23, D-TMC-25]
blocked_by: []
tags: [time-machine, restauracion]
---

# US-TMC-009: El desarrollador devuelve su worktree a cualquier punto del timeline

## Descripción

**Como** desarrollador orquestador
**Quiero** restaurar mi worktree a un punto concreto del timeline
**Para** volver a un estado bueno conocido aunque después hubo varias operaciones

**Valor**: recupera de una cadena de errores en un paso, y la restauración se puede deshacer.

## Reglas cubiertas

BR-TMC-WF-003 (restaurar; alcance D-TMC-20) · BR-TMC-CONS-001 (punto previo a la restauración) · BR-TMC-EDGE-002 (sin puntos en huecos) · BR-TMC-AUTH-001 (trabajo de otro actor, D-TMC-23; sin confirmación en Windows, TQ-14) · BR-TMC-CONS-002 (lo excluido no se toca al restaurar) — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Restaurar el worktree a un punto anterior**

Dado que "feat-login" tuvo tres operaciones de un solicitante que queda sin atribuir después del punto de las 10:00
Cuando ese solicitante restaura "feat-login" al punto de las 10:00
Entonces "feat-login" queda exactamente como estaba a las 10:00, incluido su trabajo sin commitear

**Escenario: La restauración alcanza lo que cambió después del punto y nunca lo excluido**

Dado que después del punto de las 10:00 un solicitante sin atribuir borró la rama "feat-login-v2" creada desde "feat-login"
  Y "feat-login" contiene "deploy.pem", excluido por credenciales, y el repo anidado "vendor/lib"
Cuando ese solicitante restaura "feat-login" al punto de las 10:00
Entonces la rama "feat-login-v2" vuelve a existir
  Y los worktrees que no cambiaron después de ese punto no se modifican
  Y "deploy.pem" y "vendor/lib" quedan en el disco tal como estaban, sin escribirse ni borrarse

**Escenario: La restauración se puede deshacer**

Dado una restauración de "feat-login" al punto de las 10:00
Cuando un solicitante sin atribuir pide deshacer desde "feat-login"
Entonces "feat-login" vuelve al estado previo a la restauración

**Escenario: Un punto incompleto o dentro de un hueco no se restaura**

Dado un punto que no tiene snapshot completo
Cuando un solicitante sin atribuir intenta restaurar a ese punto
Entonces la restauración se rechaza con el motivo
  Y el repo no cambia

**Escenario: En macOS y Linux, restaurar sobre trabajo de otro actor exige confirmación**

Dado que después del punto de las 10:00 "claude-1" hizo un commit en "feat-login"
  Y el solicitante de la restauración queda sin atribuir y pide desde macOS o Linux
Cuando pide restaurar "feat-login" al punto de las 10:00
  Y lo confirma de forma interactiva
Entonces "feat-login" queda como estaba a las 10:00, sin el commit de "claude-1"

**Esquema del escenario: Restaurar sobre trabajo ajeno se rechaza para otro agente y, en Windows, sin atribuir**

Dado que después del punto de las 10:00 "claude-1" hizo un commit en "feat-login"
Cuando <solicitante> pide restaurar "feat-login" al punto de las 10:00 desde <sistema>
Entonces la restauración se rechaza con el motivo, sin pedir confirmación
  Y el repo no cambia

Ejemplos:

| solicitante | sistema |
|-------------|---------|
| "claude-2" | cualquier sistema |
| un solicitante sin atribuir | Windows |

## Requisitos Técnicos

- Alcance (D-TMC-20): worktree pedido más las refs y los worktrees que cambiaron después del punto, calculados por diferencia entre el snapshot destino y el estado actual.
- Solo puntos válidos (ref del almacén + fila completa) y fuera de huecos (ADR-TMC-003 § 3).
- Restaurar un worktree borrado lo recrea sin checkout y escribe sus archivos en bruto (ADR-TMC-002 § 3).
- Permisos de US-TMC-013 sobre todo lo que la restauración deshace; solape de US-TMC-012.

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-001, US-TMC-002, US-TMC-006, US-TMC-013.
- **Externas**: ninguna.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux salvo la confirmación interactiva, que en el MVP no existe en Windows (D-TMC-23); mensajes en inglés y español.
