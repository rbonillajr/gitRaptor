---
id: US-GRP-012
title: "El desarrollador ve el ahead/behind de cada worktree contra la rama base del repo"
type: us
status: draft
priority: medium
created: 2026-10-03
updated: 2026-10-04
feature: motor-local
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  stories:
    - US-GRP-001
tags:
  - motor-local
  - rama-base
---

# US-GRP-012: El desarrollador ve el ahead/behind de cada worktree contra la rama base del repo

## Descripción

**Como** desarrollador orquestador, **quiero** que el ahead/behind de cada worktree se calcule contra la rama base del repo, hoy `main`, y se mantenga al día sin que el motor toque el remoto, **para** saber cuánto se ha separado cada agente de lo que se integra.

**Valor**: un ahead/behind fiable sin esperar a Guardrails; el valor del equipo llega con US-GRP-016.

## Reglas cubiertas

BR-CONS-006 (rama base `main` provisional; esta historia es la única dueña del ahead/behind; si la rama base no existe, no se elige otra, Q42) · BR-CONS-001 (sin traer novedades del remoto, Q12) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-001.
- **Externas**: ninguna. Leer la rama base de la configuración del equipo es US-GRP-016 (desbloqueada el 2026-10-04; depende de US-GRP-013 y TS-GRD-001). Hasta que se integre, la rama base es `main` en todos los repos.
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Escenario: Sin configuración del equipo, la rama base es main**

Dado el repo "demo" observado sin configuración del equipo
  Y el worktree "feat-login" con 3 commits que no están en "main" y 1 commit de "main" que no tiene
Cuando se consulta el estado del motor
Entonces el estado indica que la rama base de "demo" es "main"
  Y "feat-login" figura con 3 por delante y 1 por detrás

**Escenario: El ahead/behind se recalcula cuando avanza la rama base**

Dado "feat-login" con 3 por delante y 1 por detrás de "main"
Cuando se hace un commit nuevo en "main"
Entonces "feat-login" figura con 3 por delante y 2 por detrás

**Escenario: El motor no trae novedades del remoto**

Dado el repo "demo" observado cuyo remoto tiene 2 commits en "main" que el repo todavía no conoce
Cuando se consulta el estado del motor
Entonces el ahead/behind de cada worktree se calcula con lo que el repo ya conoce del remoto
  Y las ramas remotas conocidas por el repo siguen apuntando a los mismos commits que antes

**Escenario: Si la rama base no existe, el motor lo indica y no elige otra**

Dado el repo "otro" observado sin rama "main" y sin configuración del equipo
Cuando se consulta el estado del motor
Entonces el estado indica para cada worktree de "otro" que no se puede calcular el ahead/behind porque la rama base "main" no existe
  Y no se usa ninguna otra rama como base
