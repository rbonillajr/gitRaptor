---
id: US-TMC-005
title: "Las operaciones de Git crudo tienen punto previo cuando el repo usa los hooks de Guardrails"
type: us
status: draft
priority: medium
created: 2026-10-03
updated: 2026-10-04
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-004, US-GRD-001]
covers: [BR-TMC-CONS-003, D-TMC-4, D-TMC-9, D-TMC-10]
blocked_by: []
tags: [time-machine, git-crudo, guardrails]
---

# US-TMC-005: Las operaciones de Git crudo tienen punto previo cuando el repo usa los hooks de Guardrails

## Descripción

**Como** desarrollador orquestador
**Quiero** que, si mi repo tiene los hooks de Guardrails, las operaciones de Git crudo queden precedidas de un punto recuperable
**Para** que lo editado justo antes de un reset o un checkout de un agente tampoco se pierda

**Valor**: cierra el riesgo R2 en los repos con Guardrails, sin que la Time Machine dependa de ellos.

## Reglas cubiertas

BR-TMC-CONS-003 (nivel b con hooks) · D-TMC-4 (los hooks son de Guardrails) · D-TMC-10 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Con hooks de Guardrails, la operación de Git crudo tiene punto previo**

Dado un repo con los hooks de Guardrails activos
Cuando un agente ejecuta un checkout con Git crudo que sobrescribe "api.rs" sin commitear
Entonces existe un punto recuperable anterior al checkout con "api.rs"
  Y ese punto figura como "snapshot previo"

**Escenario: Sin hooks, la cobertura sigue siendo por observación**

Dado un repo sin los hooks de Guardrails
Cuando un agente ejecuta un checkout con Git crudo
Entonces la Time Machine no exige los hooks
  Y la cobertura de ese cambio figura como "capturado por observación"

**Escenario: Si el punto previo falla, no se presenta como protegido**

Dado un repo con los hooks de Guardrails activos
  Y que guardar el punto previo falla
Cuando un agente ejecuta una operación de Git crudo
Entonces esa operación no figura como "snapshot previo" en el historial

**Escenario: Una operación que no modifica el repo no genera punto previo**

Dado un repo con los hooks de Guardrails activos
Cuando un agente ejecuta con Git crudo una consulta que no modifica el estado del repo
Entonces la Time Machine no guarda ningún punto por esa consulta

## Requisitos Técnicos

- Comando de la CLI para hooks de Guardrails: snapshot del worktree con nivel `previo_hook`, tiempo máximo y resultado `completo` o `fallido` (ADR-TMC-004 § 3).
- Solicitante por ascendencia del proceso que ejecuta el hook (ADR-TMC-005 § 1).
- Sin recursión: las escrituras de la Time Machine desactivan los hooks (ADR-TMC-002 § 2). La Time Machine no instala ni exige hooks (Q22).

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-004; US-GRD-001 (Guardrails: instala los hooks que llaman al snapshot `previo_hook`).
- **Externas**: ninguna. Desbloqueada el 2026-10-04 por decisión de Rene Bonilla, 2026-10-04: los hooks de Guardrails están definidos en ADR-GRD-001 (aceptado) y los instala US-GRD-001, así que el bloqueo por F-001-04 pasa a ser dependencia de historia. Antes se bloqueaba en cruz con US-GRD-017, que ahora depende de esta historia. La Time Machine no instala hooks (Q22).
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
