---
id: US-TMC-012
title: "Un undo nunca sobrescribe trabajo posterior de otro actor"
type: us
status: draft
priority: high
created: 2026-10-03
updated: 2026-10-27
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-002, US-TMC-006]
  specs: [DS-US-TMC-012]
covers: [BR-TMC-CONS-005, D-TMC-13, D-TMC-27]
blocked_by: []
tags: [time-machine, undo, solape]
---

# US-TMC-012: Un undo nunca sobrescribe trabajo posterior de otro actor

## Descripción

**Como** desarrollador orquestador
**Quiero** que un undo se detenga y me muestre el solape cuando tocaría cambios posteriores de otro actor en los mismos archivos o ramas
**Para** que recuperar lo de uno nunca destruya el trabajo de otro

**Valor**: la red de seguridad no puede causar pérdida de datos (NFR-01), ni siquiera al deshacer.

## Reglas cubiertas

BR-TMC-CONS-005 (solape; alcance del MVP: sin continuar, por archivo y por rama, ediciones sin Git como "sin agente") · D-TMC-13 · D-TMC-27 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: El undo se detiene ante un solape**

Dado que "claude-1" hizo a las 10:00 una operación que cambió "api.rs"
  Y a las 10:05 se vio un cambio "sin atribuir" en otra función de "api.rs"
Cuando se pide deshacer la operación de "claude-1" de las 10:00
Entonces el undo se detiene sin cambiar el repo
  Y el solicitante recibe los dos cambios en conflicto con su actor y el momento en que se vio cada uno

**Escenario: Sin solape, el undo procede**

Dado que el cambio posterior de otro actor está en otro archivo
Cuando se pide deshacer la operación de "claude-1" de las 10:00
Entonces la operación de "claude-1" se deshace
  Y el cambio del otro actor sigue intacto

**Escenario: Tras un solape no se puede continuar**

Dado un undo detenido por solape
Cuando el desarrollador revisa el resultado
Entonces el repo queda exactamente como antes de pedir el undo
  Y no se le ofrece continuar a pesar del solape

**Escenario: Solo cuentan los cambios posteriores**

Dado que un cambio "sin atribuir" modificó "api.rs" a las 09:55
  Y "claude-1" hizo a las 10:00 una operación que cambió "api.rs"
Cuando se pide deshacer la operación de "claude-1" de las 10:00
Entonces la operación de "claude-1" se deshace
  Y "api.rs" queda como lo dejó el cambio de las 09:55

**Escenario: Una edición posterior sin Git cuenta como de otro actor, aunque la haga el mismo agente**

Dado que "claude-1" hizo a las 10:00 una operación que cambió "api.rs"
  Y después "claude-1" editó "api.rs" sin usar Git
Cuando "claude-1" pide deshacer su operación de las 10:00
Entonces el undo se detiene sin cambiar el repo
  Y el cambio posterior se presenta como "sin agente"

## Requisitos Técnicos

- Dueña de la detección de solape por archivo y por ref (ADR-TMC-005 § 5): cambios posteriores de otro actor según diferencias de árbol entre capturas y la atribución vigente de los eventos del intervalo.
- Atribución mixta en un intervalo cuenta como otro actor; los cambios anteriores no cuentan.
- Resultado: operación `rechazada` por solape con los cambios en conflicto (actor y momento); granularidad por archivo y por ref en el MVP (por fragmento, más adelante).
- Una ruta que solo cambió otro actor después se conserva y se informa.
- Sin poder separar el cambio deshecho de lo posterior, se detiene con `overlap-unverified`.

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** [DS-US-TMC-012](../dev-specs/US-TMC-012-solape-otro-actor.md)

## Dependencias

- **Historias**: US-TMC-002, US-TMC-006.
- **Externas**: ninguna. Las opciones que se ofrecen tras detenerse se precisan en el diseño (design-flow).
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
