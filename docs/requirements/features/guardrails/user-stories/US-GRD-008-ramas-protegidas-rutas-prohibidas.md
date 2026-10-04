---
id: US-GRD-008
title: "Ningún agente cambia una rama protegida ni toca una ruta prohibida"
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
    - US-GRD-007
tags:
  - guardrails
  - politicas
  - ramas-protegidas
  - rutas-prohibidas
  - bloqueada
---

# US-GRD-008: Ningún agente cambia una rama protegida ni toca una ruta prohibida

## Descripción

**Como** desarrollador orquestador, **quiero** declarar ramas protegidas y rutas prohibidas en la configuración del equipo, **para** que la rama de integración solo cambie por una acción mía y los agentes no toquen archivos sensibles.

**Valor**: "que nadie rompa main" (BRD § 4); primer bloque de políticas de Q-GRD-9.

## Reglas cubiertas

BR-VAL-003 (rama protegida y ruta prohibida; force-push y `reset --hard` prohibidos ya los cubre el permiso, US-GRD-007) · BR-CALC-001 (varias reglas incumplidas: se nombran todas) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-007 (lectura de la configuración del equipo y decisión).
- **Externas**: **bloqueada** por el ADR de formato P8 (motor-local).
- **Transversal**: Windows, macOS y Linux.

## Criterios de Aceptación

**Esquema del escenario: Una rama protegida no cambia por un agente**

Dado el repo "demo" protegido, con "main" como rama protegida en la configuración del equipo
Cuando un proceso hace "<operación>" sobre "main" con Git directo
Entonces la operación no se ejecuta y el motivo nombra la rama protegida "main"

Ejemplos:
| operación |
| commit |
| push |
| borrar la rama |

**Escenario: Las ramas no protegidas siguen libres**

Dado el repo "demo" con "main" protegida
Cuando un proceso hace commit y push en "feat-x"
Entonces las dos operaciones se ejecutan

**Escenario: Un commit que toca una ruta prohibida se deniega**

Dado el repo "demo" con la ruta prohibida "secrets/"
Cuando un proceso hace un commit que modifica "secrets/api.txt"
Entonces el commit no se ejecuta y el motivo nombra la ruta prohibida
  Y los cambios siguen en el working tree, sin perderse

**Escenario: Crear o borrar en una ruta prohibida también se deniega**

Dado el repo "demo" con la ruta prohibida "secrets/"
Cuando un proceso hace un commit que crea "secrets/nuevo.txt" o borra "secrets/viejo.txt"
Entonces el commit no se ejecuta

**Escenario: Dos reglas incumplidas se nombran juntas**

Dado el repo "demo" con "main" protegida y la ruta prohibida "secrets/"
Cuando un proceso hace commit en "main" de un cambio en "secrets/api.txt"
Entonces el commit no se ejecuta y el motivo nombra las dos reglas

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto).
