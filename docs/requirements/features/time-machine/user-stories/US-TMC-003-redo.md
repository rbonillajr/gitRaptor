---
id: US-TMC-003
title: "El desarrollador rehace lo que deshizo por error"
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
  stories: [US-TMC-002, US-TMC-012, US-TMC-013]
covers: [BR-TMC-WF-001, BR-TMC-VAL-001, BR-TMC-CONS-005, BR-TMC-AUTH-001, D-TMC-13, D-TMC-23]
blocked_by: []
tags: [time-machine, redo]
---

# US-TMC-003: El desarrollador rehace lo que deshizo por error

## Descripción

**Como** desarrollador orquestador
**Quiero** revertir mi último undo con `raptor redo`
**Para** que equivocarme al deshacer no me cueste trabajo

**Valor**: deshacer deja de ser una decisión arriesgada.

## Reglas cubiertas

BR-TMC-WF-001 (redo; S5 aceptado: con cambios intermedios aplica el solape) · BR-TMC-VAL-001 · BR-TMC-CONS-005 (solape) · BR-TMC-AUTH-001 (solicitante, D-TMC-23) — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Rehacer el último undo**

Dado que en "feat-login" un solicitante que queda sin atribuir deshizo una operación suya que había borrado "b.rs"
Cuando ese mismo solicitante pide rehacer desde "feat-login"
Entonces "feat-login" vuelve al estado posterior a esa operación
  Y el contenido de "b.rs" sigue recuperable en el historial

**Escenario: El redo queda protegido por su propio punto previo**

Dado un redo ejecutado en "feat-login"
Cuando se consulta el historial de la Time Machine
Entonces existe un punto recuperable con el estado de "feat-login" justo antes del redo

**Escenario: No hay undo que rehacer**

Dado que el último evento de "feat-login" no es un undo
Cuando un solicitante sin atribuir pide rehacer
Entonces el repo no cambia
  Y el solicitante recibe el aviso de que no hay nada que rehacer

**Escenario: Otro actor cambió los mismos archivos después del undo**

Dado que tras el undo el agente "claude-1" modificó "b.rs"
Cuando un solicitante sin atribuir pide rehacer
Entonces el redo se detiene sin cambiar el repo
  Y el solicitante recibe el solape con el cambio de "claude-1"

**Escenario: Un agente no rehace trabajo de otro actor**

Dado que un solicitante sin atribuir deshizo un commit de "claude-2" en "feat-login"
Cuando "claude-1" pide rehacer ese undo
Entonces la petición se rechaza con el motivo
  Y el repo no cambia

## Requisitos Técnicos

- Redo disponible solo si el último evento del ámbito es un undo; restaura el snapshot previo de ese undo (ADR-TMC-003 § 4).
- Aplica la detección de solape de US-TMC-012 sobre lo cambiado desde el undo (S5) y los permisos de US-TMC-013.
- Semántica de undos y redos seguidos: pila por worktree, pendiente de TQ-9.

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-002, US-TMC-012, US-TMC-013.
- **Externas**: ninguna.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
