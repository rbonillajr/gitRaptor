---
id: US-GRD-007
title: "El equipo decide qué operaciones de Git se permiten, se deniegan o piden confirmación"
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
    - US-GRD-004
    - US-GRP-013
tags:
  - guardrails
  - permisos
  - configuracion-equipo
  - bloqueada
---

# US-GRD-007: El equipo decide qué operaciones de Git se permiten, se deniegan o piden confirmación

## Descripción

**Como** desarrollador orquestador, **quiero** fijar en la configuración del equipo el permiso de cada operación gobernada (permitir, pedir confirmación o denegar), **para** que los agentes de todo el equipo trabajen con las mismas reglas en ese repo.

**Valor**: las reglas del repo viajan con él y aplican igual en cada clon (BRD BR-11).

## Reglas cubiertas

BR-VAL-002 (catálogo de operaciones y sus tres permisos; "pedir confirmación" como denegar mientras no exista la cola, S-GRD-9) · BR-CALC-001 (decisión y motivo con su nivel) · BR-VAL-001 (el permiso se define en el nivel de equipo) · BR-EDGE-001 (el equipo puede desactivar el mínimo seguro) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-001 (contrato de decisión y capa de hooks); US-GRD-004 (lista de operaciones que la capa de hooks puede interceptar); US-GRP-013 (motor-local), dueña de la lectura de la configuración en tres niveles, que esta historia reutiliza.
- **Externas**: **bloqueada** por el ADR de formato de la configuración, P8 (motor-local), y por US-GRP-013. Por Q-GRD-17, un cambio en la configuración del equipo se aplica al commitearlo en el worktree de la operación, no al editarlo.
- **Transversal**: Windows, macOS y Linux. Las operaciones que la lista de US-GRD-004 declare no interceptables con Git directo (p. ej. `reset --hard`) se verifican por la capa MCP en US-GRD-016.

## Criterios de Aceptación

**Esquema del escenario: El permiso "denegar" del equipo impide cada operación interceptable**

Dado el repo "demo" protegido, cuya configuración del equipo fija "<operación>" en "denegar"
Cuando un proceso hace "<operación>" con Git directo
Entonces la operación no se ejecuta
  Y el motivo nombra el permiso y el nivel "equipo"

Ejemplos:
| operación |
| commit |
| push |
| force-push |
| borrar rama |
| rebase |
| merge |
| crear worktree |
| borrar worktree |

> Las filas de operaciones que la lista de US-GRD-004 declare no interceptables con Git directo no se ejecutan aquí: pasan a US-GRD-016.

**Escenario: El permiso "permitir" deja pasar la operación**

Dado el repo "demo" protegido con push en "permitir" y ninguna política aplicable
Cuando un proceso hace push de "feat-x" con Git directo
Entonces el push se ejecuta

**Escenario: "Pedir confirmación" se deniega mientras no exista la cola**

Dado el repo "demo" con borrar rama en "pedir confirmación" y sin el modo de confirmación disponible
Cuando un proceso borra la rama "feat-x"
Entonces la operación no se ejecuta y el motivo indica que requiere confirmación humana

**Escenario: El equipo desactiva el mínimo seguro**

Dado el repo "demo" cuya configuración del equipo desactiva el conjunto mínimo y permite force-push
Cuando un proceso hace force-push de "feat-x"
Entonces la operación se ejecuta

**Escenario: Un cambio en la configuración del equipo se aplica al commitearlo, no al editarlo**

Dado el repo "demo" con push en "permitir" en la configuración del equipo commiteada
Cuando el desarrollador cambia push a "denegar" en el worktree "feat-x" sin commitear
Entonces el siguiente push desde "feat-x" se ejecuta
  Y tras commitear ese cambio en "feat-x", el siguiente push desde "feat-x" se deniega sin reinstalar la protección

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto). Reutiliza la lectura de los tres niveles de US-GRP-013 (motor-local).
