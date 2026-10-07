---
id: US-CKP-026
title: "El desarrollador ve en la flota de quién es el último commit de cada worktree y con qué agente"
type: us
status: expanded
priority: should
created: 2026-10-07
updated: 2026-10-07
feature: cockpit
source: inline
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
  stories:
    - US-CKP-001
    - US-GRD-018
    - US-GRD-019
tags:
  - cockpit
  - autoria
  - co-authored-by
  - trailer
  - flota
  - should
---

# US-CKP-026: El desarrollador ve en la flota de quién es el último commit de cada worktree y con qué agente

## Descripción

**Como** desarrollador que orquesta agentes, **quiero** ver en la flota, bajo cada worktree, de quién es su último commit y con qué agente se hizo, **para** detectar sin salir de la TUI un commit de agente que no se declaró.

**Valor**: la decisión D6 de Rene (la persona es la autora y el agente va como `Co-Authored-By`) ya se ve en `raptor events` (US-GRD-019); la vista principal no la mostraba. DS-US-GRD-018 § 12 y el PR #128 dejaron esta historia pendiente.

> **Origen**: Decisión del orquestador (2026-10-07), validada por el PO. El PO aprobó la sublínea y el alcance, y cambió dos textos: un commit humano sin agente es "commit de Ana", sin sufijo (no es una falta), y la pista inferida es "posible Claude Code (inferido)".

## Reglas cubiertas

D6 (autoría del commit: persona autora, agente co-autor) · BR-CKP-CALC-001 (lo que el motor no publica no se inventa) · guía de contenido y símbolos del design system (DSYS-GRP-001 § 2.2 y § 5).

## Dependencias

- **Historias**: US-CKP-001 (la flota); US-GRD-019 (publica `events.authorship`, ya en `main`). Sin cambios de contrato.
- **Prioridad**: Should. Entra en M1 como la última historia del Cockpit y no bloquea su cierre: `raptor events` ya cubre la necesidad (decisión del orquestador, 2026-10-07, validada por el PO).
- **Transversal**: verificado en macOS; Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

## Criterios de Aceptación

**Escenario: Commit de una persona con el trailer de un agente**

Dado el worktree "feat-pagos" cuyo último commit es de "Ana" con `Co-Authored-By` de Claude Code
Cuando el desarrollador mira la flota
Entonces bajo la fila de "feat-pagos" se lee "commit de Ana con Claude Code"

**Escenario: Commit de un agente detectado que no dejó su trailer**

Dado el worktree "feat-api" cuyo último commit lo ejecutó Claude Code, detectado, sin trailer de agente
Cuando el desarrollador mira la flota
Entonces bajo su fila se lee "commit de Ana · ejecutado por Claude Code · sin trailer" con el símbolo de advertencia

**Escenario: Commit de una persona sin agente**

Dado el worktree "feat-docs" cuyo último commit es de "Ana", sin agente detectado, sin trailer y sin pista
Cuando el desarrollador mira la flota
Entonces bajo su fila se lee "commit de Ana", sin sufijo

**Escenario: Commit con un agente inferido y no confirmado**

Dado un commit sin atribuir con la pista de Claude Code y sin su trailer
Cuando el desarrollador mira la flota
Entonces se lee "commit de Ana · posible Claude Code (inferido)"

**Escenario: Sin autoría publicada no hay sublínea**

Dado un worktree cuyo último commit no tiene autoría publicada
Cuando el desarrollador mira la flota
Entonces su fila no tiene sublínea

**Escenario: En una terminal pequeña las filas mandan**

Dado una flota que no cabe en el alto con las sublíneas
Cuando el desarrollador mira la flota
Entonces se ven todas las filas que caben sin sublíneas

## Requisitos Técnicos

- **Datos**: la conexión de la TUI ya pide `events.authorship`. Tras el snapshot del repo, la TUI lee `events.history` (los 200 eventos más recientes) y toma, por worktree, el `commit` o `merge` más reciente con autoría. Después lo actualiza con cada `git.event` del flujo del repo. Si un commit queda fuera de esa página, no hay sublínea.
- **"Último commit del worktree"**: el último `commit` o `merge` que el motor observó en ese worktree (⚠️ ASSUMPTION del PO: equivale al último commit de su rama mientras la rama no cambie de worktree).
- **Textos**: i18n en/es; el autor pasa por el saneador (SEC-12). Glifos estructurales `└ •` (ASCII `` ` o ``) y el token `Warning` (`⚠`, ASCII `[!]`) para "sin trailer": se lee sin color.
- **Verificación**: snapshots de la flota en en/es con los tres casos (humano con trailer, agente sin trailer y sin agente) y pruebas de `update` sobre el historial y el flujo.

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 § 2.2 y § 5.
- **Dev Spec:** no aplica (historia pequeña; brief de `/nassa-core:implement`).
