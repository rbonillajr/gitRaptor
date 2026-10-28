---
id: US-TMC-015
title: "El desarrollador no rompe un rebase o un merge a medias al deshacer o restaurar"
type: us
status: draft
priority: medium
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
  stories: [US-TMC-002, US-TMC-009, US-GRP-003]
  specs: [DS-US-TMC-015]
covers: [BR-TMC-EDGE-004, D-TMC-28]
blocked_by: []
tags: [time-machine, undo, estados-especiales]
---

# US-TMC-015: El desarrollador no rompe un rebase o un merge a medias al deshacer o restaurar

## Descripción

**Como** desarrollador orquestador
**Quiero** que el undo y la restauración se detengan si hay un rebase o un merge en curso
**Para** no acabar con el repo en un estado mezclado que nadie sabe recuperar

**Valor**: el undo nunca empeora una situación ya delicada (S6 aceptado).

## Reglas cubiertas

BR-TMC-EDGE-004 (operación de Git en curso; otros worktrees, D-TMC-28) — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Undo con un rebase en curso**

Dado un rebase a medias en "feat-login"
Cuando un solicitante sin atribuir pide deshacer desde "feat-login"
Entonces el undo no se ejecuta
  Y el solicitante recibe el aviso de terminar o abortar el rebase de "feat-login" antes
  Y el repo no cambia

**Escenario: Restauración con un merge en curso**

Dado un merge a medias en "feat-login"
Cuando un solicitante sin atribuir pide restaurar "feat-login" a un punto anterior
Entonces la restauración no se ejecuta
  Y el solicitante recibe el aviso del merge a medias en "feat-login"
  Y el repo no cambia

**Escenario: Tras abortar la operación, el undo funciona**

Dado que en "feat-login" una operación borró trabajo sin commitear
  Y después se empezó un rebase y se abortó
Cuando un solicitante sin atribuir pide deshacer desde "feat-login"
Entonces el undo se ejecuta y el solicitante recibe qué operación deshizo
  Y con undos seguidos "feat-login" vuelve al estado anterior a la operación que borró el trabajo, sin pérdida

**Escenario: Una operación en curso en otro worktree no bloquea el undo**

Dado un merge a medias en el worktree principal
  Y una operación de "claude-1" en "feat-login" que no toca la rama del worktree principal
Cuando "claude-1" pide deshacer desde "feat-login"
Entonces se deshace la operación de "claude-1"
  Y el merge del worktree principal sigue a medias, sin cambios

**Escenario: La rama que reescribe un rebase en otro worktree no se mueve**

Dado que "feat-pagos" está rebasando la rama "B" y se paró a medias
  Y deshacer la última operación de "feat-login" movería "B"
Cuando un solicitante sin atribuir pide deshacer desde "feat-login"
Entonces el undo no se ejecuta
  Y el solicitante recibe el motivo: la rama "B" está en uso en "feat-pagos"
  Y el repo no cambia

## Requisitos Técnicos

- Precondición del aplicador: marcadores de rebase, merge, cherry-pick, revert, bisect o sequencer en cualquier worktree afectado detienen sin cambios (ADR-TMC-002 § 3).
- Un lock de Git ajeno presente se reporta como "Git ocupado" y nunca se borra.

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** [DS-US-TMC-015](../dev-specs/US-TMC-015-operacion-git-en-curso.md)

## Dependencias

- **Historias**: US-TMC-002, US-TMC-009; US-GRP-003 (estados especiales) de motor-local.
- **Externas**: ninguna.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
