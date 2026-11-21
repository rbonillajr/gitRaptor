---
id: US-CKP-018
title: "El desarrollador prepara un worktree para un agente nuevo"
type: us
status: ready
priority: medium
blocked_by:
  - US-MCP-019
  - US-CKP-014
  - US-CKP-017
created: 2026-10-04
updated: 2026-11-21
feature: cockpit
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
  stories:
    - US-GRP-016
    - US-CKP-014
    - US-CKP-017
    - US-MCP-019
    - US-WTL-001
  specs:
    - DS-US-CKP-018
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

- **Historias**: US-MCP-019 (dueña de la operación de crear worktree: plantilla de ruta y motivos de rechazo; esta historia la reutiliza); US-CKP-014 (flujo de escritura de la TUI); US-CKP-017 (descarte, que hace de Deshacer); US-GRP-016 (base pendiente publicada).
- **Técnicas**: TS-CKP-002, TS-CKP-003 (implementadas).
- **Decisión del orquestador (2026-11-21), validada por PO**: esta historia se mantiene y no se absorbe en US-WTL-001. Es la superficie de la TUI para preparar un worktree sin lanzar agente (CTX-WTL-001, Pregunta 7); `raptor new` y la copia desde la semilla son de US-WTL-001, por la CLI. Ninguna de las dos construye la operación de crear worktree: la construye US-MCP-019 y las dos la reutilizan. US-WTL-001 ya no depende de esta historia. Pasa a `ready` como historia; su construcción sigue bloqueada por G1 (operación de US-MCP-019 sin mezclar en `main`) y G2 (flujo de escritura de US-CKP-014 y descarte de US-CKP-017, sin mezclar) de su Dev Spec.

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

**Escenario: Poco espacio libre, no se crea el worktree**

Dado un volumen con menos espacio libre que el suelo (el mayor de 5 GiB y el 5 % del volumen)
Cuando el desarrollador crea la rama "feat/pagos"
Entonces se rechaza con "queda poco espacio libre en el disco del worktree nuevo; libera espacio" y nada cambia

**Escenario: Ruta UNC rechazada**

Dado una ruta UNC como "\\\\servidor\\compartido\\shop-x"
Cuando el desarrollador la indica como ruta
Entonces se rechaza con su motivo y nada cambia

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec)._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001.
- **Dev Spec:** [DS-US-CKP-018](../dev-specs/US-CKP-018-crear-worktree.md) (approved). Rutas UNC: Pendiente: etapa de validación multiplataforma (Windows).
