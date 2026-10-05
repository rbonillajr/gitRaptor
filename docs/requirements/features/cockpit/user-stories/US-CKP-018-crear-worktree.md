---
id: US-CKP-018
title: "El desarrollador prepara un worktree para un agente nuevo"
type: us
status: draft
priority: medium
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
    - US-CKP-014
tags:
  - cockpit
  - acciones-por-agente
  - crear-worktree
  - operacion-protegida
  - must
---

# US-CKP-018: El desarrollador prepara un worktree para un agente nuevo

## Descripción

**Como** desarrollador orquestador, **quiero** crear desde la TUI una rama y un worktree nuevos sobre la base confirmada, con la orden para lanzar al agente, **para** sumar un agente a la flota en segundos y sin errores de ruta.

**Valor**: BR-07 (Must).

## Reglas cubiertas

BR-CKP-VAL-001 · BR-CKP-ELIG-005 · BR-CKP-WF-005 (crear desactivado) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-014 (flujo de escritura); US-GRP-016 (base pendiente publicada).
- **Técnicas**: TS-CKP-002, TS-CKP-003.

## Criterios de Aceptación

**Escenario: Crear con la ruta por defecto**

Dado "main" confirmada en "/code/shop"
Cuando el desarrollador crea la rama "feat/pagos" sin indicar ruta
Entonces existen la rama "feat/pagos" desde "main" y el worktree "/code/shop-feat-pagos"
  Y la TUI muestra "Para lanzar Claude Code: cd /code/shop-feat-pagos && claude" sin lanzar ningún agente
  Y Deshacer borra el worktree y la rama creados

**Escenario: Nombre de rama no válido o existente**

Dado la rama "feat-login" ya existente
Cuando el desarrollador intenta crear "feat..pagos" o "feat-login"
Entonces se rechaza con "nombre de rama no válido" o "la rama ya existe" y nada cambia

**Escenario: La ruta ya existe**

Dado el directorio "/code/shop-feat-pagos" ya existente
Cuando el desarrollador crea "feat/pagos" con la ruta por defecto
Entonces se rechaza con "la ruta ya existe; elige otra" y el directorio no cambia

**Escenario: Plantilla de ruta del perfil**

Dado una plantilla de ruta en el perfil que apunta a "~/orca/workspaces/shop/<rama>"
Cuando el desarrollador crea "feat/pagos"
Entonces el worktree se crea en "~/orca/workspaces/shop/feat-pagos"

**Escenario: Sin base confirmada, crear desactivado**

Dado la rama base no confirmada
Cuando el desarrollador busca crear un worktree
Entonces la acción aparece desactivada con "confirma la rama base" y cómo confirmarla

**Escenario: Ruta UNC rechazada**

Dado una ruta UNC como "\\\\servidor\\compartido\\shop-x"
Cuando el desarrollador la indica como ruta
Entonces se rechaza con su motivo y nada cambia

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec)._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001.
- **Dev Spec:** pendiente. Rutas UNC: Pendiente: etapa de validación multiplataforma (Windows).
