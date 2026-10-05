---
id: US-CKP-015
title: "El desarrollador pone al día la rama de un agente sobre la base"
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
tags:
  - cockpit
  - acciones-por-agente
  - rebase
  - operacion-protegida
  - must
---

# US-CKP-015: El desarrollador pone al día la rama de un agente sobre la base

## Descripción

**Como** desarrollador orquestador, **quiero** rebasar la rama de un agente que ya terminó sobre la base confirmada, con snapshot previo y Deshacer, **para** resolver la divergencia antes de integrar sin pisar a un agente que sigue trabajando.

**Valor**: BR-07 (Must).

## Reglas cubiertas

BR-CKP-ELIG-003 · BR-CKP-ELIG-001 (columna rebase) · BR-CKP-EDGE-004 (sesión más restrictiva) · BR-CKP-WF-005 (rebase desactivado) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-014 (flujo de escritura).
- **Técnicas**: TS-CKP-002, TS-CKP-003.

## Criterios de Aceptación

**Escenario: Rebase de una rama terminada y Deshacer**

Dado "claude-3" Terminado en "feat-login", working tree limpio, y "main" con 2 commits nuevos
Cuando el desarrollador rebasa "feat-login"
Entonces "feat-login" queda sobre el último commit de "main", con snapshot previo y Deshacer disponible
  Y al deshacer, "feat-login" vuelve a su commit anterior

**Escenario: Agente presente, rebase bloqueado**

Dado "claude-2" Inactivo en "feat-pagos"
Cuando el desarrollador selecciona "feat-pagos"
Entonces "Rebasar" aparece desactivado con "el agente sigue en este directorio; espera a que termine"

**Escenario: Compartido, manda la sesión más restrictiva**

Dado "feat-pagos" con "claude-1" Terminado y "claude-2" Inactivo
Cuando el desarrollador selecciona "feat-pagos"
Entonces "Rebasar" sigue desactivado por "claude-2"

**Escenario: Cambios sin commitear, rebase bloqueado**

Dado "feat-login" con un archivo sin commitear y su sesión Terminado
Cuando el desarrollador selecciona "feat-login"
Entonces "Rebasar" aparece desactivado con "hay cambios sin commitear"

**Escenario: Con ⚡ contra la base, aviso y confirmación**

Dado ⚡ entre "feat-login" y "main"
Cuando el desarrollador pide rebasar "feat-login"
Entonces la TUI avisa del conflicto previsto y pide confirmación; si la rechaza, nada cambia

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec)._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001.
- **Dev Spec:** pendiente.
