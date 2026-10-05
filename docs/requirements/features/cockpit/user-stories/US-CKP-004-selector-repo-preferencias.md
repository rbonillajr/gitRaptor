---
id: US-CKP-004
title: "El desarrollador cambia de repo y la TUI recuerda cómo la dejó"
type: us
status: draft
priority: low
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
    - US-CKP-002
tags:
  - cockpit
  - selector-de-repo
  - preferencias
  - should
---

# US-CKP-004: El desarrollador cambia de repo y la TUI recuerda cómo la dejó

## Descripción

**Como** desarrollador orquestador con varios repos observados, **quiero** que la TUI abra el repo en el que estoy, me deje cambiar a otro y recuerde filtros y panel, **para** no reconfigurar la vista cada vez que la abro.

**Valor**: comodidad de uso diario (P2). No bloquea BR-04: sin esta historia la TUI abre en el repo del directorio actual.

## Reglas cubiertas

BR-CKP-CONS-006 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-001; US-CKP-002 (filtro "ver terminadas").
- **Huecos del motor**: DEP-CKP-11 (el daemon guarda las preferencias en el perfil; la TUI no lo abre).

## Criterios de Aceptación

**Escenario: Arranca en el repo del directorio actual**

Dado los repos "shop" y "api" observados
Cuando el desarrollador abre la TUI dentro de "api"
Entonces la vista muestra "api"

**Escenario: Fuera de un repo observado, arranca en el último usado**

Dado que la última vista usada fue "shop"
Cuando el desarrollador abre la TUI desde su carpeta personal
Entonces la vista muestra "shop"

**Escenario: El selector cambia entre repos observados**

Dado la TUI en "shop"
Cuando el desarrollador elige "api" en el selector
Entonces la vista muestra los worktrees de "api" y solo los de "api"

**Escenario: Las preferencias sobreviven al cierre y no tocan el repo**

Dado el filtro "ver terminadas" activo en "shop"
Cuando el desarrollador cierra la TUI y la vuelve a abrir
Entonces el filtro sigue activo
  Y ningún archivo del repo "shop" cambió

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec)._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001.
- **Dev Spec:** pendiente.
