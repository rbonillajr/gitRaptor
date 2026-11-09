---
id: US-DSK-014
title: "El usuario resuelve un conflicto de merge dentro de la app y termina la integración"
type: us
status: ready
priority: medium
created: 2026-10-28
updated: 2026-11-08
feature: desktop
source: inline
decision: "Decisión de Rene del 2026-11-08 (aceptación de las recomendaciones, Q-DSK-7 a Q-DSK-20), ver CTX-DSK-001 § 11"
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

Completado por el Arquitecto (2026-11-08). **Decisión del orquestador (2026-11-08), validada por Arquitecto**: continuar tras resolver tiene una sola historia dueña, esta (US-CKP-016 solo ofrece Abortar, Abrir en el editor y Cancelar), así que va aquí y no en una TS. Marcar resuelto es preparar el archivo: TS-CKP-008. **Bloqueada por**: TS-CKP-006 (`diff.index` marca los archivos en conflicto), TS-CKP-008, US-CKP-016 y US-TMC-015, y para la app, la presencia de sesión o el factor de ADR-GRP-017 § 4.

- Lectura `diff.conflict` en el módulo `diff` del canal: las versiones base, nuestra y suya de un archivo en conflicto, con los topes y la frontera sin filtros de `diff.worktree` (ADR-GRP-009 § 1), solo a la conexión que la pide; perfil completo, nunca `raptor-mcp`.
- Operación nueva `resolve-conflict` (ADR-CKP-002 § 1, versión menor): escribe en el worktree el resultado que el daemon compone a partir de la elección por fragmento (nuestra, suya o las dos) y lo prepara. La app manda elecciones, nunca el contenido del archivo; editar a mano es "Abrir en el editor" (`open-in-editor`).
- Operación nueva `continue-in-progress`: continúa un merge o un rebase **que dejó detenido el propio ejecutor**, como `abort-in-progress`. Con algún archivo aún en conflicto, `rejected` con la lista (escenario 2). El mensaje del merge es el que Git preparó, sin editor (ADR-CKP-002 § 6). Un rebase que vuelve a chocar queda detenido otra vez.
- Punto previo (NFR-01): snapshot `previo_garantizado` antes de escribir el archivo y antes de continuar; Deshacer o restaurar no rompe la operación a medias (US-TMC-015). Abortar sigue siendo `abort-in-progress` (escenario 3).
- Guardrails: `continue-in-progress` se decide otra vez como la operación original (Merge o Rebase), porque el estado cambió desde el plan; `resolve-conflict` no es gobernada.
- Capa (ADR-GRP-017 § 4): en la app, `cockpit` con presencia de sesión; si la operación detenida toca trabajo de otro actor, la confirmación de trabajo ajeno pide factor por acción; con identidad por ruta, factor por acción; agente o `raptor-mcp`, "no disponible".
- Pruebas: los tres escenarios en repo temporal; carrera (el archivo cambia entre preparar y ejecutar `resolve-conflict`: `rejected`); almacén lleno: `aborted`; captura de red: 0 conexiones.

## Diseño y Dev Spec

- **Diseño:** pendiente (UI-2 no está en DSF-DSK-001).
- **Dev Spec:** pendiente.
