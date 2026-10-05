---
id: US-CKP-023
title: "El desarrollador aprueba o rechaza desde la TUI las acciones que un agente deja en espera"
type: us
status: blocked
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
    - US-GRD-015
    - US-CKP-019
tags:
  - cockpit
  - guardrails
  - cola
  - bloqueada
---

# US-CKP-023: El desarrollador aprueba o rechaza desde la TUI las acciones que un agente deja en espera

## Descripción

**Como** desarrollador orquestador, **quiero** ver en la TUI las peticiones que los agentes dejan en "pedir confirmación", con su cuenta atrás, y aprobarlas o rechazarlas, **para** que los agentes avancen en lo arriesgado sin perder yo la última palabra.

**Valor**: superficie de la cola de Guardrails (BR-13, Should; Q-GRD-13).

## Reglas cubiertas

BR-CKP-WF-006 · BR-CKP-AUTH-004 · BR-CKP-TIME-003 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Bloqueada** (no se planifica en el MVP del Cockpit hasta que existan las dos piezas):
  1. **Cola publicada por Guardrails** — US-GRD-015, a su vez bloqueada (DEP-CKP-8).
  2. **Factor de autenticación fuera de banda del sistema operativo** (Q-GRD-19) — sin él no se ofrece aprobar. Su decisión de arquitectura está en curso en otra rama.
- **Historias**: US-CKP-019 (hasta entonces, "pedir confirmación" se presenta como denegar).

## Criterios de Aceptación

**Escenario: Peticiones pendientes con cuenta atrás**

Dado una petición de "claude-2" para rebasar "feat-login" por la regla "rebase=ask", creada a las 10:00
Cuando el desarrollador abre la cola a las 10:03
Entonces la petición muestra actor, operación, regla y "2:00" restantes

**Escenario: Aprobar con el factor del sistema**

Dado la petición pendiente y el factor de autenticación del sistema disponible
Cuando el desarrollador la aprueba y supera el factor
Entonces el rebase se ejecuta una vez, con snapshot previo, y la petición queda "aprobada"

**Escenario: Sin factor, solo rechazar**

Dado la petición pendiente y sin factor de autenticación disponible
Cuando el desarrollador mira sus acciones
Entonces solo se ofrece Rechazar

**Escenario: Rechazar sin ventana**

Dado la petición pendiente
Cuando el desarrollador la rechaza
Entonces queda "rechazada" al momento y "feat-login" no cambia

**Escenario: Caducada, sin acciones**

Dado la petición creada a las 10:00
Cuando son las 10:05
Entonces la petición dice "caducada" y no ofrece ninguna acción

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec)._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001.
- **Dev Spec:** pendiente (tras desbloquearse).
