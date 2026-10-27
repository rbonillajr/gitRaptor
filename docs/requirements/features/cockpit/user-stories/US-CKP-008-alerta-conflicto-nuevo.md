---
id: US-CKP-008
title: "El desarrollador se entera en la TUI cuando aparece un conflicto previsto nuevo"
type: us
status: draft
priority: medium
created: 2026-10-04
updated: 2026-10-27
feature: cockpit
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
  stories:
    - US-CKP-006
  specs:
    - DS-US-CKP-008
tags:
  - cockpit
  - prediccion-conflictos
  - alertas
  - must
---

# US-CKP-008: El desarrollador se entera en la TUI cuando aparece un conflicto previsto nuevo

## Descripción

**Como** desarrollador orquestador, **quiero** que la TUI me avise en el momento en que dos agentes empiezan a chocar, **para** intervenir mientras el contexto de cada agente sigue fresco.

**Valor**: BR-06 (Must). Mitiga R-CKP-4 (el usuario no mira la lista a tiempo).

## Reglas cubiertas

BR-CKP-WF-007 · BR-CKP-WF-001 (la fila con ⚡ sube a la zona de atención) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-006; US-CKP-002 (orden por atención).

## Criterios de Aceptación

**Escenario: Un ⚡ nuevo genera una alerta**

Dado "claude-1" y "claude-2" sin conflicto previsto entre ellos
Cuando "claude-2" commitea un cambio que choca con "claude-1" en "src/api.rs"
Entonces la TUI registra una alerta "⚡ Conflicto previsto: claude-1 ↔ claude-2 en src/api.rs" y un aviso efímero con el mismo texto
  Y las filas de los dos worktrees suben a la zona de atención

**Escenario: Un ⚡ que ya existía no repite la alerta**

Dado el ⚡ "claude-1 ↔ claude-2" ya alertado
Cuando la predicción se recalcula y el conflicto sigue
Entonces no se genera otra alerta para ese par

**Escenario: Sin notificación del sistema ni campana**

Dado la TUI en segundo plano
Cuando aparece un ⚡ nuevo
Entonces no se envía ninguna notificación del sistema operativo ni se emite la campana de la terminal

## Requisitos Técnicos

Resumen de la [Dev Spec DS-US-CKP-008](../dev-specs/US-CKP-008-alerta-conflicto-nuevo.md):

- Un ⚡ nuevo es un `pair_id` que pasa a `level: conflict` con `state: current` y no está en el conjunto de alertados de la TUI, que vive en memoria (A5). Lo saca un `current` sin ⚡ o el `removed`; `calculating` y `pending` no lo mueven ([T002](../dev-specs/US-CKP-008-alerta-conflicto-nuevo.md#t002--crear-el-conjunto-de-alertados-y-su-regla)).
- Al abrir la TUI, los ⚡ existentes se siembran sin aviso; tras un `scope.resync`, incluido `daemon-restarted`, solo avisan los que no estaban.
- La alerta pone la marca `new` en el panel ConflictAlert y lanza un toast de 5 s con el texto de P5, en en/es y con fallback ASCII `<->`. La pila de toasts y su caducidad se crean en `apps/cli/src/tui/toasts.rs` ([T003](../dev-specs/US-CKP-008-alerta-conflicto-nuevo.md#t003--crear-la-pila-de-toasts-y-cablearla-en-model-y-update), [T004](../dev-specs/US-CKP-008-alerta-conflicto-nuevo.md#t004--pintar-el-toast-y-la-marca-new-en-enes-y-ascii)).
- Las filas suben a la zona de atención por `AgentRowModel.conflict`, que llena DS-US-CKP-006, con el orden de DS-US-CKP-002.
- Sin BEL ni notificación del SO: un test de bytes de la salida y los tests estáticos de `apps/cli/tests/tui_no_os_alert.rs` ([T001](../dev-specs/US-CKP-008-alerta-conflicto-nuevo.md#t001--escribir-en-rojo-las-pruebas-estáticas-de-sin-bel-ni-so)).
- Bloqueada por T009 (PR-B de TS-CKP-001) y por DS-US-CKP-006.

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (ConflictAlert y aviso efímero).
- **Dev Spec:** [DS-US-CKP-008](../dev-specs/US-CKP-008-alerta-conflicto-nuevo.md).
