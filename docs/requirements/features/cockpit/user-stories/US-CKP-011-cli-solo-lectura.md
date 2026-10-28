---
id: US-CKP-011
title: "El desarrollador consulta la flota y los conflictos desde la línea de comandos"
type: us
status: draft
priority: low
created: 2026-10-04
updated: 2026-10-27
feature: cockpit
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
  stories:
    - US-CKP-001
    - US-CKP-006
tags:
  - cockpit
  - cli
  - solo-lectura
  - should
---

# US-CKP-011: El desarrollador consulta la flota y los conflictos desde la línea de comandos

## Descripción

**Como** desarrollador orquestador, **quiero** `raptor status` y `raptor conflicts`, con salida legible y `--json`, **para** consultar la flota y los choques desde un script o sin abrir la TUI.

**Valor**: Should (Q-CKP-20). Mitiga R-CKP-4: el ⚡ se puede consultar sin mirar la TUI.

## Reglas cubiertas

BR-CKP-CONS-007 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-001 (`status`); US-CKP-006 (`conflicts`).

## Criterios de Aceptación

**Escenario: `status` muestra lo mismo que la TUI**

Dado la TUI y `raptor status --json` sobre "shop" en el mismo instante
Cuando el desarrollador compara las dos salidas
Entonces los worktrees, agentes, estados, archivos modificados y ahead/behind coinciden

**Escenario: `conflicts` lista pares sin contenido**

Dado ⚡ "claude-1 ↔ claude-2" en "src/api.rs"
Cuando el desarrollador ejecuta `raptor conflicts --json`
Entonces la salida incluye el par, el nivel, el archivo, los rangos de líneas y la antigüedad
  Y no incluye contenido de diff ni mensajes de commit

**Escenario: La CLI no ofrece acciones de escritura**

Dado la CLI del MVP
Cuando el desarrollador busca en la ayuda de `raptor` cómo integrar, rebasar o descartar un worktree
Entonces no existe ningún comando que lo haga y la ayuda remite a la TUI

## Requisitos Técnicos

Detalle en [DS-US-CKP-011](../dev-specs/US-CKP-011-cli-solo-lectura.md). La forma de la predicción es la de [DS-TS-CKP-001 § Contrato de API](../dev-specs/TS-CKP-001-predictor-conflictos.md#contrato-de-api).

- `raptor status --json` conserva su forma de US-GRP-001 y solo añade el campo `schema_version: 1` (C11); un test de punta a punta prueba que su `--json` coincide con la TUI en worktrees, agentes, estados, archivos y ahead/behind (C1).
- `raptor conflicts` lee los pares con `prediction.list`, sin métodos nuevos en el canal, y los imprime en texto o en un JSON propio con `schema_version: 1`, sin contenido de archivos ni mensajes de commit (C2, C6).
- Código de salida `4` con al menos un ⚡ conocido (`current` o `recalculating`); `0` sin él, aunque queden pares por calcular, lo que el JSON dice con `complete` (C4, C5).
- La CLI no tiene órdenes de BR-07; la ayuda de `raptor tui` dice que integrar, rebasar o descartar se hace en la TUI (C9).
- `conflicts` espera a T009 de TS-CKP-001; la paridad de `status` y la ayuda no.

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (CLI).
- **Dev Spec:** [DS-US-CKP-011](../dev-specs/US-CKP-011-cli-solo-lectura.md) (`draft`; `conflicts` bloqueado por T009 y el PR-B de TS-CKP-001).
