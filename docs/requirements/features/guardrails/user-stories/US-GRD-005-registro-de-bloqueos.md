---
id: US-GRD-005
title: "El desarrollador cuenta las acciones peligrosas que Guardrails bloqueó en cada repo"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRD-001
    - US-GRP-009
tags:
  - guardrails
  - registro-decisiones
  - kpi
---

# US-GRD-005: El desarrollador cuenta las acciones peligrosas que Guardrails bloqueó en cada repo

## Descripción

**Como** desarrollador orquestador, **quiero** consultar por repo cada operación denegada con su motivo y su actor, **para** medir cuántas acciones peligrosas se evitaron, el KPI del BRD (§ 9).

**Valor**: el valor de Guardrails se puede demostrar con datos de dogfooding.

## Reglas cubiertas

BR-CONS-004 (qué se anota, con todos sus campos; en el perfil, nunca en el repo; Q-GRD-10) · BR-TIME-002 (retención de 90 días) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-001 (primera denegación); US-GRP-009 (motor-local: agente registrado para el actor "agente X").
- **Externas**: ninguna.
- **Transversal**: Windows, macOS y Linux; nada sale de la máquina (NFR-03).

## Criterios de Aceptación

**Escenario: Una denegación queda registrada con todos sus datos**

Dado el repo "demo" protegido y el agente "codex" registrado en el worktree "feat-x"
Cuando "codex" hace force-push de "feat-x" con Git directo y se deniega
Entonces el registro de "demo" tiene una entrada con el momento, el repo, el worktree, la rama, el actor "codex", la operación, la decisión, la regla, el nivel que la causó y la capa
  Y el actor queda como "sin atribuir" cuando lo intenta un proceso que el motor no asigna a ningún agente

**Escenario: Se cuentan las acciones bloqueadas de un periodo**

Dado el repo "demo" con 3 denegaciones esta semana y 1 la semana anterior
Cuando el desarrollador consulta las acciones bloqueadas de "demo" de esta semana
Entonces obtiene 3

**Escenario: Las operaciones permitidas sin regla no se anotan**

Dado el repo "demo" protegido
Cuando un proceso hace 10 commits permitidos
Entonces el registro de "demo" no tiene entradas nuevas

**Escenario: El registro no está en el repo**

Dado el repo "demo" con entradas en su registro
Cuando se preparan todos los cambios del repo para un commit
Entonces no se recoge nada del registro de decisiones

**Escenario: Las entradas caducan a los 90 días**

Dado una entrada del registro de hace 91 días y otra de hace 89 días
Cuando el desarrollador consulta el registro de "demo"
Entonces solo aparece la de hace 89 días

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto).
