---
id: US-TMC-002
title: "El desarrollador deshace con un comando la última operación de su worktree"
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
  stories: [US-TMC-001]
covers: [BR-TMC-WF-001, BR-TMC-VAL-001, BR-TMC-CONS-001, D-TMC-19, D-TMC-23]
blocked_by: []
tags: [time-machine, undo]
---

# US-TMC-002: El desarrollador deshace con un comando la última operación de su worktree

## Descripción

**Como** desarrollador orquestador
**Quiero** que `raptor undo` devuelva el worktree desde el que lo invoco al estado previo a su última operación
**Para** recuperarme de un error en segundos, sin reflog ni cherry-pick (BRD P4)

**Valor**: un solo comando revierte la última operación del worktree actual, y el propio undo se puede revertir.

## Reglas cubiertas

BR-TMC-WF-001 (undo y ámbito por defecto) · BR-TMC-VAL-001 (nada que deshacer) · BR-TMC-CONS-001 (snapshot previo al undo) — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Deshacer la última operación del worktree actual**

Dado en "feat-login" una operación lanzada por un solicitante "sin atribuir" que descartó los cambios sin commitear de "a.rs"
Cuando ese solicitante pide deshacer la última operación desde "feat-login"
Entonces "feat-login" vuelve al estado previo a la operación con "a.rs" recuperado
  Y la Time Machine registra el undo con su solicitante y la operación sobre la que actuó

**Escenario: El undo queda protegido por su propio punto previo**

Dado una operación deshecha en "feat-login"
Cuando se consulta el historial de la Time Machine
Entonces existe un punto recuperable con el estado de "feat-login" justo antes del undo

**Escenario: El undo no actúa sobre otros worktrees**

Dado que la última operación del repo ocurrió en "feat-pagos" y la última de "feat-login" es anterior
Cuando un solicitante sin atribuir pide deshacer desde "feat-login"
Entonces se deshace la última operación de "feat-login"
  Y "feat-pagos" no cambia

**Escenario: No hay nada que deshacer**

Dado un worktree sin operaciones registradas por la Time Machine
Cuando un solicitante sin atribuir pide deshacer desde ese worktree
Entonces el repo no cambia
  Y el solicitante recibe el aviso de que no hay nada que deshacer

**Escenario: Si el punto previo al undo falla, el undo no se ejecuta**

Dado que guardar el punto previo al undo falla
Cuando un solicitante sin atribuir pide deshacer desde "feat-login"
Entonces el undo no se ejecuta
  Y "feat-login" queda como estaba
  Y el solicitante recibe el motivo

## Requisitos Técnicos

- Destino = estado previo a la operación más reciente no deshecha del worktree del cwd: su snapshot previo garantizado o la última captura válida anterior (ADR-TMC-003 § 4). ⚠️ **ASSUMPTION** (TQ-9): undos seguidos retroceden una operación más cada vez.
- El undo es una operación protegida con su propio snapshot previo y se aplica con el protocolo de ADR-TMC-002 § 3 (TS-TMC-003).
- El registro guarda el solicitante congelado, el canal y la operación sobre la que actuó (ADR-TMC-003 § 2; TS-TMC-002, TS-TMC-004).
- Nada que deshacer: `rechazada` con motivo y repo sin cambios (BR-TMC-VAL-001).

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-001.
- **Externas**: ninguna. Si la última operación es de otro actor, aplican US-TMC-012 (solape) y US-TMC-013 (permisos).
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
