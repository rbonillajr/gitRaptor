---
id: US-TMC-004
title: "El trabajo hecho fuera de GitRaptor queda capturado como punto recuperable"
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
  stories: [US-TMC-001, US-GRP-002, US-GRP-004]
covers: [BR-TMC-CONS-003, BR-TMC-CONS-002, D-TMC-9, D-TMC-10, D-TMC-16]
blocked_by: []
tags: [time-machine, captura-continua, git-crudo]
---

# US-TMC-004: El trabajo hecho fuera de GitRaptor queda capturado como punto recuperable

## Descripción

**Como** desarrollador orquestador
**Quiero** que lo que mis agentes o yo hacemos con Git crudo o en el editor se capture a medida que ocurre
**Para** recuperar trabajo aunque la operación no haya pasado por GitRaptor

**Valor**: la red de seguridad cubre el caso más frecuente, el agente que usa Git directamente, sin prometer más de lo que cumple.

## Reglas cubiertas

BR-TMC-CONS-003 (cobertura por observación, nivel b) · BR-TMC-CONS-002 · D-TMC-9, D-TMC-10, D-TMC-16 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Una edición fuera de GitRaptor queda capturada**

Dado un repo observado sin hooks de Guardrails
Cuando se modifica "api.rs" y se crea "util.rs" sin seguimiento en "feat-login" desde el editor
Entonces la Time Machine guarda un punto recuperable con ambos archivos
  Y ese punto figura como "capturado por observación"

**Escenario: Un reset destructivo con Git crudo deja recuperable el último estado capturado**

Dado que el último estado capturado de "feat-login" incluye "api.rs" modificado
Cuando un agente ejecuta un reset destructivo con Git crudo en "feat-login"
Entonces el último estado capturado antes del reset sigue disponible para restaurar
  Y figura como "capturado por observación", no como "snapshot previo"

**Escenario: Los archivos ignorados no se capturan**

Dado que se modifica ".env", ignorado por el repo
Cuando la Time Machine captura los cambios de "feat-login"
Entonces el punto no contiene ".env"

**Escenario: Una captura que falla no se presenta como protegida**

Dado que guardar una captura de "feat-login" falla
Cuando se consulta el historial de la Time Machine
Entonces ese cambio figura sin punto recuperable
  Y ningún punto se presenta como protegido para ese cambio

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-001; US-GRP-002 (eventos en vivo) y US-GRP-004 (observación continua) de motor-local.
- **Externas**: ninguna. Riesgo residual R2: lo editado entre la última captura y una operación destructiva de Git crudo puede perderse.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
