---
id: US-DSK-012
title: "El usuario crea, cambia y borra ramas desde la app sin perder trabajo"
type: us
status: ready
priority: high
created: 2026-10-28
updated: 2026-11-08
feature: desktop
source: inline
decision: "Decisión de Rene del 2026-11-08 (aceptación de las recomendaciones, Q-DSK-7 a Q-DSK-20), ver CTX-DSK-001 § 11"
related:
  context:
    - CTX-DSK-001
  stories:
    - US-GRD-008
    - US-GRD-017
    - US-TMC-001
    - US-DSK-005
    - US-DSK-006
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-2
  - ramas
  - must
---

# US-DSK-012: El usuario crea, cambia y borra ramas desde la app sin perder trabajo

## Descripción

**Como** desarrollador novato, **quiero** ver todas las ramas del repo y crear, cambiar o borrar una con un clic, **para** organizar mi trabajo sin miedo a perder lo que no he commiteado.

**Valor**: BR-27 (UI-2, Must). Nivel **UI-2**. Imprescindible. Entra antes de v0.1.0 (Q-DSK-10, decisión de Rene del 2026-11-08).

## Reglas

- Cambiar de rama con trabajo sin commitear nunca lo pierde: hay punto de recuperación previo (US-TMC-001, NFR-01).
- Borrar una rama es destructivo: exige punto previo (US-GRD-017) y respeta las ramas protegidas (US-GRD-008).
- Una rama sacada en otra copia de trabajo no se puede sacar ni borrar aquí; el usuario sabe en cuál está.

## Dependencias

- **Reutiliza**: la operación `create-worktree` del catálogo para la variante "rama nueva para un agente" (US-DSK-016); US-GRD-008, US-GRD-017, US-TMC-001.
- **Historias**: US-DSK-005 (el grafo, que en UI-2 muestra todas las ramas), US-DSK-006 (deshacer).
- **Qué falta en el motor**: DEP-DSK-3 (lista de todas las ramas y sus commits, no solo las de los agentes) y DEP-DSK-4 (crear una rama suelta, cambiar de rama y borrar una rama). transversal (lo define el Arquitecto).

## Criterios de Aceptación

**Escenario: Crear una rama y cambiarse a ella**

Dado "main" sacada en la copia de trabajo principal
Cuando el usuario crea "fix-login" desde "main" y se cambia a ella
Entonces la copia de trabajo principal tiene "fix-login" sacada

**Escenario: Cambiar de rama con trabajo sin commitear**

Dado "fix-login" con "login.rs" cambiado sin commitear
Cuando el usuario se cambia a "main"
Entonces el cambio de "login.rs" queda guardado en un punto de recuperación
  Y el usuario sabe cómo recuperarlo

**Escenario: Borrar una rama protegida**

Dado "main" como rama protegida
Cuando el usuario intenta borrar "main"
Entonces "main" no se borra y el usuario conoce la regla que lo impide

**Escenario: Rama sacada en otra copia de trabajo**

Dado "feat-pagos" sacada en la copia de trabajo de "claude-1"
Cuando el usuario intenta cambiarse a "feat-pagos" en la copia principal
Entonces la copia principal no cambia
  Y el usuario sabe que "feat-pagos" está en la copia de trabajo de "claude-1"

## Requisitos Técnicos

Completado por el Arquitecto (2026-11-08). **Decisión del orquestador (2026-11-08), validada por Arquitecto**: las operaciones de rama tienen una sola historia dueña, esta, así que van aquí y no en una TS (Enabler Decision Gate); la lista de ramas sale de TS-CKP-007. **Bloqueada por**: TS-CKP-007, TS-CKP-002 y TS-CKP-003 (implementadas las dos últimas), y para la app, la presencia de sesión o el factor de ADR-GRP-017 § 4.

- Tres operaciones nuevas del catálogo (ADR-CKP-002 § 1, versión menor), servidas por `operation.prepare` y `operation.run`, cada una en su módulo del ejecutor (ADR-GRP-016): `create-branch` (rama local desde un oid del plan, sin sacarla), `switch-branch` (sacar una rama local en el worktree del solicitante) y `delete-branch` (borrar una rama local). Marcas: Cockpit sí, MCP no.
- Nombres de rama con las reglas L-02 de ADR-CKP-002 § 1 y siempre con el nombre completo de la ref.
- Punto previo (NFR-01): snapshot `previo_garantizado` antes de cualquier efecto; "Deshacer" de la Time Machine recrea la rama borrada en su oid, vuelve a la rama anterior y devuelve los cambios sin commitear (US-TMC-001, US-GRD-017).
- `switch-branch` con trabajo sin commitear: el plan lo dice y, al ejecutar, el snapshot lo guarda, la copia queda limpia y se cambia de rama (escenario 2). Clase destructiva. Una rama sacada en otro worktree se rechaza nombrando ese worktree (escenario 4, precondición común de ADR-CKP-002 § 1).
- Guardrails: `delete-branch` es gobernada como "borrar rama" (rama protegida de US-GRD-008 y mínimo de la rama base); `switch-branch` que descarta cambios sin commitear se gobierna como `reset-hard`; `create-branch` no es gobernada. El plan avisa de los commits que solo están en la rama que se borra.
- Capa (ADR-GRP-017 § 4): en la app, `cockpit` con presencia de sesión, porque todo se revierte en local; con identidad por ruta, factor por acción ligado al plan; desde la TUI, la capa `cockpit` de siempre; un agente solicitante o `raptor-mcp`, "no disponible". Borrar ramas **remotas** queda fuera (efecto remoto: ADR-CKP-002, Enmienda 2026-11-08).
- Pruebas: los cuatro escenarios en repo temporal; con el almacén lleno, `aborted` y repo igual (INF-GRP-001); captura de red: 0 conexiones; matriz de capa V4 de ADR-GRP-017 con el doble del factor.

## Diseño y Dev Spec

- **Diseño:** pendiente (UI-2 no está en DSF-DSK-001).
- **Dev Spec:** pendiente.
