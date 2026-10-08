---
id: US-GRD-008
title: "Ningún agente cambia una rama protegida ni toca una ruta prohibida"
type: us
status: implemented
priority: high
created: 2026-10-04
updated: 2026-10-08
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
---

# US-GRD-008: Ningún agente cambia una rama protegida ni toca una ruta prohibida

## Descripción

**Como** desarrollador orquestador, **quiero** declarar ramas protegidas y rutas prohibidas en la configuración del equipo, **para** que la rama de integración solo cambie por una acción mía y los agentes no toquen archivos sensibles.

**Valor**: "que nadie rompa main" (BRD § 4); primer bloque de políticas de Q-GRD-9.

## Reglas cubiertas

BR-VAL-003 (rama protegida y ruta prohibida; force-push y `reset --hard` prohibidos ya los cubre el permiso, US-GRD-007) · BR-CALC-001 (varias reglas incumplidas: se nombran todas) · Q-GRD-35 (actor al que aplican) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-007 (lectura de la configuración del equipo y decisión).
- **Externas**: ninguna. Estuvo bloqueada por P8 (formato de la configuración, motor-local) hasta el 2026-10-04, cuando Rene Bonilla aceptó ADR-GRP-007, que la cierra.
- **Transversal**: Windows, macOS y Linux.

## Criterios de Aceptación

Las ramas protegidas y las rutas prohibidas aplican por defecto solo a los agentes detectados o registrados (Q-GRD-35). Force-push y `reset --hard` sobre la rama protegida los cubre además el permiso (US-GRD-007).

**Esquema del escenario: Una rama protegida no cambia por un agente**

Dado el repo "demo" protegido, con "main" como rama protegida en la configuración del equipo
Cuando un agente detectado hace "<operación>" sobre "main" con Git directo
Entonces la operación no se ejecuta y el motivo nombra la rama protegida "main"

Ejemplos:
| operación |
| commit |
| push |
| borrar la rama |
| crear la rama |

**Escenario: La persona en su terminal sí puede**

Dado el repo "demo" con "main" protegida para agentes
Cuando la persona, sin agente, hace un commit en "main" desde su terminal
Entonces la operación se ejecuta

**Escenario: Con la protección para todos también se deniega a la persona**

Dado el repo "demo" con "main" protegida con `appliesTo: everyone`
Cuando la persona, sin agente, hace un commit en "main" desde su terminal
Entonces el commit no se ejecuta y el motivo nombra la rama protegida "main"

**Escenario: Las ramas no protegidas siguen libres**

Dado el repo "demo" con "main" protegida
Cuando un agente detectado hace commit y push en "feat-x"
Entonces las dos operaciones se ejecutan

**Escenario: Un commit que toca una ruta prohibida se deniega**

Dado el repo "demo" con la ruta prohibida "secrets/"
Cuando un agente detectado hace un commit que modifica "secrets/api.txt"
Entonces el commit no se ejecuta y el motivo nombra la ruta prohibida
  Y los cambios siguen en el working tree, sin perderse

**Escenario: Crear o borrar en una ruta prohibida también se deniega**

Dado el repo "demo" con la ruta prohibida "secrets/"
Cuando un agente detectado hace un commit que crea "secrets/nuevo.txt" o borra "secrets/viejo.txt"
Entonces el commit no se ejecuta

**Escenario: Dos reglas incumplidas se nombran juntas**

Dado el repo "demo" con "main" protegida y la ruta prohibida "secrets/"
Cuando un agente detectado hace commit en "main" de un cambio en "secrets/api.txt"
Entonces el commit no se ejecuta y el motivo nombra las dos reglas

## Requisitos Técnicos

El detalle técnico vive en la [Dev Spec DS-US-GRD-008](../dev-specs/US-GRD-008-ramas-protegidas-rutas-prohibidas.md); el Arquitecto la mantiene.

- Las dos políticas se evalúan con el actor de la operación y el valor `appliesTo` (Q-GRD-35); la Dev Spec fija cómo se obtiene y cómo se degrada.
- Misma decisión y motivo en las dos capas (BR-CONS-002) y en modo degradado solo rigen las reglas `everyone` del suelo.
- Crear, mover o borrar una rama que casa con un patrón protegido cuenta como cambiarla.

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** [DS-US-GRD-008](../dev-specs/US-GRD-008-ramas-protegidas-rutas-prohibidas.md).

## Estado de la implementación (2026-10-08)

Implementado en: PR #<n>. Dev Spec: [DS-US-GRD-008](../dev-specs/US-GRD-008-ramas-protegidas-rutas-prohibidas.md).

Los siete escenarios están cubiertos por `apps/cli/tests/guard_us_grd_008.rs` (verificado en macOS; Linux lo cubre el CI de ubuntu; Windows no tiene canal: **Pendiente: etapa de validación multiplataforma**). Límites declarados en la lista "lo que no se puede impedir" (`policy-actor`, `policy-reach`). Pendiente fuera de esta historia: push solo a tags ([TD-GRD-001](../technical-stories/TD-GRD-001-dispatcher-plantilla-3-pre-push-toda-ref.md)) y confirmación de Rene de Q-GRD-35.
