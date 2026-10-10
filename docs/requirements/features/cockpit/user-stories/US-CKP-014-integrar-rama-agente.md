---
id: US-CKP-014
title: "El desarrollador integra la rama de un agente con una tecla y puede deshacerlo"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-09
feature: cockpit
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
  stories:
    - US-CKP-001
    - US-TMC-001
    - US-TMC-002
    - US-GRP-016
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

**Valor**: BR-07 (Must); KPI "−30 % de tiempo de integración". Esqueleto de toda acción de escritura del Cockpit: fija el flujo preparar → confirmar el plan → ejecutar → Deshacer que reutilizan 015, 017, 018 y 024.

## Reglas cubiertas

BR-CKP-WF-002 · BR-CKP-ELIG-002 · BR-CKP-ELIG-001 (columna merge) · BR-CKP-CONS-002 · BR-CKP-AUTH-003 (confirmación del plan en el camino feliz) · BR-CKP-WF-005 (merge desactivado) — ver [business-rules.md](../business-rules.md). Los casos límite del merge están en US-CKP-024.

## Dependencias

- **Historias**: US-CKP-001; US-TMC-001 (snapshot previo) y US-TMC-002 (Deshacer); US-GRP-016 (base pendiente publicada); US-TMC-019 (el Deshacer o la restauración desde la TUI deben entregar los avisos pendientes de `timemachine.notices` del worktree; deuda anotada el 2026-10-09).
- **Técnicas**: TS-CKP-002 (catálogo y ejecutor), TS-CKP-003 (capa `cockpit` de Guardrails), TS-TMC-004 (operación protegida y reto ligado al plan).
- **Contrato que fija**: el Deshacer desde la TUI (lo reutiliza US-CKP-021).

## Criterios de Aceptación

**Escenario: Integrar el trabajo de un agente, confirmando el plan, y deshacer**

Dado "main" confirmada y sacada en el worktree principal, limpio y sin sesión, y "feat-pagos" con commits de "claude-1", su sesión Terminado
Cuando el desarrollador pide integrar "feat-pagos"
Entonces la TUI muestra el plan "integrar feat-pagos (3 commits de claude-1) en main" y pide confirmarlo
  Y tras confirmarlo se toma un snapshot previo, "feat-pagos" queda integrada en "main" y la TUI ofrece Deshacer
  Y al deshacer, "main" vuelve exactamente al commit anterior

**Escenario: Agente activo, se integra hasta el commit visto**

Dado "claude-1" Activo en "feat-pagos" con HEAD "a1b2c3"
Cuando el desarrollador pide integrar "feat-pagos"
Entonces el mismo aviso reúne "el agente sigue activo; se integra hasta el commit a1b2c3" y el plan
  Y tras confirmarlo "main" recibe los commits hasta "a1b2c3" y ninguno posterior

**Escenario: Destino con cambios, acción desactivada con motivo**

Dado el worktree principal con un archivo modificado sin commitear
Cuando el desarrollador selecciona "feat-pagos"
Entonces "Integrar" aparece desactivado con el motivo y la acción que lo desbloquea

**Escenario: Base pendiente, acción desactivada**

Dado la rama base pendiente de confirmar
Cuando el desarrollador selecciona "feat-pagos"
Entonces "Integrar" aparece desactivado con "confirma la rama base" y cómo confirmarla

**Escenario: Sin snapshot no hay merge**

Dado que el snapshot previo falla (disco lleno)
Cuando el desarrollador integra "feat-pagos"
Entonces "main" no cambia y la TUI informa "no se pudo tomar el snapshot previo; no se integró nada"

**Escenario: Nunca se contacta con el remoto**

Dado un remoto "origin" configurado
Cuando el desarrollador integra "feat-pagos"
Entonces las refs de "origin" no cambian y la TUI no ofrece push

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec). Catálogo y ejecutor: ADR-CKP-002 § 2-3._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (ConfirmPrompt con default No, aviso con Deshacer); ADR-GRP-004 § 3.
- **Dev Spec:** pendiente.
