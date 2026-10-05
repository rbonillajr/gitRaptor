---
id: US-CKP-008
title: "El desarrollador se entera en la TUI cuando aparece un conflicto previsto nuevo"
type: us
status: draft
priority: medium
created: 2026-10-04
updated: 2026-10-04
feature: cockpit
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
  stories:
    - US-CKP-006
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

_Pendiente — lo completa el Arquitecto (Dev Spec)._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (ConflictAlert y aviso efímero).
- **Dev Spec:** pendiente.
