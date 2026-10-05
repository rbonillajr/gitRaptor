---
id: US-CKP-022
title: "El desarrollador ve crecer la rama de cada agente sobre la base"
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
    - US-CKP-005
    - US-CKP-012
tags:
  - cockpit
  - grafo
  - must
---

# US-CKP-022: El desarrollador ve crecer la rama de cada agente sobre la base

## Descripción

**Como** desarrollador orquestador, **quiero** un carril por agente que muestre cómo crece su rama desde la base confirmada, **para** ver de un vistazo cuánto se aleja cada agente y cuándo conviene integrarlo.

**Valor**: BR-05 (Must). Último en el orden de entrega (Q-CKP-24).

## Reglas cubiertas

BR-CKP-CALC-004 · BR-CKP-EDGE-001 (sin carriles) · BR-CKP-EDGE-005 (el grafo colapsa primero) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-001; US-CKP-005 (reparto del espacio); US-CKP-012 (fija las consultas bajo demanda, N8).
- **Huecos del motor**: DEP-CKP-2 (consulta de commits base..rama). Actor por commit solo si el motor publica commit→evento (opcional).

## Criterios de Aceptación

**Escenario: Un carril por worktree, desde su merge-base**

Dado "main" confirmada y "feat-pagos" de "claude-1" y "feat-login" de "claude-2" con commits propios
Cuando el desarrollador abre el grafo
Entonces hay un carril por rama desde su merge-base con "main", cada uno con el nombre y el color de su agente

**Escenario: Ventana acotada**

Dado "feat-pagos" con 72 commits desde su merge-base
Cuando el desarrollador mira su carril
Entonces se ven los 50 más recientes y "22 más" colapsados

**Escenario: Sin relación commit→evento, "sin atribuir"**

Dado que el motor no publica qué agente hizo cada commit
Cuando el desarrollador mira un commit del carril
Entonces su actor dice "sin atribuir" y el carril conserva el color de la sesión del worktree

**Escenario: Base que no existe**

Dado "develop" como base y sin esa rama en el repo
Cuando el desarrollador abre el grafo
Entonces no hay carriles y la vista dice "Rama base develop no encontrada: no calculable"

**Escenario: El grafo cede espacio primero**

Dado una terminal de 80×24 con lista, alertas y grafo
Cuando no caben todos
Entonces el grafo se reduce antes que la lista y las alertas

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec)._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (paleta por carril).
- **Dev Spec:** pendiente. La ventana de 50 commits es supuesto (S-CKP-2); la afina diseño.
