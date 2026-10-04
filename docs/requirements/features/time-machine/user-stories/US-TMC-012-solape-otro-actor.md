---
id: US-TMC-012
title: "Un undo nunca sobrescribe trabajo posterior de otro actor"
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
  stories: [US-TMC-002, US-TMC-006]
covers: [BR-TMC-CONS-005, D-TMC-13]
blocked_by: []
tags: [time-machine, undo, solape]
---

# US-TMC-012: Un undo nunca sobrescribe trabajo posterior de otro actor

## Descripción

**Como** desarrollador orquestador
**Quiero** que un undo se detenga y me muestre el solape cuando tocaría cambios posteriores de otro actor en los mismos archivos o fragmentos
**Para** que recuperar lo de uno nunca destruya el trabajo de otro

**Valor**: la red de seguridad no puede causar pérdida de datos (NFR-01), ni siquiera al deshacer.

## Reglas cubiertas

BR-TMC-CONS-005 (solape) · D-TMC-13 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: El undo se detiene ante un solape**

Dado que "claude-1" editó la función "login" de "api.rs" a las 10:00
  Y un cambio "sin atribuir" modificó la misma función a las 10:05
Cuando se pide deshacer la operación de "claude-1" de las 10:00
Entonces el undo se detiene sin cambiar el repo
  Y el solicitante recibe los dos cambios en conflicto con su actor y su momento

**Escenario: Sin solape, el undo procede**

Dado que el cambio posterior de otro actor está en otro archivo
Cuando se pide deshacer la operación de "claude-1" de las 10:00
Entonces la operación de "claude-1" se deshace
  Y el cambio del otro actor sigue intacto

**Escenario: El desarrollador decide no seguir**

Dado un undo detenido por solape
Cuando el desarrollador decide no continuar
Entonces el repo queda exactamente como antes de pedir el undo

**Escenario: Solo cuentan los cambios posteriores**

Dado que un cambio "sin atribuir" modificó la función "login" de "api.rs" a las 09:55
  Y "claude-1" editó esa misma función a las 10:00
Cuando se pide deshacer la operación de "claude-1" de las 10:00
Entonces la operación de "claude-1" se deshace
  Y la función queda como la dejó el cambio de las 09:55

## Requisitos Técnicos

- Dueña de la detección de solape por archivo y por ref (ADR-TMC-005 § 5): cambios posteriores de otro actor según diferencias de árbol entre capturas y la atribución vigente de los eventos del intervalo.
- Atribución mixta en un intervalo cuenta como otro actor; los cambios anteriores no cuentan.
- Resultado: operación `rechazada` por solape con los cambios en conflicto (actor y momento); granularidad por fragmento pendiente de TQ-6.

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-002, US-TMC-006.
- **Externas**: ninguna. Las opciones que se ofrecen tras detenerse se precisan en el diseño (design-flow).
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
