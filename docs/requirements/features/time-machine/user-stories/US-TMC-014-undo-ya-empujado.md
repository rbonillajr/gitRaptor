---
id: US-TMC-014
title: "El desarrollador sabe cuándo lo que deshizo sigue en el remoto"
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
  stories: [US-TMC-002]
covers: [BR-TMC-EDGE-001, D-TMC-8, D-TMC-14]
blocked_by: []
tags: [time-machine, undo, remoto]
---

# US-TMC-014: El desarrollador sabe cuándo lo que deshizo sigue en el remoto

## Descripción

**Como** desarrollador orquestador
**Quiero** que el undo sea solo local y me avise si lo deshecho ya se empujó
**Para** no creer que un error publicado quedó resuelto y no arriesgar el remoto del equipo

**Valor**: evita la falsa sensación de seguridad (R6) sin que GitRaptor reescriba nunca el remoto.

## Reglas cubiertas

BR-TMC-EDGE-001 (solo local, con aviso) · D-TMC-8, D-TMC-14 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Deshacer un commit ya empujado**

Dado un commit de "feat-login" que el repo ya conoce como empujado al remoto
Cuando un solicitante sin atribuir lo deshace
Entonces el commit se deshace en local
  Y el solicitante recibe el aviso de que ese commit sigue en el remoto
  Y no se envía nada al remoto

**Escenario: Deshacer un commit que no se empujó**

Dado un commit local que el repo no conoce en el remoto
Cuando un solicitante sin atribuir lo deshace
Entonces el commit se deshace sin aviso sobre el remoto

**Escenario: La Time Machine nunca empuja**

Dado cualquier undo, redo o restauración sobre una rama con remoto
Cuando la operación termina
Entonces el remoto no recibe ningún push ni force-push de la Time Machine

**Escenario: Rama sin remoto configurado**

Dado un commit en una rama que no tiene remoto configurado
Cuando un solicitante sin atribuir lo deshace
Entonces el commit se deshace sin aviso sobre el remoto
  Y el solicitante no recibe ningún error

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-002.
- **Externas**: ninguna. La Time Machine no trae novedades del remoto: el aviso usa lo que el repo ya conoce.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
