---
id: US-TMC-013
title: "Un agente no puede deshacer trabajo ajeno, aunque lance la CLI desde su propia shell"
type: us
status: implemented
priority: high
created: 2026-10-03
updated: 2026-10-09
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-002, US-GRP-007, US-GRP-009]
covers: [BR-TMC-AUTH-001, D-TMC-6, D-TMC-17, D-TMC-23]
blocked_by: []
tags: [time-machine, permisos, solicitante]
---

# US-TMC-013: Un agente no puede deshacer trabajo ajeno, aunque lance la CLI desde su propia shell

## Descripción

**Como** desarrollador orquestador
**Quiero** que cada petición de undo se atribuya como un evento y que tocar trabajo de otro actor exija mi confirmación interactiva, o se rechace donde esa confirmación no se puede probar
**Para** que un agente no pueda borrar el trabajo de otro usando la propia red de seguridad

**Valor**: GitRaptor no puede probar quién es humano (Q34); este control evita que esa limitación se convierta en un riesgo (R7).

## Reglas cubiertas

BR-TMC-AUTH-001 (solicitante atribuido; confirmación interactiva en macOS y Linux; rechazo en Windows y por MCP) · D-TMC-6, D-TMC-17, D-TMC-23 (actualizada por TQ-14 y TQ-7) — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Un agente deshace su propia operación**

Dado un commit de "claude-1" en "feat-login"
Cuando "claude-1" pide por MCP deshacerlo
Entonces el commit se deshace
  Y el registro del undo indica como solicitante a "claude-1"

**Esquema del escenario: Un agente no deshace la operación de otro**

Dado un commit de "claude-2"
  Y "codex-1" registrado como otro agente
Cuando "<agente>" pide deshacerlo por <canal>
Entonces la petición se rechaza con el motivo
  Y el repo no cambia

Ejemplos:

| agente | canal |
|--------|-------|
| claude-1 | MCP |
| claude-1 | la CLI desde su propia shell |
| codex-1 | MCP |

**Escenario: En macOS y Linux, un solicitante sin atribuir confirma para tocar trabajo ajeno**

Dado un commit de "claude-1"
Cuando un solicitante "sin atribuir" pide deshacerlo con la CLI en macOS o Linux
  Y el desarrollador lo confirma de forma interactiva en ese momento
Entonces el commit se deshace
  Y el registro del undo indica como solicitante "sin atribuir"

**Escenario: Sin confirmación no hay undo**

Dado un commit de "claude-1"
Cuando un solicitante "sin atribuir" pide deshacerlo con la CLI en macOS o Linux sin poder confirmar de forma interactiva
Entonces la petición se rechaza
  Y el repo no cambia

**Escenario: En Windows, un solicitante sin atribuir no puede tocar trabajo ajeno**

Dado un commit de "claude-1"
Cuando un solicitante "sin atribuir" pide deshacerlo con la CLI en Windows
Entonces la petición se rechaza sin pedir confirmación
  Y el solicitante recibe el motivo: en Windows todavía no se puede confirmar trabajo de otro actor
  Y el repo no cambia

**Escenario: Por MCP, un solicitante sin atribuir se rechaza siempre**

Dado una edición "sin atribuir" en "feat-login"
Cuando un solicitante "sin atribuir" pide deshacerla por MCP
Entonces la petición se rechaza con el motivo
  Y el repo no cambia

## Requisitos Técnicos

- Dueña de la regla base de permisos y de la confirmación (ADR-TMC-005 § 2-3): agente X solo lo suyo; "sin atribuir" necesita confirmación para trabajo de un agente.
- La confirmación reutiliza los controles del daemon de ADR-GRP-005 § 6 (identificador no reutilizable, ascendencia, terminal y líder de sesión); el MCP nunca la ofrece.
- "Sin atribuir" por MCP: rechazo incondicional y previo. Agentes registrados: reconocidos por la identidad de proceso guardada al registrarse; depende de que motor-local aplique la nota a ADR-GRP-012/013 (ADR-TMC-005 § 1).
- Toda petición, aceptada o rechazada, queda en el oplog con su motivo (SEC-TMC-03).

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** no se escribió: la historia se implementó con un Implementation Brief del `rust-architect`, aprobado por el coordinador (flujo ligero de AADD). Las decisiones están abajo.

