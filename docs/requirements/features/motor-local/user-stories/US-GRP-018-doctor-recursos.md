---
id: US-GRP-018
title: "El desarrollador diagnostica con raptor doctor si GitRaptor gasta de más"
type: us
status: draft
priority: low
created: 2026-10-05
updated: 2026-10-05
feature: motor-local
source: inline
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  adrs:
    - ADR-GRP-015
    - ADR-TMC-007
  stories:
    - US-GRP-017
    - US-TMC-022
tags:
  - motor-local
  - recursos
  - doctor
  - diagnostico
---

# US-GRP-018: El desarrollador diagnostica con raptor doctor si GitRaptor gasta de más

## Descripción

**Como** desarrollador que nota la máquina lenta o el disco lleno, **quiero** que `raptor doctor` me diga si GitRaptor está fuera de alguno de sus objetivos de consumo y qué puedo hacer, **para** descartar o confirmar a GitRaptor como causa sin investigar a mano.

**Valor**: un único comando de diagnóstico, que funciona aunque el motor esté parado o roto.

> **Origen**: decisión del orquestador (2026-10-05), validada por el PO (Could, fuera de M1) y el Arquitecto. Es la parte de `raptor doctor` que el PO separó de US-GRP-017.

## Reglas cubiertas

RES-10 (parte de `doctor`) · RES-05 y RES-09 (disco) — ver [non-functional.md](../../../../architecture/non-functional.md) § Consumo de recursos.

## Dependencias

- **Historias**: US-GRP-017 (método `engine.resources` y objetivos).
- **Externas**: `raptor doctor` aún no tiene historia dueña. Si esta historia llega primero, crea el comando con su sección de recursos; las comprobaciones de SEC-MCP-10 y SPIKE-GRD-002 se añaden en sus historias.
- **Transversal**: verificado en macOS; Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

## Criterios de Aceptación

**Escenario: Todo dentro de objetivo**

Dado el motor en marcha con todos los valores de consumo dentro de su objetivo
Cuando el desarrollador ejecuta `raptor doctor`
Entonces la sección de recursos dice que GitRaptor está dentro de sus objetivos

**Escenario: La Time Machine se acerca a su tope**

Dado un tope de disco de la Time Machine de 10 GiB y un almacén de 8,5 GiB
Cuando el desarrollador ejecuta `raptor doctor`
Entonces la sección de recursos avisa de que la Time Machine pasó el 80 % de su tope
  Y explica cómo cambiar el tope en el perfil

**Escenario: Diagnóstico con el motor parado**

Dado el motor parado
Cuando el desarrollador ejecuta `raptor doctor`
Entonces la sección de recursos muestra el disco del perfil y de la Time Machine
  Y dice que la CPU y la memoria no se pueden medir porque el motor no está en marcha
  Y el comando no arranca el motor
