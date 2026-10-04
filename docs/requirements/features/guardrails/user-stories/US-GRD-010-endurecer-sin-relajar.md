---
id: US-GRD-010
title: "El desarrollador endurece las reglas en su máquina sin poder relajar las del equipo"
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
    - US-GRD-008
tags:
  - guardrails
  - configuracion-tres-niveles
  - precedencia
---

# US-GRD-010: El desarrollador endurece las reglas en su máquina sin poder relajar las del equipo

## Descripción

**Como** desarrollador orquestador, **quiero** añadir restricciones en mi perfil o en la configuración local personal del repo, sabiendo que ningún nivel personal puede relajar una regla del equipo, **para** ser más estricto con mis agentes sin debilitar lo que el equipo acordó.

**Valor**: las reglas del equipo son un suelo que nadie baja en su máquina (Q-GRD-14, refina Q23 de motor-local).

## Reglas cubiertas

BR-CONS-001 (los personales endurecen, nunca relajan; local sobre perfil) · BR-VAL-001 (valor en un nivel no admitido: no se tiene en cuenta) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-007 (permisos del equipo), US-GRD-008 (ramas protegidas como lista).
- **Externas**: ninguna. Estuvo bloqueada por P8 (formato de la configuración, motor-local) hasta el 2026-10-04, cuando Rene Bonilla aceptó ADR-GRP-007, que la cierra.
- **Transversal**: Windows, macOS y Linux. El motor no escribe ningún nivel (Q23 de motor-local).

## Criterios de Aceptación

**Esquema del escenario: Precedencia entre equipo, perfil y local personal**

Dado el repo "demo" protegido con "<equipo>" en la configuración del equipo, "<perfil>" en el perfil y "<local>" en la configuración local personal
Cuando se evalúa la regla en "demo"
Entonces el valor efectivo es "<efectivo>"

Ejemplos:
| equipo | perfil | local | efectivo |
| force-push: denegar | sin definir | force-push: permitir | denegar |
| force-push: permitir | force-push: denegar | sin definir | denegar |
| force-push: permitir | force-push: denegar | force-push: permitir | permitir |
| rebase: pedir confirmación | sin definir | rebase: permitir | pedir confirmación |
| límite de diff 400 | sin definir | límite de diff 200 | 200 |
| límite de diff 400 | sin definir | límite de diff 1000 | 400 |
| ramas protegidas: main | ramas protegidas: release | sin definir | main y release |

**Escenario: Un endurecimiento local no afecta a otro clon**

Dado el repo "demo" con push permitido por el equipo y denegado en la configuración local personal de esta máquina
Cuando un proceso hace push desde otro clon de "demo" sin ese ajuste local
Entonces el push se ejecuta en ese clon

**Escenario: La rama base en un nivel personal no se tiene en cuenta**

Dado el repo "demo" sin rama base en la configuración del equipo y con "develop" en la configuración local personal
Cuando se evalúa la protección de la rama base en "demo"
Entonces la rama base protegida es "main"

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto).
