---
id: US-CKP-019
title: "El desarrollador entiende por qué Guardrails frena una acción y puede hacer una excepción consciente"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-28
feature: cockpit
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
  stories:
    - US-CKP-014
    - US-GRD-006
    - US-GRD-007
    - US-GRD-008
  specs:
    - DS-US-CKP-019
tags:
  - cockpit
  - guardrails
  - excepcion-consciente
  - solicitante
  - must
---

# US-CKP-019: El desarrollador entiende por qué Guardrails frena una acción y puede hacer una excepción consciente

## Descripción

**Como** desarrollador orquestador, **quiero** ver qué regla y qué nivel deniegan una acción del Cockpit y poder saltarla con una excepción consciente y auditada, **para** no quedar bloqueado por mis propias políticas sin abrirles la puerta a los agentes.

**Valor**: BR-07 (Must) gobernado por Guardrails (BR-11/BR-12). El Cockpit no autoriza: presenta la decisión.

## Reglas cubiertas

BR-CKP-AUTH-001 · BR-CKP-AUTH-002 · BR-CKP-WF-001 (la fila con ⛔ sube a la zona de atención) · Q-CKP-52 ("pedir confirmación" sin cola, según quién pide; Q-GRD-46) · Q-CKP-53 (la fila sube en la TUI que pidió la acción) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-014 (flujo de escritura); US-GRD-006 (excepción consciente); US-GRD-007 (permiso "pedir confirmación"); US-GRD-008 (ramas protegidas); US-CKP-002 (zona de atención).
- **Técnicas**: TS-CKP-003 (capa `cockpit`; DEP-CKP-10).
- **Contrato que fija**: el comando reservado lanzado desde la TUI (excepción consciente); lo reutilizan las demás acciones reservadas de la TUI.
- **Riesgo aceptado**: R-CKP-3 (todo merge a una base protegida pasa por excepción consciente; Q-CKP-26).

## Criterios de Aceptación

**Escenario: Acción denegada, con regla y nivel**

Dado "main" protegida en la configuración del equipo
Cuando el desarrollador integra "feat-pagos" en "main"
Entonces la TUI muestra "⛔ main es rama protegida (equipo)" y "main" no cambia
  Y ofrece la excepción consciente
  Y la fila de "feat-pagos" sube a la zona de atención mientras la denegación siga vigente

**Escenario: Excepción consciente con ventana cancelable**

Dado la integración de "feat-pagos" denegada por la regla anterior
Cuando el desarrollador pide la excepción consciente y deja pasar la ventana de 10 s
Entonces la integración sigue el flujo normal con snapshot previo y Deshacer
  Y la excepción queda auditada con regla, operación y solicitante

**Escenario: Cancelar dentro de la ventana**

Dado una excepción consciente anunciada
Cuando el desarrollador la cancela antes de 10 s
Entonces no se ejecuta nada y la auditoría registra la cancelación

**Escenario: TUI lanzada desde el terminal de un agente**

Dado la TUI abierta desde la pestaña donde corre "claude-1"
Cuando el desarrollador mira la cabecera y selecciona "feat-pagos"
Entonces la cabecera dice "Actúas como claude-1"
  Y "Integrar", "Descartar" y "Cancelar" aparecen desactivados con el motivo "una TUI lanzada por un agente actúa como ese agente"
  Y la excepción consciente no se ofrece, y si se pide por el canal se rechaza

**Esquema del escenario: "Pedir confirmación" sin cola depende de quién pide**

Dado una regla con "pedir confirmación" para el rebase y sin cola publicada
  Y la TUI abierta <origen>
Cuando el desarrollador pide rebasar "feat-login"
Entonces <resultado>
  Y no se crea ninguna petición pendiente

Ejemplos:
| origen | resultado |
| desde el terminal de "claude-1" | la TUI la presenta como denegada, con la regla, y "feat-login" no cambia |
| por el desarrollador en su terminal | el rebase sigue el flujo normal con snapshot previo y Deshacer, con el aviso de que la regla pide confirmación humana y se ejecuta porque la hace él |

**Escenario: Confirmar en la TUI no cambia quién pide**

Dado la TUI abierta desde el terminal de "claude-1"
Cuando el desarrollador rebasa "feat-pagos", trabajo de "claude-1", y confirma en la TUI
Entonces Guardrails decide con "claude-1" como solicitante y con la capa de los agentes

## Requisitos Técnicos

Resumen de [DS-US-CKP-019](../dev-specs/US-CKP-019-denegada-excepcion-consciente.md); el detalle vive allí.

- Mecanismo de ADR-GRD-007 (D5, D10, Enmienda Cockpit) y capa `cockpit` de ADR-CKP-002 § 4: la excepción del Cockpit no emite token; va ligada al plan y a su huella.
- Método reservado nuevo `operation.exception {planId}`, fuera del MCP; tras la ventana, la TUI ejecuta el mismo plan con `operation.run` y el `actionId` (capacidad `operation.exception`). Cancelar es `guard.cancel`.
- La ventana es la de las acciones que relajan (`guardrails/pending.rs`, 10 s, un pendiente por repo, compartido con `raptor guard exec` y la desinstalación); el anuncio es el `reserved.audit` en todos los clientes.
- Una sola entrada por plan en el registro: `exception`, `exception-cancelled`, `exception-rejected` o `denial`, con capa `cockpit`. La excepción nunca salta el snapshot previo.
- TUI: PolicyBanner ⛔ con regla y nivel, la fila sube a la zona de atención mientras viva el plan, y un flujo reservado genérico (`tui/reserved.rs`) que reutilizarán las demás acciones reservadas.
- "Actúas como <agente>" sale del solicitante que resuelve el daemon; Integrar, Descartar, Cancelar y la excepción comparten un único texto de motivo.
- "Pedir confirmación" sin cola, por solicitante (Q-CKP-52): la TUI de un agente lo pinta denegado con la regla; la persona sigue el flujo normal con el aviso "pide confirmación humana; se ejecuta porque la haces tú", que llega en `PrepareResult.guardrails_notices`. Ninguno crea petición.
- Pendiente: el escenario 5 de punta a punta depende de US-GRD-016 (D2 de DS-US-GRD-016: los permisos del equipo se aplican a todos los planes del ejecutor); el escenario 6 en la TUI depende de US-MCP-018.

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (PolicyBanner).
- **Dev Spec:** [`dev-specs/US-CKP-019-denegada-excepcion-consciente.md`](../dev-specs/US-CKP-019-denegada-excepcion-consciente.md) — `DS-US-CKP-019` (status: draft). Ascendencia en Linux y Windows: Pendiente: etapa de validación multiplataforma.
