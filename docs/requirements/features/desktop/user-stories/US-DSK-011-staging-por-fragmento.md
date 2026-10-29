---
id: US-DSK-011
title: "El usuario elige qué fragmentos o líneas de un archivo entran en el commit"
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
    - US-DSK-009
    - US-DSK-010
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-2
  - staging
  - should
---

# US-DSK-011: El usuario elige qué fragmentos o líneas de un archivo entran en el commit

## Descripción

**Como** usuario que revisa un archivo con varios cambios mezclados, **quiero** preparar para el commit solo algunos fragmentos o líneas y dejar el resto sin commitear, **para** entregar commits pequeños y con un solo propósito, como pide el equipo.

**Valor**: BR-27 (UI-2, Should); apoya BR-18 (diffs pequeños). Nivel **UI-2**. Imprescindible.

## Reglas

- Preparar y quitar fragmentos modifica el índice del repo: es una escritura y tiene punto de recuperación previo (NFR-01; Q-DSK-2).
- Lo que no se prepara sigue intacto en la copia de trabajo.

## Dependencias

- **Historias**: US-DSK-009 (diff por fragmento), US-DSK-010 (commit de "lo preparado").
- **Reutiliza**: la operación `commit` con "todo lo preparado" (ADR-CKP-002 § 1).
- **Qué falta en el motor**: DEP-DSK-4, operaciones de preparar y quitar del índice por fragmento y por línea. transversal (lo define el Arquitecto).

## Criterios de Aceptación

**Escenario: Preparar un solo fragmento**

Dado "pagos.rs" con un fragmento que corrige un error y otro que añade un log
Cuando el usuario prepara solo el fragmento de la corrección y commitea
Entonces el commit contiene solo la corrección
  Y el log sigue cambiado sin commitear

**Escenario: Quitar una línea preparada**

Dado un fragmento preparado de 5 líneas en "pagos.rs"
Cuando el usuario quita del commit una de esas líneas
Entonces quedan preparadas 4 líneas y la quinta sigue en la copia de trabajo

**Escenario: El archivo cambió mientras el usuario preparaba**

Dado que "claude-1" modifica "pagos.rs" mientras el usuario prepara un fragmento
Cuando el usuario confirma la preparación
Entonces no se prepara nada sobre una versión vieja del archivo
  Y el usuario sabe que debe revisar los cambios de nuevo

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** pendiente (UI-2 no está en DSF-DSK-001).
- **Dev Spec:** pendiente.
