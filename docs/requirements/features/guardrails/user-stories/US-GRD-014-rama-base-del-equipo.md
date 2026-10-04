---
id: US-GRD-014
title: "El equipo fija la rama base del repo y Guardrails la protege"
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
    - US-GRD-007
    - US-GRP-013
tags:
  - guardrails
  - rama-base
  - configuracion-equipo
  - bloqueada
---

# US-GRD-014: El equipo fija la rama base del repo y Guardrails la protege

## Descripción

**Como** desarrollador orquestador, **quiero** definir la rama base de un repo en la configuración del equipo, el único nivel que la admite, **para** que Guardrails proteja esa rama y el motor mida el ahead/behind contra ella, igual en todos los clones.

**Valor**: la rama de integración que fija el equipo queda protegida igual en todos los clones.

## Reglas cubiertas

BR-CONS-003 (rama base solo del nivel de equipo; `main` por defecto) · BR-EDGE-001 (el mínimo seguro protege la rama base efectiva) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-007 (permisos del equipo, que reutilizan la lectura de los tres niveles de US-GRP-013, motor-local). No depende del comando de edición (US-GRD-013): el valor se puede escribir a mano. La coherencia con la rama base que lee el motor (US-GRP-016, motor-local) es una prueba de integración posterior, anotada en el índice.
- **Externas**: **bloqueada** por el ADR de formato P8 (motor-local).
- **Transversal**: Windows, macOS y Linux.

## Criterios de Aceptación

**Escenario: La rama base del equipo es la que protege el mínimo seguro**

Dado el repo "demo" protegido, cuya configuración del equipo define la rama base "develop"
Cuando un proceso intenta borrar la rama "develop"
Entonces la operación no se ejecuta y el motivo nombra la rama base "develop"

**Escenario: Sin rama base del equipo, la rama base es main**

Dado el repo "demo" protegido sin rama base en la configuración del equipo
Cuando un proceso intenta borrar la rama "main"
Entonces la operación no se ejecuta

**Escenario: Un nivel personal no cambia la rama base**

Dado el repo "demo" con rama base "develop" en la configuración del equipo y "release" en la configuración local personal
Cuando un proceso intenta borrar la rama "release"
Entonces la decisión es la de cualquier otra rama no protegida
  Y la rama base efectiva de "demo" sigue siendo "develop"

**Escenario: En una máquina nueva aplica desde el primer momento**

Dado una máquina nueva con un perfil vacío y "demo" recién clonado con rama base "develop" en la configuración del equipo
Cuando el desarrollador añade y protege "demo"
Entonces borrar "develop" se deniega desde la primera operación

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto).
