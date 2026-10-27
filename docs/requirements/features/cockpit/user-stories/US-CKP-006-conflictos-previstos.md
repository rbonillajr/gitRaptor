---
id: US-CKP-006
title: "El desarrollador ve qué agentes van a chocar antes de hacer merge"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-27
feature: cockpit
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
  stories:
    - US-GRP-016
    - US-CKP-001
    - US-GRD-014
  specs:
    - DS-US-CKP-006
tags:
  - cockpit
  - prediccion-conflictos
  - demo
  - must
---

# US-CKP-006: El desarrollador ve qué agentes van a chocar antes de hacer merge

## Descripción

**Como** desarrollador orquestador, **quiero** ver por cada par de agentes, y de cada agente con la base, si comparten archivos o si su trabajo commiteado va a chocar, con los archivos y los hunks, **para** reordenar o coordinar el trabajo antes de que el conflicto exista.

**Valor**: BR-06 (Must) y KPI "≥ 70 % de conflictos detectados antes". Es la demo del BRD § 13.

## Reglas cubiertas

BR-CKP-CALC-002 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-001; US-GRD-014 y US-GRP-016 (rama base confirmada o pendiente, para los pares contra la base).
- **Técnicas**: TS-CKP-001 (predictor del daemon), que espera a SPIKE-CKP-001 (ADR-CKP-001). No es un bloqueo: el spike está definido; esta historia empieza cuando TS-CKP-001 esté integrada.

## Criterios de Aceptación

**Escenario: Demo del BRD § 13 — el conflicto se ve antes del merge y luego ocurre**

Dado 4 sesiones en "shop": "claude-1" y "claude-2" commitean cambios distintos en la misma función de "src/api.rs", y "claude-3" y "claude-4" trabajan en archivos propios
Cuando el desarrollador mira la TUI antes de cualquier merge
Entonces el par "claude-1 ↔ claude-2" muestra ⚡ conflicto previsto en "src/api.rs" con el rango de líneas del hunk
  Y al hacer el merge real de las dos ramas, Git produce el conflicto en "src/api.rs"

**Escenario: Lo sin commitear solo cuenta como solape**

Dado "claude-3" con "README.md" modificado sin commitear y "claude-4" con "README.md" modificado y commiteado
Cuando el motor publica la predicción
Entonces el par "claude-3 ↔ claude-4" muestra solo ⚠ solape en "README.md", sin ⚡

**Escenario: Cada agente contra la base confirmada**

Dado "main" como base confirmada y un commit en "main" que choca con lo commiteado en "feat-pagos"
Cuando el motor publica la predicción
Entonces el par "feat-pagos ↔ main" muestra ⚡ con el archivo y el hunk

**Escenario: La vista declara sus límites**

Dado una predicción visible
Cuando el desarrollador mira el panel de conflictos
Entonces el panel declara que el solape incluye lo sin commitear, que el conflicto previsto solo usa lo commiteado y que no aplica los drivers de merge ni `.gitattributes` del usuario

**Escenario: Predecir no deja rastro en el repo**

Dado el repo "shop" con su estado anotado (refs, índice, working trees, configuración)
Cuando el motor calcula la predicción de los 6 pares
Entonces el estado del repo es idéntico al anotado

**Escenario: Un worktree sin trabajo propio no forma pares entre worktrees**

Dado "feat-nuevo" recién creado desde la base, sin commits ni cambios
Cuando el motor publica la predicción
Entonces no aparece ningún par entre "feat-nuevo" y otro worktree

## Requisitos Técnicos

Detalle en [DS-US-CKP-006](../dev-specs/US-CKP-006-conflictos-previstos.md). La forma del contrato es la de [DS-TS-CKP-001 § Contrato de API](../dev-specs/TS-CKP-001-predictor-conflictos.md#contrato-de-api); mecanismo en ADR-CKP-001.

- La TUI consume la predicción publicada con la capacidad `prediction.pairs`: `RepoView.prediction` de la instantánea y el evento `prediction.changed` del ámbito repo. No calcula ni inventa pares (P7).
- El detalle de los pares ⚡ y ⚠ se pide con `prediction.list` (≤ 50 `pair_ids`) y solo se muestra si su `rev` es el del resumen (A2).
- Una fila con un ⚡ conocido (`current` o `recalculating`) lleva el símbolo y sube a la zona de atención (DS-US-CKP-002 D2).
- Panel de conflictos (ConflictAlert): ⚡ con archivo y rangos de líneas por lado y la marca de regla de merge; ⚠ con los archivos en común; los lados con el nombre de su fila o de la rama base (P5), unidos por `↔` (`<->` en ASCII).
- Línea de límites siempre visible al pie y el texto completo de los 8 límites con `?` (P3), con textos en/es en un catálogo propio.
- Los escenarios 1 y 5 se apoyan en las pruebas de TS-CKP-001 (T010, T011, `repo_intact`) y en la prueba de punta a punta `apps/cli/tests/tui_conflicts.rs`.

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (ConflictAlert, símbolos ⚡ ⚠).
- **Dev Spec:** [DS-US-CKP-006](../dev-specs/US-CKP-006-conflictos-previstos.md) (`draft`; bloqueada por T009 y el PR-B de TS-CKP-001).
