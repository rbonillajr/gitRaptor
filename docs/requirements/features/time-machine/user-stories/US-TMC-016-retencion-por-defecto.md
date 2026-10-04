---
id: US-TMC-016
title: "Los snapshots no llenan el disco y nunca se pierde el último punto antes de una operación destructiva"
type: us
status: draft
priority: medium
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
  stories: [US-TMC-001]
covers: [BR-TMC-TIME-001, D-TMC-15]
blocked_by: []
tags: [time-machine, retencion]
---

# US-TMC-016: Los snapshots no llenan el disco y nunca se pierde el último punto antes de una operación destructiva

## Descripción

**Como** desarrollador orquestador
**Quiero** que los puntos de más de 30 días se purguen con aviso previo y que nunca se purgue el previo a la última operación destructiva
**Para** usar la Time Machine a diario sin quedarme sin disco ni perder la recuperación que más importa

**Valor**: controla el crecimiento de los snapshots (R4) sin romper NFR-01.

## Reglas cubiertas

BR-TMC-TIME-001 (retención por defecto, purga segura, aviso) · D-TMC-15 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Purga de puntos antiguos con aviso**

Dado puntos de hace 40 días y de hace 5 días, sin retención configurada
Cuando corresponde purgar
Entonces el desarrollador recibe antes el aviso de qué se va a purgar
  Y se purgan los puntos de hace 40 días
  Y se conservan los de hace 5 días

**Escenario: El punto previo a la última operación destructiva nunca se purga**

Dado que la última operación destructiva del repo ocurrió hace 45 días
Cuando corresponde purgar
Entonces el punto previo a esa operación se conserva

**Escenario: Nada que purgar**

Dado que todos los puntos tienen menos de 30 días
Cuando corresponde purgar
Entonces no se purga nada

**Escenario: Una purga interrumpida no deja puntos a medias**

Dado una purga en curso de puntos de hace 40 días
Cuando la purga se interrumpe antes de terminar
Entonces todo punto que no se purgó por completo sigue disponible para restaurar

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-001.
- **Externas**: ninguna. Cuándo "corresponde purgar" lo define el Arquitecto.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
