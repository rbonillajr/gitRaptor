---
id: US-TMC-005
title: "El borrado de ramas y el rebase con Git crudo tienen punto previo cuando el repo usa los hooks de Guardrails"
type: us
status: draft
priority: medium
created: 2026-10-03
updated: 2026-10-09
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

# US-TMC-005: El borrado de ramas y el rebase con Git crudo tienen punto previo cuando el repo usa los hooks de Guardrails

## Descripción

**Como** desarrollador orquestador
**Quiero** que, si mi repo tiene los hooks de Guardrails, el borrado de una rama y el rebase con Git crudo queden precedidos de un punto recuperable
**Para** que una rama con commits únicos que un agente borra, o el estado previo a un rebase, tenga siempre un punto previo recuperable

**Valor**: reduce el riesgo R2 en los repos con Guardrails, sin que la Time Machine dependa de ellos, y da a US-GRD-017 el punto que exige antes de permitir una operación destructiva. **R2 sigue parcialmente abierto**: `checkout -f`, `restore` y `reset --hard` con Git crudo no tienen un hook que corra antes de tocar el working tree (ADR-GRD-002 § 1), así que lo editado sin commitear antes de ellos sigue cubierto solo por observación (US-TMC-004).

> **Decisión del orquestador (2026-10-04), validada por Arquitecto/PO** (2026-10-09): el escenario 1 original (un `checkout` que sobrescribe "api.rs") no se puede cumplir, porque ningún hook de Guardrails corre antes de que un checkout cambie el working tree y Git ya se niega a un checkout o un rebase que sobrescriba ediciones sin commitear. Se reescribe con el borrado de una rama, se añade un escenario de rebase y se ajustan el título y el valor. Diseño en el [Brief](../../../../dev-briefs/us-tmc-005-pre-hook-snapshot.md) (D2, D16).

## Reglas cubiertas

BR-TMC-CONS-003 (nivel b con hooks) · D-TMC-4 (los hooks son de Guardrails) · D-TMC-10 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Con hooks de Guardrails, el borrado de una rama con Git crudo tiene punto previo**

Dado un repo con los hooks de Guardrails activos
  Y la rama "feat-x" con un commit que añade "api.rs" y que no está en ninguna otra rama
Cuando un agente borra "feat-x" con Git crudo
Entonces existe un punto recuperable anterior al borrado con "api.rs" de "feat-x"
  Y ese punto figura como "snapshot previo"

**Escenario: Con hooks de Guardrails, el rebase con Git crudo tiene punto previo**

Dado un repo con los hooks de Guardrails activos
Cuando un agente hace un rebase con Git crudo
Entonces la entrada del rebase en el historial figura como "snapshot previo"

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
  Y el hook avisa del fallo y deja pasar la operación (la denegación es de US-GRD-017)

**Escenario: Una operación que no modifica el repo no genera punto previo**

Dado un repo con los hooks de Guardrails activos
Cuando un agente ejecuta con Git crudo una consulta que no modifica el estado del repo
Entonces la Time Machine no guarda ningún punto por esa consulta

## Requisitos Técnicos

- El daemon toma el snapshot `previo_hook` dentro de `guard.evaluate`, en la misma llamada que decide, con la capacidad `guard.prior-snapshot`, y responde `priorSnapshot` = `complete` o `failed` (ADR-TMC-004 § 3 y su Enmienda 2026-10-09; ADR-GRD-003 § 7). El comando de la CLI es `raptor hook`.
- Solo con efecto `allow`, fuera del ejecutor, en `pre-rebase` y en `reference-transaction` `prepared` con un borrado de `refs/heads/*`. Un solo punto por comando de Git. Tiempo máximo 5 s; si vence, `failed` y ninguna fila completa.
- Cupo SEC-TMC-12 por solicitante, worktree y repo (⚠️ ASSUMPTION en BR-TMC-CONS-003; los cupos globales solo cuentan a los agentes, Q-GRD-37). Pasa por la misma puerta del `.git` que #226: el worktree y el repo los resuelve el daemon, nunca rutas que mande el hook.
- Solicitante por ascendencia del proceso que ejecuta el hook (ADR-TMC-005 § 1).
- Sin recursión: las escrituras de la Time Machine desactivan los hooks (ADR-TMC-002 § 2). La Time Machine no instala ni exige hooks (Q22).

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** no aplica; el plan es el Implementation Brief [`docs/dev-briefs/us-tmc-005-pre-hook-snapshot.md`](../../../../dev-briefs/us-tmc-005-pre-hook-snapshot.md), con su contrato de ejecución.

## Dependencias

- **Historias**: US-TMC-004; US-GRD-001 (Guardrails: instala los hooks que llaman al snapshot `previo_hook`).
- **Externas**: ninguna. Desbloqueada el 2026-10-04 por decisión de Rene Bonilla, 2026-10-04: los hooks de Guardrails están definidos en ADR-GRD-001 (aceptado) y los instala US-GRD-001, así que el bloqueo por F-001-04 pasa a ser dependencia de historia. Antes se bloqueaba en cruz con US-GRD-017, que ahora depende de esta historia. La Time Machine no instala hooks (Q22).
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
