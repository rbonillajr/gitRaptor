---
id: US-DSK-018
title: "El usuario hace las acciones frecuentes con el teclado y las descubre en una paleta de comandos"
type: us
status: draft
priority: medium
created: 2026-10-28
updated: 2026-10-28
feature: desktop
source: inline
related:
  context:
    - CTX-DSK-001
  stories:
    - US-DSK-006
    - US-DSK-017
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-2
  - teclado
  - accesibilidad
  - should
---

# US-DSK-018: El usuario hace las acciones frecuentes con el teclado y las descubre en una paleta de comandos

## Descripción

**Como** usuario que pasa a diario por la app, **quiero** deshacer, rehacer, buscar y abrir las vistas principales con atajos, y encontrar cualquier acción escribiendo su nombre, **para** trabajar tan rápido como en la terminal y no depender del ratón.

**Valor**: BR-27 (UI-2, Should); NFR-09 (navegable por teclado). Nivel **UI-2**. Imprescindible. Patrón P1 del [benchmark](../research/benchmark-2026-10.md) (Cmd/Ctrl+Z).

## Reglas

- Los atajos siguen las convenciones de cada SO (Cmd en macOS, Ctrl en Windows y Linux).
- Un atajo nunca salta una confirmación: deshacer el trabajo de un agente sigue pidiendo lo que pide sin atajo (Q-DSK-3).
- Toda acción de la paleta respeta el mismo estado que su botón: si no está disponible, la paleta dice por qué.

## Dependencias

- **Historias**: US-DSK-006 (deshacer y rehacer), US-DSK-017 (búsqueda).
- **Qué falta en el motor**: nada; es trabajo de la app.

## Criterios de Aceptación

**Escenario: Deshacer con el atajo del sistema**

Dado que el usuario acaba de commitear "docs: update readme" desde la app en macOS
Cuando pulsa Cmd+Z
Entonces se deshace ese commit igual que con la acción "Deshacer"

**Escenario: Descubrir una acción en la paleta**

Dado la paleta de comandos abierta
Cuando el usuario escribe "proteger"
Entonces encuentra la acción de proteger el repo con su atajo, si lo tiene

**Escenario: Acción no disponible**

Dado "notas" ya protegido
Cuando el usuario busca "proteger" en la paleta
Entonces la acción aparece como no disponible porque el repo ya está protegido

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** pendiente (UI-2 no está en DSF-DSK-001).
- **Dev Spec:** pendiente.
