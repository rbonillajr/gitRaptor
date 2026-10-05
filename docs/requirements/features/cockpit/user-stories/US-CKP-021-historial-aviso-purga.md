---
id: US-CKP-021
title: "El desarrollador ve en la TUI el historial de operaciones y el aviso de purga"
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
    - US-CKP-001
    - US-TMC-006
    - US-TMC-012
    - US-TMC-016
tags:
  - cockpit
  - time-machine
  - timeline
  - purga
  - must
---

# US-CKP-021: El desarrollador ve en la TUI el historial de operaciones y el aviso de purga

## Descripción

**Como** desarrollador orquestador, **quiero** ver en la TUI qué operaciones hizo cada actor, deshacer desde ahí y enterarme de qué snapshots se van a purgar, **para** saber qué puedo recuperar sin salir de la vista de la flota.

**Valor**: superficie de la Time Machine (BR-08 a BR-10) dentro del Cockpit; habilita el Deshacer "después de 5 s" de BR-CKP-WF-002.

## Reglas cubiertas

BR-CKP-TIME-004 · BR-CKP-WF-002 (Deshacer desde el historial) · BR-CKP-CONS-003 (actor en el historial) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-001; US-TMC-006 (timeline), US-TMC-012 (Deshacer se detiene ante solape con otro actor), US-TMC-016 (retención y purga).
- **Huecos del motor**: DEP-CKP-5 (timeline en vivo).

## Criterios de Aceptación

**Escenario: El historial muestra las operaciones con su actor**

Dado una integración de "feat-pagos" desde la TUI y un commit de "claude-2" con Git directo
Cuando el desarrollador abre el historial
Entonces aparecen las dos entradas con su hora y su actor, "Tú u otro (sin atribuir)" y "claude-2"

**Escenario: Deshacer desde el historial tras los 5 s**

Dado la integración de "feat-pagos" hecha hace 10 minutos
Cuando el desarrollador la deshace desde el historial
Entonces "main" vuelve al commit anterior a la integración

**Escenario: Deshacer se detiene ante trabajo posterior de otro actor**

Dado la integración de "feat-pagos" y después un commit de "claude-3" sobre "main"
Cuando el desarrollador intenta deshacer la integración
Entonces la TUI muestra el solape con "claude-3" y no deshace nada sin su decisión

**Escenario: Aviso de purga visto**

Dado 3 snapshots de más de 30 días con purga programada
Cuando el desarrollador abre la TUI
Entonces la TUI muestra "3 snapshots se purgarán a partir de mañana a las 10:00"
  Y la Time Machine registra el aviso como visto

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec)._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001.
- **Dev Spec:** pendiente.
