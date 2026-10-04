---
id: US-GRD-011
title: "Una configuración rota no deja pasar las operaciones peligrosas"
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
    - US-GRD-010
tags:
  - guardrails
  - fail-safe
  - configuracion-tres-niveles
  - bloqueada
---

# US-GRD-011: Una configuración rota no deja pasar las operaciones peligrosas

## Descripción

**Como** desarrollador orquestador, **quiero** que, si un nivel de la configuración no se puede leer, GitRaptor me avise y siga aplicando el mínimo seguro y lo que sí se puede leer, **para** que un error de edición o un conflicto de merge no desactive la protección.

**Valor**: Guardrails nunca cae en "todo permitido" (Q-GRD-12).

## Reglas cubiertas

BR-EDGE-004 (aviso; mínimo seguro más lo legible; un nivel personal ilegible solo pierde sus endurecimientos) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-010 (lectura de los tres niveles).
- **Externas**: **bloqueada** por el ADR de formato P8 (motor-local). Dos escenarios dependen de la pregunta abierta P-GRD-17: si rige la última versión commiteada, un conflicto sin commitear no vuelve ilegible la configuración del equipo y hay que reformularlos (ver índice). Alinear con BR-CONS-007 (motor-local) es una dependencia para el Arquitecto; esta historia no cambia el comportamiento del motor.
- **Transversal**: Windows, macOS y Linux.

## Criterios de Aceptación

**Escenario: La configuración del equipo con un conflicto sin resolver** *(Depende de P-GRD-17)*

Dado el repo "demo" protegido cuya configuración del equipo permite force-push
  Y esa configuración queda con un conflicto de merge sin resolver
Cuando un proceso hace force-push
Entonces la operación no se ejecuta por el conjunto mínimo por defecto
  Y GitRaptor avisa de que la configuración del equipo de "demo" no se puede leer

**Escenario: Lo legible de otros niveles sigue aplicando**

Dado el repo "demo" con la configuración del equipo ilegible y la ruta prohibida "secrets/" en la configuración local personal
Cuando un proceso hace un commit que modifica "secrets/api.txt"
Entonces el commit no se ejecuta

**Escenario: Un nivel personal ilegible solo pierde sus endurecimientos**

Dado el repo "demo" con push permitido por el equipo y un perfil ilegible que lo denegaba
Cuando un proceso hace push
Entonces el push se ejecuta
  Y GitRaptor avisa de que el perfil no se puede leer

**Escenario: Al corregir la configuración vuelven sus reglas** *(Depende de P-GRD-17)*

Dado el repo "demo" con la configuración del equipo ilegible
Cuando el desarrollador resuelve el conflicto
Entonces las reglas del equipo vuelven a aplicar sin reinstalar la protección y el aviso desaparece

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto).
