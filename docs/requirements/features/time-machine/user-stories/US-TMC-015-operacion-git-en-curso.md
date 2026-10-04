---
id: US-TMC-015
title: "El desarrollador no rompe un rebase o un merge a medias al deshacer o restaurar"
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
  stories: [US-TMC-002, US-TMC-009, US-GRP-003]
covers: [BR-TMC-EDGE-004]
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

BR-TMC-EDGE-004 (operación de Git en curso) — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Undo con un rebase en curso**

Dado un rebase a medias en "feat-login"
Cuando un solicitante sin atribuir pide deshacer desde "feat-login"
Entonces el undo no se ejecuta
  Y el solicitante recibe el aviso de terminar o abortar el rebase antes
  Y el repo no cambia

**Escenario: Restauración con un merge en curso**

Dado un merge a medias en "feat-login"
Cuando un solicitante sin atribuir pide restaurar "feat-login" a un punto anterior
Entonces la restauración no se ejecuta
  Y el repo no cambia

**Escenario: Tras abortar la operación, el undo funciona**

Dado que se abortó el rebase de "feat-login"
Cuando pide deshacer desde "feat-login"
Entonces se deshace la última operación de "feat-login"

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-002, US-TMC-009; US-GRP-003 (estados especiales) de motor-local.
- **Externas**: ninguna.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
