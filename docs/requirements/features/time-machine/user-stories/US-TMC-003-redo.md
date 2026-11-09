---
id: US-TMC-003
title: "El desarrollador rehace lo que deshizo por error"
type: us
status: implemented
priority: medium
created: 2026-10-03
updated: 2026-11-09
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-002, US-TMC-012, US-TMC-013]
  specs: [DS-US-TMC-003]
covers: [BR-TMC-WF-001, BR-TMC-VAL-001, BR-TMC-CONS-005, BR-TMC-AUTH-001, D-TMC-13, D-TMC-23, D-TMC-25, D-TMC-26, D-TMC-27]
blocked_by: []
tags: [time-machine, redo]
---

# US-TMC-003: El desarrollador rehace lo que deshizo por error

## Descripción

**Como** desarrollador orquestador
**Quiero** revertir mi último undo con `raptor redo`
**Para** que equivocarme al deshacer no me cueste trabajo

**Valor**: deshacer deja de ser una decisión arriesgada.

## Reglas cubiertas

BR-TMC-WF-001 (redo; S5 aceptado: con cambios intermedios aplica el solape; una operación nueva invalida el redo, TQ-9; redos seguidos rehacen undos seguidos, D-TMC-26) · BR-TMC-VAL-001 · BR-TMC-CONS-005 (solape; en el MVP no se continúa, D-TMC-27) · BR-TMC-AUTH-001 (solicitante, D-TMC-23) — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Rehacer el último undo**

Dado que en "feat-login" un solicitante que queda sin atribuir deshizo una operación suya que había borrado "b.rs"
Cuando ese mismo solicitante pide rehacer desde "feat-login"
Entonces "feat-login" vuelve al estado posterior a esa operación
  Y el contenido de "b.rs" sigue recuperable en el historial

**Escenario: El redo queda protegido por su propio punto previo**

Dado un redo ejecutado en "feat-login"
Cuando se consulta el historial de la Time Machine
Entonces existe un punto recuperable con el estado de "feat-login" justo antes del redo

**Escenario: No hay undo que rehacer**

Dado que "feat-login" no tiene ningún undo pendiente de rehacer
Cuando un solicitante sin atribuir pide rehacer
Entonces el repo no cambia
  Y el solicitante recibe el aviso de que no hay nada que rehacer

**Escenario: Una operación nueva invalida el redo**

Dado que en "feat-login" un solicitante sin atribuir deshizo una operación
  Y después ese solicitante hizo un commit nuevo en "feat-login"
Cuando pide rehacer desde "feat-login"
Entonces el repo no cambia
  Y el solicitante recibe el motivo: una operación posterior invalidó el redo

**Escenario: Otro actor cambió los mismos archivos después del undo**

Dado que "claude-1" deshizo en "feat-login" una operación suya que había tocado "b.rs"
  Y después un cambio sin atribuir modificó "b.rs"
Cuando "claude-1" pide rehacer desde "feat-login"
Entonces el redo se detiene sin cambiar el repo
  Y "claude-1" recibe el solape con el cambio sin atribuir de "b.rs"

**Escenario: Un agente no rehace trabajo de otro actor**

Dado que un solicitante sin atribuir deshizo un commit de "claude-2" en "feat-login"
Cuando "claude-1" pide rehacer ese undo
Entonces la petición se rechaza con el motivo
  Y el repo no cambia

## Requisitos Técnicos

- Redo disponible si desde el último undo del ámbito no hubo una operación nueva; redos seguidos rehacen undos seguidos (D-TMC-26). Restaura el snapshot previo de ese undo (ADR-TMC-003 § 4).
- Aplica la detección de solape de US-TMC-012 sobre lo cambiado desde el undo (S5) y los permisos de US-TMC-013.
- Semántica de undos y redos seguidos: pila por worktree; una operación nueva en el ámbito invalida el redo (ADR-TMC-003 § 4).

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** [DS-US-TMC-003](../dev-specs/US-TMC-003-redo.md)

## Estado de la implementación (2026-11-09)

Implementado en: PR #NNN.

- **Hecho:** `raptor redo [--json]` (CLI, en/es) y `timemachine.redo` sin `operation_id` en el canal del daemon. Rehace el último undo pendiente del worktree del cwd (redos seguidos rehacen undos seguidos, D-TMC-26) como operación protegida con su propio punto previo y su ancla. Sin undo pendiente responde `nothing-to-redo`; si una operación posterior lo invalidó, `redo-invalidated`; el repo no cambia. Permisos sobre el solicitante del undo y los dueños de lo que deshizo, con la confirmación interactiva de ADR-TMC-005 § 3. Solape de US-TMC-012: se detiene con `overlap` y conserva (`kept`) lo que otro actor cambió después. Se rechaza entero si una parte no se puede rehacer. El redo de un undo por agente (US-TMC-011) rehace los mismos ids y quita la barrera `selective-undo-below`. `operation_id` sigue sin implementar; no se ofrece por MCP.
- **Tests:** unitarios en `crates/api/src/timemachine_redo_tests.rs`, `crates/core/src/timemachine/oplog/stack_redo_tests.rs`, `confirm/redo_tests.rs`, `redo_tests.rs`, `redo_tamper_tests.rs` y `apps/cli/src/undo/redo_tests.rs`; integración con daemon y Git reales en `crates/core/tests/us_tmc_003.rs` y `us_tmc_003_agents.rs` (agentes `raptor-fake-agent`, independientes de la sesión de Claude Code); los pendientes de redo de US-TMC-011 en `crates/core/tests/us_tmc_011.rs`; e2e en `apps/cli/tests/redo_process.rs` y el paso de redo de la demo M2 (`apps/cli/tests/m2_demo_undo_agent.rs`).
- **Decisiones de la implementación:** ver [DS-US-TMC-003, Enmienda 2026-11-09](../dev-specs/US-TMC-003-redo.md).
- **Pendiente:**
  - Linux y Windows (XP-70, etapa de validación multiplataforma).
  - La demo M2 depende de TD-GRP-005 (atribución bajo carga).
  - Sin certificar: modo rápido (sin línea base de suite completa).

## Dependencias

- **Historias**: US-TMC-002, US-TMC-012, US-TMC-013.
- **Externas**: ninguna.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
