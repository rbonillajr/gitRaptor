---
id: US-DSK-003
title: "El usuario sabe si GitRaptor está funcionando y qué hacer cuando no"
type: us
status: draft
priority: high
created: 2026-10-28
updated: 2026-10-28
feature: desktop
source: inline
related:
  context:
    - CTX-DSK-001
  stories:
    - US-CKP-003
    - US-GRP-004
    - US-GRP-015
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-1
  - estados-vacios
  - must
---

# US-DSK-003: El usuario sabe si GitRaptor está funcionando y qué hacer cuando no

## Descripción

**Como** usuario de la app, **quiero** saber en todo momento si el motor de GitRaptor está en marcha, conectado y al día, y qué hacer cuando no lo está, **para** no creer que mi repo está protegido y con historial cuando no lo está.

**Valor**: BR-27 (UI-1, Must); BR-01. Nivel **UI-1**. Incluye los estados vacíos: sin repos observados y repo sin copias de trabajo de agentes.

## Reglas

- La app nunca presenta como actual un dato de un motor parado o desconectado.
- Los estados del motor y de la conexión son los mismos que muestra la TUI (US-CKP-003).
- Un estado vacío dice qué hacer a continuación, sin culpar al usuario.

## Dependencias

- **Reutiliza**: US-CKP-003 (estados del motor y de la conexión), US-GRP-004 (autoarranque y lo ocurrido con la app cerrada), US-GRP-015 (primer repo); `engine.snapshot`, `scope.snapshot`, `daemon.replace`.
- **Qué falta en el motor**: DEP-DSK-5, arrancar el motor desde la app y distribuirla con el binario del motor. transversal (lo define el Arquitecto).

## Criterios de Aceptación

**Escenario: El motor no está en marcha**

Dado que el motor de GitRaptor está parado
Cuando el usuario abre la app
Entonces la app indica que el motor no está en marcha y que no se registra historial
  Y el usuario puede pedir que arranque

**Escenario: Se pierde la conexión con el motor**

Dado la app mostrando la flota de "tienda"
Cuando el motor deja de responder
Entonces la app indica que los datos pueden estar desactualizados y desde cuándo
  Y al volver la conexión se pone al día sin que el usuario haga nada

**Escenario: Versión del motor incompatible**

Dado un motor de una versión anterior a la que necesita la app
Cuando la app se conecta
Entonces el usuario sabe que debe actualizar el motor y cómo hacerlo

**Escenario: Sin repos observados**

Dado un motor en marcha sin ningún repo observado
Cuando el usuario abre la app
Entonces la app le propone añadir su primer repo o uno de los descubiertos

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** [design-flow.md](../design/design-flow.md) (DSF-DSK-001), estados vacíos y de conexión.
- **Dev Spec:** pendiente.
