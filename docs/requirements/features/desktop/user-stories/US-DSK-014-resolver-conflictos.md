---
id: US-DSK-014
title: "El usuario resuelve un conflicto de merge dentro de la app y termina la integración"
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
    - US-CKP-016
    - US-TMC-015
    - US-DSK-013
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-2
  - conflictos
  - should
---

# US-DSK-014: El usuario resuelve un conflicto de merge dentro de la app y termina la integración

## Descripción

**Como** desarrollador novato con un merge detenido por un conflicto, **quiero** ver cada archivo en conflicto con las dos versiones, elegir o combinar los cambios y continuar, **para** terminar la integración sin aprender los marcadores de conflicto ni los comandos para continuar o abortar.

**Valor**: BR-27 (UI-2, Should). Nivel **UI-2**. Imprescindible. Patrón P8 del [benchmark](../research/benchmark-2026-10.md).

## Reglas

- Las salidas de US-CKP-016: continuar cuando todo está resuelto o abortar y volver al estado previo.
- Deshacer o restaurar no rompe un merge o un rebase a medias (US-TMC-015).
- No se puede continuar con archivos aún en conflicto.

## Dependencias

- **Reutiliza**: US-CKP-016 (operación detenida), US-TMC-015; `abort-in-progress` del catálogo.
- **Historias**: US-DSK-013 (de dónde viene el merge detenido).
- **Qué falta en el motor**: DEP-DSK-4, marcar un archivo como resuelto y continuar el merge o el rebase, con punto previo. transversal (lo define el Arquitecto).

## Criterios de Aceptación

**Escenario: Resolver y continuar**

Dado un merge de "feat-login" en "main" detenido con "login.rs" en conflicto
Cuando el usuario elige qué cambios conservar en "login.rs" y continúa
Entonces el merge termina y "main" contiene la versión que eligió el usuario

**Escenario: Quedan archivos sin resolver**

Dado un merge detenido con "login.rs" resuelto y "auth.rs" en conflicto
Cuando el usuario intenta continuar
Entonces el merge sigue detenido
  Y el usuario sabe que falta resolver "auth.rs"

**Escenario: Abortar el merge**

Dado un merge detenido con "login.rs" en conflicto
Cuando el usuario decide abortar
Entonces "main" y la copia de trabajo vuelven al estado anterior al merge

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** pendiente (UI-2 no está en DSF-DSK-001).
- **Dev Spec:** pendiente.
