---
id: US-CKP-014
title: "El desarrollador integra la rama de un agente con una tecla y puede deshacerlo"
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
    - US-CKP-001
    - US-CKP-006
    - US-TMC-001
    - US-TMC-002
tags:
  - cockpit
  - acciones-por-agente
  - merge
  - operacion-protegida
  - esqueleto-escritura
  - must
---

# US-CKP-014: El desarrollador integra la rama de un agente con una tecla y puede deshacerlo

## Descripción

**Como** desarrollador orquestador, **quiero** integrar en local la rama de un agente en la base confirmada desde su fila, con snapshot previo y Deshacer, **para** integrar rápido sin miedo a perder trabajo.

**Valor**: BR-07 (Must); KPI "−30 % de tiempo de integración". Esqueleto de toda acción de escritura del Cockpit.

## Reglas cubiertas

BR-CKP-WF-002 · BR-CKP-ELIG-002 · BR-CKP-ELIG-001 (columna merge) · BR-CKP-CONS-002 · BR-CKP-CONS-004 · BR-CKP-EDGE-009 · BR-CKP-WF-005 (merge desactivado) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-001; US-TMC-001 (snapshot previo) y US-TMC-002 (Deshacer); US-CKP-006 para el aviso por ⚡ (sin ella, ese escenario se verifica después).
- **Técnicas**: TS-CKP-002 (catálogo y ejecutor), TS-CKP-003 (capa `cockpit` de Guardrails), TS-TMC-004 (operación protegida).

## Criterios de Aceptación

**Escenario: Integrar y deshacer**

Dado "main" confirmada y sacada en el worktree principal, limpio y sin sesión, y "feat-pagos" con "claude-1" Terminado
Cuando el desarrollador integra "feat-pagos"
Entonces se toma un snapshot previo, "feat-pagos" queda integrada en "main" y la TUI ofrece Deshacer
  Y al deshacer, "main" vuelve exactamente al commit anterior

**Escenario: Agente activo, se integra hasta el commit visto**

Dado "claude-1" Activo en "feat-pagos" con HEAD "a1b2c3"
Cuando el desarrollador integra "feat-pagos" y confirma "se integra hasta el commit a1b2c3"
Entonces "main" recibe los commits hasta "a1b2c3" y ninguno posterior

**Escenario: Con ⚡ en el par, aviso y confirmación**

Dado ⚡ entre "feat-pagos" y "main"
Cuando el desarrollador pide integrar "feat-pagos"
Entonces la TUI avisa del conflicto previsto y pide confirmación antes de ejecutar; si la rechaza, nada cambia

**Escenario: Destino con cambios, acción desactivada con motivo**

Dado el worktree principal con un archivo modificado sin commitear
Cuando el desarrollador selecciona "feat-pagos"
Entonces "Integrar" aparece desactivado con el motivo y la acción que lo desbloquea

**Escenario: Base pendiente, acción desactivada**

Dado la rama base pendiente de confirmar
Cuando el desarrollador selecciona "feat-pagos"
Entonces "Integrar" aparece desactivado con "confirma la rama base" y la acción para confirmarla

**Escenario: Sin snapshot no hay merge**

Dado que el snapshot previo falla (disco lleno)
Cuando el desarrollador integra "feat-pagos"
Entonces "main" no cambia y la TUI informa "no se pudo tomar el snapshot previo; no se integró nada"

**Escenario: Nunca se contacta con el remoto**

Dado un remoto "origin" configurado
Cuando el desarrollador integra "feat-pagos"
Entonces las refs de "origin" no cambian y la TUI no ofrece push

**Escenario: Dos TUIs a la vez, una sola ejecución**

Dado dos TUIs abiertas en "shop"
Cuando las dos piden integrar "feat-pagos" al mismo tiempo
Entonces la primera se ejecuta y la segunda recibe "el estado cambió" sin modificar nada
  Y las dos TUIs muestran la integración

**Escenario: Ningún worktree tiene la base sacada**

Dado ningún worktree con "main" sacada y "feat-pagos" no integrable con fast-forward
Cuando el desarrollador pide integrar "feat-pagos"
Entonces se rechaza con "no hay ningún worktree con main sacada" y la acción que lo resuelve

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec). Catálogo y ejecutor: ADR-CKP-002._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (ConfirmPrompt con default No, aviso con Deshacer); ADR-GRP-004 § 3.
- **Dev Spec:** pendiente.
