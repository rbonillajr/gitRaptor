---
id: US-CKP-019
title: "El desarrollador entiende por qué Guardrails frena una acción y puede hacer una excepción consciente"
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
    - US-GRD-006
    - US-GRD-007
    - US-GRD-008
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

BR-CKP-AUTH-001 · BR-CKP-AUTH-002 · BR-CKP-WF-001 (la fila con ⛔ sube a la zona de atención) — ver [business-rules.md](../business-rules.md)

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

**Escenario: "Pedir confirmación" sin cola equivale a denegar**

Dado una regla con "pedir confirmación" para el rebase y sin cola publicada
Cuando el desarrollador pide rebasar "feat-login"
Entonces la TUI la presenta como denegada, con la regla, y no crea ninguna petición pendiente

**Escenario: Confirmar en la TUI no cambia quién pide**

Dado la TUI abierta desde el terminal de "claude-1"
Cuando el desarrollador rebasa "feat-pagos", trabajo de "claude-1", y confirma en la TUI
Entonces Guardrails decide con "claude-1" como solicitante y con la capa de los agentes

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec). Mecanismo: ADR-GRD-007; capa `cockpit`: ADR-CKP-002 § 4._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (PolicyBanner).
- **Dev Spec:** pendiente. Ascendencia en Linux y Windows: Pendiente: etapa de validación multiplataforma.
