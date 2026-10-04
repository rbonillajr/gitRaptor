---
id: US-GRD-002
title: "Los hooks que el repo ya tenía siguen funcionando al protegerlo"
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
tags:
  - guardrails
  - hooks-git
  - hooks-previos
  - nfr-01
---

# US-GRD-002: Los hooks que el repo ya tenía siguen funcionando al protegerlo

## Descripción

**Como** desarrollador orquestador, **quiero** que proteger un repo que ya tiene hooks propios o de otro gestor no los reemplace ni los altere, **para** no perder las comprobaciones que mi equipo ya tenía (linter, tests, convenciones).

**Valor**: proteger un repo nunca rompe lo que ya funcionaba (NFR-01, Q-GRD-4).

## Reglas cubiertas

BR-EDGE-002 (detectar, informar y encadenar solo con permiso; si no se puede, no instalar) · BR-CONS-005 (hooks previos con el mismo contenido y efecto) · BR-AUTH-002 (el permiso explica qué hooks previos hay) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-001 (instalación con permiso y mínimo seguro).
- **Externas**: ninguna.
- **Transversal**: Windows, macOS y Linux; nada cambia fuera del repo (NFR-01).

## Criterios de Aceptación

**Escenario: El permiso informa de los hooks previos**

Dado el repo "demo" observado con un hook propio que ejecuta el linter antes de cada commit
Cuando el desarrollador pide proteger "demo"
Entonces GitRaptor le informa de que existe ese hook y de que se conservará
  Y solo instala la protección si el desarrollador concede el permiso

**Escenario: Tras proteger, el hook previo y Guardrails actúan los dos**

Dado el repo "demo" protegido con permiso, que tenía un hook propio de linter
Cuando un proceso hace un commit con un error de linter
Entonces el commit se rechaza por el linter, como antes de proteger el repo
  Y un force-push posterior sigue denegado por el mínimo seguro

**Escenario: El contenido del hook previo no cambia**

Dado el repo "demo" con un hook propio de linter
Cuando el desarrollador protege "demo"
Entonces el hook propio conserva exactamente el mismo contenido

**Escenario: Si no se puede encadenar, no se instala nada**

Dado el repo "demo" con hooks de otro gestor que no se pueden encadenar sin alterarlos
Cuando el desarrollador pide proteger "demo"
Entonces GitRaptor no instala nada y explica el motivo
  Y las rutas operativas de "demo" quedan idénticas a como estaban
  Y el repo sigue sin la capa de hooks (Solo MCP o Sin protección, según la allowlist)

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** pendiente (Arquitecto).
