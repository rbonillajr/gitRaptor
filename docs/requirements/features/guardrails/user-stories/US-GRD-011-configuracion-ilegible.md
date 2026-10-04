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
---

# US-GRD-011: Una configuración rota no deja pasar las operaciones peligrosas

## Descripción

**Como** desarrollador orquestador, **quiero** que, si un nivel de la configuración no se puede leer, GitRaptor me avise y siga aplicando el mínimo seguro y lo que sí se puede leer, **para** que un error de edición o un conflicto de merge no desactive la protección.

**Valor**: Guardrails nunca cae en "todo permitido" (Q-GRD-12).

## Reglas cubiertas

BR-EDGE-004 (aviso; mínimo seguro más lo legible; un nivel personal ilegible solo pierde sus endurecimientos; una clave desconocida deja el nivel parcial y fuerza el mínimo, Q-GRD-26) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-010 (lectura de los tres niveles).
- **Externas**: ninguna bloqueante. P8 (formato de la configuración, motor-local) quedó cerrada por ADR-GRP-007, aceptado por Rene Bonilla el 2026-10-04. Por Q-GRD-17 rige la última versión commiteada de la configuración del equipo: un conflicto o una edición sin commitear no la vuelven ilegible.
- **Transversal**: Windows, macOS y Linux.

## Criterios de Aceptación

**Escenario: Se commitea la configuración del equipo con marcas de conflicto**

Dado el repo "demo" protegido cuya configuración del equipo permitía force-push
  Y en el worktree "feat-x" se commitea esa configuración con las marcas de conflicto de un merge
Cuando un proceso hace force-push desde "feat-x"
Entonces la operación no se ejecuta por el conjunto mínimo por defecto, porque cualquier versión ilegible, también la del worktree de la operación, lo fuerza (Q-GRD-12)
  Y GitRaptor avisa de que la configuración del equipo de "demo" no se puede leer en "feat-x"

**Escenario: Un conflicto sin commitear no cuenta**

Dado el repo "demo" cuya configuración del equipo commiteada deniega push
  Y un merge deja esa configuración en conflicto en el working tree de "feat-x", sin commitear
Cuando un proceso hace push desde "feat-x"
Entonces el push se deniega por la versión commiteada
  Y no hay aviso de configuración ilegible

**Escenario: Lo legible de otros niveles sigue aplicando**

Dado el repo "demo" con la configuración del equipo ilegible y la ruta prohibida "secrets/" en la configuración local personal
Cuando un proceso hace un commit que modifica "secrets/api.txt"
Entonces el commit no se ejecuta

**Escenario: Un nivel personal ilegible solo pierde sus endurecimientos**

Dado el repo "demo" con push permitido por el equipo y un perfil ilegible que lo denegaba
Cuando un proceso hace push
Entonces el push se ejecuta
  Y GitRaptor avisa de que el perfil no se puede leer

**Escenario: Una errata en la configuración del equipo nunca relaja**

Dado el repo "demo" cuya configuración del equipo en la rama principal desactiva el conjunto mínimo, con ese cambio confirmado
  Y esa configuración incluye un permiso con el nombre mal escrito
Cuando un proceso hace force-push de "feat-x"
Entonces la operación no se ejecuta por el conjunto mínimo por defecto
  Y GitRaptor avisa de que la configuración del equipo de "demo" se aplica solo en parte

**Escenario: Al commitear la configuración corregida vuelven sus reglas**

Dado el repo "demo" con la configuración del equipo commiteada ilegible en "feat-x"
Cuando el desarrollador commitea en "feat-x" la configuración corregida
Entonces las reglas del equipo vuelven a aplicar sin reinstalar la protección y el aviso desaparece

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto).
