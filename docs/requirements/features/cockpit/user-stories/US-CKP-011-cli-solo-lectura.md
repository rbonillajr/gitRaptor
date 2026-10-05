---
id: US-CKP-011
title: "El desarrollador consulta la flota y los conflictos desde la línea de comandos"
type: us
status: draft
priority: low
created: 2026-10-04
updated: 2026-10-04
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

_Pendiente — lo completa el Arquitecto (Dev Spec)._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (CLI).
- **Dev Spec:** pendiente.
