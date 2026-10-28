---
id: US-TMC-008
title: "El timeline refleja las correcciones de atribución sin reescribir quién deshizo qué"
type: us
status: draft
priority: medium
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
  stories: [US-TMC-002, US-TMC-006, US-GRP-010]
  specs: [DS-US-TMC-008]
covers: [BR-TMC-CONS-005, D-TMC-2, D-TMC-18]
blocked_by: []
tags: [time-machine, timeline, atribucion-vigente]
---

# US-TMC-008: El timeline refleja las correcciones de atribución sin reescribir quién deshizo qué

## Descripción

**Como** desarrollador orquestador
**Quiero** que el timeline muestre la atribución vigente tras corregir una detección, conservando intacto el registro de cada undo
**Para** confiar en "quién hizo qué" y poder auditar qué se deshizo y a petición de quién

**Valor**: una corrección arregla el pasado de la sesión sin falsear la historia de las recuperaciones.

## Reglas cubiertas

BR-TMC-CONS-005 (atribución vigente; registro del undo inmutable) · D-TMC-2 (Q37; retirar la corrección, Q50 de motor-local), D-TMC-18 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Corregir una atribución reatribuye la sesión en el timeline**

Dado tres commits de una sesión detectada como "claude-1" en "feat-login"
Cuando el desarrollador corrige esa atribución a "codex-1"
Entonces los tres commits figuran en el timeline como "codex-1" con origen registrado

**Escenario: Las demás sesiones no cambian**

Dado que en "feat-login" también hay eventos de la sesión de "claude-2"
Cuando el desarrollador corrige la sesión de "claude-1" a "codex-1"
Entonces los eventos de "claude-2" conservan su atribución

**Escenario: El registro de un undo no se reescribe**

Dado un undo ya ejecutado sobre un commit atribuido entonces a "claude-1"
Cuando el desarrollador corrige esa sesión a "codex-1"
Entonces el registro del undo conserva su solicitante y la operación sobre la que actuó
  Y el commit deshecho figura con la atribución vigente "codex-1 (registrado)"

**Escenario: Retirar la corrección devuelve la atribución detectada en el timeline**

Dado tres commits de una sesión detectada como "claude-1" en "feat-login" que el desarrollador corrigió a "codex-1"
Cuando el desarrollador retira esa corrección
Entonces los tres commits vuelven a figurar en el timeline como "claude-1 (detectado)"

**Escenario: Sin corrección no cambia nada**

Dado un timeline con eventos atribuidos a "claude-1"
Cuando se consulta el timeline sin ninguna corrección registrada
Entonces la atribución de esos eventos es la detectada

## Requisitos Técnicos

- El solicitante de undo, redo y restauración es inmutable en el oplog; el actor de los eventos se resuelve con la atribución vigente (ADR-TMC-003 § 5).
- Al recibir del motor un cambio de atribución de una sesión, se invalidan cachés; no se reescribe nada (ADR-GRP-013 § 6).

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** [DS-US-TMC-008](../dev-specs/US-TMC-008-timeline-atribucion-vigente.md) (approved).

## Dependencias

- **Historias**: US-TMC-002, US-TMC-006; US-GRP-010 (corregir atribución) de motor-local.
- **Externas**: ninguna. P17 de motor-local se cerró el 2026-10-27 (Q50): al retirar una corrección, sus eventos vuelven a la atribución detectada, y el escenario de retirar la corrección lo exige en el timeline.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
