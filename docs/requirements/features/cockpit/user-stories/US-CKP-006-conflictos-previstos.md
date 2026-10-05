---
id: US-CKP-006
title: "El desarrollador ve qué agentes van a chocar antes de hacer merge"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
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

_Pendiente — lo completa el Arquitecto (Dev Spec). Mecanismo: ADR-CKP-001._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (ConflictAlert, símbolos ⚡ ⚠).
- **Dev Spec:** pendiente (tras SPIKE-CKP-001).
