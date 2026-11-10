---
id: US-WTL-005
title: "El desarrollador ve los worktrees de wt, Codex, GitKraken y Orca con su origen sin que GitRaptor los toque"
type: us
status: ready
priority: high
milestone: M3
blocked_by: []
created: 2026-11-10
updated: 2026-11-10
feature: worktree-lifecycle
related:
  context:
    - CTX-WTL-001
  stories:
    - US-CKP-001
    - US-CKP-017
  rules:
    - BR-WTL-011
  specs: []
ado:
  id: null
  url: null
tags:
  - worktree-lifecycle
  - convivencia
  - worktree-ajeno
  - must
---

# US-WTL-005: El desarrollador ve los worktrees de wt, Codex, GitKraken y Orca con su origen sin que GitRaptor los toque

## Descripción

**Como** desarrollador que usa GitRaptor junto a otras herramientas de worktrees, **quiero** ver cada worktree ajeno con su origen y que los de HEAD separado queden marcados «ajeno, no tocar», **para** adoptar GitRaptor sin dejar de usar wt, la app de Codex, GitKraken u Orca y sin que ninguna pierda su trabajo.

**Valor**: BR-04 (Must); NFR-01. Convivencia con las herramientas del Panorama Competitivo de CTX-WTL-001.

## Criterios de Aceptación

**Escenario: Origen visible cuando se puede saber**

Dado un repo con un worktree creado por wt, uno por Orca y uno creado a mano
Cuando el desarrollador consulta la flota
Entonces cada worktree aparece con su origen cuando se puede saber y como «ajeno» cuando no

**Escenario: HEAD separado, ajeno, no tocar**

Dado un worktree de la app de Codex con HEAD separado
Cuando el desarrollador consulta la flota
Entonces aparece marcado «ajeno, no tocar»
  Y GitRaptor no ofrece descartarlo, limpiarlo ni copiar artefactos hacia él

**Escenario: La configuración de la otra herramienta no cambia**

Dado un repo con `.worktreeinclude` y configuración de wt
Cuando GitRaptor observa el repo y crea un worktree propio
Entonces la configuración de wt queda sin cambios

**Escenario: Un worktree ajeno desaparece mientras se observa**

Dado un worktree de GitKraken en la flota
Cuando GitKraken lo borra
Entonces la flota deja de mostrarlo sin error y sin tocar los demás

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño (marca «ajeno, no tocar» en la fila).

## Dependencias

- **De User Stories**: Relacionado US-CKP-001 (flota), US-CKP-017 (HEAD separado al descartar) · Bloquea US-WTL-008.
- **Riesgo aceptado pendiente de Rene**: un worktree de Codex con HEAD separado se limpia desde Codex, no desde GitRaptor (Riesgo 7 de CTX-WTL-001).
- **Cross-cutting / transversales**: cómo se reconoce el origen de cada worktree: transversal (lo define el Arquitecto).
