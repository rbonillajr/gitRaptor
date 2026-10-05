---
id: US-CKP-017
title: "El desarrollador descarta el trabajo de un agente sin miedo a perderlo"
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
    - US-CKP-014
    - US-TMC-002
tags:
  - cockpit
  - acciones-por-agente
  - descartar
  - operacion-protegida
  - must
---

# US-CKP-017: El desarrollador descarta el trabajo de un agente sin miedo a perderlo

## Descripción

**Como** desarrollador orquestador, **quiero** borrar el worktree y la rama de un agente desde su fila, con Deshacer y avisado de lo que no se podrá recuperar, **para** limpiar intentos fallidos sin arriesgar trabajo que todavía importa.

**Valor**: BR-07 (Must); NFR-01. Mitiga R-CKP-6.

## Reglas cubiertas

BR-CKP-ELIG-004 · BR-CKP-EDGE-008 · BR-CKP-AUTH-003 (confirmación del plan con trabajo de un agente) · BR-CKP-ELIG-001 (columna descartar) · BR-CKP-EDGE-002 (HEAD separado) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-014 (flujo de escritura); US-TMC-002 (Deshacer).
- **Técnicas**: TS-CKP-002, TS-CKP-003.

## Criterios de Aceptación

**Escenario: Trabajo ya integrado, sin preguntar y con Deshacer**

Dado "feat-old" integrado en "main", sin cambios y sin sesión presente
Cuando el desarrollador descarta "feat-old"
Entonces el worktree y la rama "feat-old" se borran sin confirmación, porque no afecta trabajo de nadie, y la TUI ofrece Deshacer
  Y al deshacer, worktree y rama vuelven con su contenido

**Escenario: Trabajo sin integrar, confirmación con default No**

Dado "feat-wip" con 2 commits no integrados de "claude-2", su sesión Terminado
Cuando el desarrollador pide descartar "feat-wip" y responde con Intro
Entonces la TUI pidió confirmar, en un solo aviso, el plan "borrar worktree feat-wip y rama feat-wip" y "Se perderán 2 commits sin integrar de claude-2 (recuperables con Deshacer)"
  Y no se borra nada

**Escenario: Lo que el snapshot no guarda se nombra antes**

Dado "feat-wip" con ".env.local" y "node_modules/" ignorados y un repo anidado sin seguimiento en "vendor/lib"
Cuando el desarrollador pide descartar "feat-wip"
Entonces la confirmación es obligatoria y lista ".env.local", "node_modules/" y "vendor/lib" como no recuperables
  Y tras descartar, Deshacer recupera todo salvo lo listado

**Escenario: No se ofrece donde no procede**

Dado el worktree principal, un worktree en la rama base, uno en una rama protegida, uno con "claude-2" Inactivo y uno bloqueado con `git worktree lock`
Cuando el desarrollador selecciona cada uno
Entonces "Descartar" aparece desactivado con su motivo, y el bloqueado nunca se desbloquea en su nombre

**Escenario: HEAD separado, solo el worktree**

Dado "feat-tmp" con HEAD separado y sin cambios
Cuando el desarrollador descarta "feat-tmp"
Entonces se borra el worktree y no se borra ninguna rama

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec). Cuarentena y bloqueo: ADR-CKP-002._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (ConfirmPrompt con default No).
- **Dev Spec:** pendiente.