## Dependencias

- **Historias**: US-TMC-002; US-GRP-007 y US-GRP-009 de motor-local.
- **Externas**: F-001-05 Servidor MCP (canal `undo` para agentes). Las restricciones adicionales de Guardrails están en US-TMC-021. Cómo se identifica al solicitante lo decide el Arquitecto (D-TMC-23).
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux salvo la confirmación interactiva, que en el MVP no existe en Windows (D-TMC-23); mensajes en inglés y español.

## Estado de la implementación (2026-10-09)

Implementado en: PR #__PR__.

- **Hecho:** los seis escenarios como tests en repos y perfiles temporales: `crates/core/tests/us_tmc_013.rs`, `crates/core/tests/us_tmc_013_restore.rs`, `crates/core/src/timemachine/confirm/tests.rs`, `apps/cli/tests/us_tmc_013_process.rs` (confirmación real con una pty a través de `script`) y `crates/api/tests/tm_confirmation.rs`. Los escenarios 1 y 2 (un agente sobre lo suyo y sobre lo ajeno) ya los cumplía US-TMC-002; aquí quedan probados por MCP y por la CLI desde la shell del agente. `codex-1` no se reconoce todavía como agente registrado (depende de motor-local, ADR-TMC-005 § 1), así que hoy se rechaza como "sin atribuir" por MCP.
- **Cómo funciona:** `timemachine.undo` y `timemachine.restore` se llaman dos veces en la misma conexión. La primera devuelve `confirmation-required` y un reto de un solo uso: 128 bits, válido 60 s y ligado a la conexión, al proceso y al hash del plan, que calcula el daemon con su propio plan. Solo lo recibe quien pasa la prueba de presencia de consola de los comandos reservados (`requester::confirmation_refusal`, ADR-GRP-005 § 6). La segunda llamada presenta el token: el daemon vuelve a planificar bajo el bloqueo del repo, comprueba otra vez la elegibilidad y lo canjea. La operación queda en el oplog con `confirmed = 1` y el solicitante "sin atribuir". La CLI pregunta solo si hay una terminal. `--json` nunca pregunta, y no existe `--yes`.
- **Decisiones del orquestador (2026-10-08), validadas por Arquitecto y el coordinador:**
  - **D1:** dos llamadas en la misma conexión, sin un método nuevo.
  - **D2:** una sola puerta en el core (`timemachine::confirm`) para undo y restore, justo después de la regla base.
  - **D3:** el mismo `ChallengeBook` que el ejecutor del Cockpit, con un solo reto vivo por conexión.
  - **D4:** el hash es canónico y sale solo del plan del daemon, nunca del cliente.
  - **D5:** un token presentado siempre se consume, y si no casa con el plan da `challenge-invalid`.
  - **D8:** el MCP nunca ofrece el reto.
  - **D9 y D10:** capacidad `timemachine.confirmation` y tipo `TmConfirmData`.
  - **D11:** la primera llamada también queda en el oplog como rechazada.
  - **D12:** no hay `--yes`.
  - **D13:** una costura de test que solo existe en builds de debug.
  - **D14:** al cerrar una conexión se olvida su reto.
- **Windows (decisión A del coordinador, 2026-10-08):** la prueba de terminal también existe en Windows (`authz::TERMINAL_PROOF`), pero BR-TMC-AUTH-001 y ADR-TMC-005 § 3 prohíben confirmar trabajo ajeno ahí en el MVP, por M-01 y M-04. La regla es `confirm::FOREIGN_WORK_CONFIRMABLE = cfg!(unix)`: en Windows la petición se rechaza con `confirmation-unavailable`, sin emitir reto. Habilitarla es una decisión de producto pendiente: hay que enmendar BR-TMC-AUTH-001 y ADR-TMC-005, y antes cerrar M-01.
- **Seguridad:** el token se compara en tiempo constante y nunca aparece en el oplog, en los logs, en `Debug` ni en la salida de la CLI.
- **Fuera de esta ficha:** el ejecutor del Cockpit ofrece en Windows la confirmación de trabajo ajeno, en contra de BR-CKP-AUTH-003 (revisión de seguridad H-01, ya existía). Va en una rama `fix/` aparte y bloquea v0.1.0. La confirmación en la TUI se hará cuando la TUI ofrezca undo o restore.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md), XP-41).
