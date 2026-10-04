---
id: US-GRD-009
title: "Los agentes entregan commits pequeños y con el formato que exige el equipo"
type: us
status: draft
priority: medium
created: 2026-10-04
updated: 2026-10-04
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRD-008
tags:
  - guardrails
  - politicas
  - tamano-diff
  - formato-commit
  - bloqueada
---

# US-GRD-009: Los agentes entregan commits pequeños y con el formato que exige el equipo

## Descripción

**Como** desarrollador orquestador, **quiero** fijar un límite de tamaño de diff y un formato de commit en la configuración del equipo, **para** revisar el trabajo de los agentes en piezas pequeñas y con un historial legible.

**Valor**: diffs revisables (BRD P5); segundo bloque de políticas de Q-GRD-9.

## Reglas cubiertas

BR-VAL-003 (límite de diff en líneas cambiadas por commit, S-GRD-7; formato de commit, Conventional Commits como mínimo) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-008 (va después del primer bloque de políticas, Q-GRD-9).
- **Externas**: **bloqueada** por el ADR de formato P8 (motor-local).
- **Transversal**: Windows, macOS y Linux.

## Criterios de Aceptación

**Esquema del escenario: El límite de diff decide el commit**

Dado el repo "demo" con un límite de diff de 400 líneas en la configuración del equipo
Cuando un proceso hace un commit de "<líneas>" líneas cambiadas
Entonces el commit "<resultado>"

Ejemplos:
| líneas | resultado |
| 400 | se ejecuta |
| 1200 | no se ejecuta; el motivo da el tamaño y el límite |

**Escenario: Un mensaje fuera de formato se deniega con un ejemplo válido**

Dado el repo "demo" que exige Conventional Commits
Cuando un proceso hace un commit con el mensaje "arreglos varios"
Entonces el commit no se ejecuta
  Y el motivo nombra el formato exigido e incluye un ejemplo válido

**Escenario: Un mensaje con formato se acepta**

Dado el repo "demo" que exige Conventional Commits
Cuando un proceso hace un commit con el mensaje "fix: handle empty branch name"
Entonces el commit se ejecuta

**Escenario: Sin límite ni formato no se exige nada**

Dado el repo "demo" sin límite de diff ni formato de commit en ningún nivel
Cuando un proceso hace un commit de 5.000 líneas con el mensaje "wip"
Entonces el commit se ejecuta

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto).
