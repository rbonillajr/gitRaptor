---
id: US-GRD-002
title: "Los hooks que el repo ya tenía siguen funcionando al protegerlo"
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

- **Gobierno**: ADR-GRD-001 § 2 (el binario encadena el hook previo sin shell, con los mismos argumentos, la misma entrada y el entorno original menos el token; si la evaluación deniega, no lo ejecuta) y § 6 (hooks propios, husky, lefthook y pre-commit).
- **Instalación**: las comprobaciones previas de ADR-GRD-001 § 4 (paso 1) y la cobertura de worktrees de § 5 deciden si se puede encadenar. Si no se puede, no se escribe nada y queda el diagnóstico `encadenado-imposible` con su causa (ADR-GRD-005 § 1).
- **Permiso**: la confirmación de instalar enumera los hooks previos detectados y dice que se conservan (ADR-GRD-007 § 1; BR-AUTH-002).
- **Crates**: `crates/core` módulo `guardrails` (detección y comprobaciones previas), `crates/git` capa de escritura (valor efectivo de `core.hooksPath` en cada worktree) y `apps/cli` (`raptor hook` y el encadenado).
- **Enablers**: SPIKE-GRD-001 completo (coexistencia con gestores) **bloquea el cierre de la Dev Spec**. La suite de encadenado de INF-GRD-001 **bloquea el merge**. TS-GRD-001 no aplica.
- **NFR y SEC**: NFR-GRD-01, 11 y 12; SEC-GRD-03 (encadenado en Rust sin shell) y SEC-GRD-08 (`core.hooksPath` manipulado).
- **Verificación**: ADR-GRD-001 Validación 1, 2, 4 y 7 (hook previo relativo de husky); ADR-GRD-005 Validación 5.
- **Enmiendas en motor-local**: INF-GRP-001 (excepción de huella por escenario), además de las que ya hereda de US-GRD-001.

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** [DS-US-GRD-002](../dev-specs/US-GRD-002-hooks-previos-respetados.md).

## Estado de la implementación (2026-10-08)

Implementado en: PR #193. Dev Spec: [DS-US-GRD-002](../dev-specs/US-GRD-002-hooks-previos-respetados.md).
