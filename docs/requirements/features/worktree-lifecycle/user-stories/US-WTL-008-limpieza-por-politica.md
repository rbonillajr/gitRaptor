---
id: US-WTL-008
title: "El disco no se llena de artefactos viejos porque GitRaptor los limpia según la política del repo"
type: us
status: ready
priority: medium
milestone: M4
blocked_by:
  - US-WTL-005
created: 2026-11-10
updated: 2026-11-10
feature: worktree-lifecycle
related:
  context:
    - CTX-WTL-001
  stories:
    - US-WTL-005
  rules:
    - BR-WTL-011
    - BR-WTL-014
  specs: []
ado:
  id: null
  url: null
tags:
  - worktree-lifecycle
  - limpieza
  - disco
  - politica
  - should
---

# US-WTL-008: El disco no se llena de artefactos viejos porque GitRaptor los limpia según la política del repo

## Descripción

**Como** desarrollador orquestador, **quiero** definir en la política del repo o de mi perfil cuándo se limpian los artefactos de compilación de los worktrees que creó GitRaptor, **para** que el disco no se llene con varios agentes a la vez, como pasó el 2026-11-08 en el dogfooding.

**Valor**: BR-07 (Should, v0.1.0); NFR-01.

## Criterios de Aceptación

**Escenario: Limpieza según la política**

Dado una política del repo que pide limpiar artefactos de compilación
  Y el worktree "feat-a" creado por GitRaptor con `target/` ignorado
Cuando se cumple la condición de la política
Entonces GitRaptor borra artefactos de compilación ignorados de "feat-a"
  Y informa el espacio que ganó el disco

**Escenario: Nunca trabajo rastreado ni sin commitear**

Dado "feat-a" con 1 archivo rastreado modificado y 1 archivo nuevo sin commitear
Cuando corre la limpieza
Entonces los dos archivos quedan intactos

**Escenario: Nunca worktrees ajenos**

Dado un worktree creado por wt y uno marcado «ajeno, no tocar»
Cuando corre la limpieza
Entonces ninguno de los dos cambia

**Escenario: Sin política, sin limpieza**

Dado un repo y un perfil sin política de limpieza
Cuando pasa el tiempo
Entonces GitRaptor no borra ningún artefacto

**Escenario: La limpieza falla a medias**

Dado una limpieza que no puede borrar un archivo de "feat-a"
Cuando termina
Entonces GitRaptor informa qué no se limpió y deja "feat-a" usable

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño.

## Dependencias

- **De User Stories**: Requiere US-WTL-005 (saber qué worktree es ajeno).
- **Cross-cutting / transversales**: qué condiciones admite la política (antigüedad, tamaño, rama integrada) y su equivalencia con cargo-sweep: transversal (lo define el Arquitecto con Rene); las cifras por defecto son `sin dato`.
