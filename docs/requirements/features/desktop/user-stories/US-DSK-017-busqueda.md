---
id: US-DSK-017
title: "El usuario encuentra un commit, una rama o un archivo escribiendo lo que busca"
type: us
status: ready
priority: medium
created: 2026-10-28
updated: 2026-11-08
feature: desktop
source: inline
decision: "Decisión de Rene del 2026-11-08 (aceptación de las recomendaciones, Q-DSK-7 a Q-DSK-20), ver CTX-DSK-001 § 11"
related:
  context:
    - CTX-DSK-001
  stories:
    - US-TMC-007
    - US-DSK-005
    - US-DSK-009
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-2
  - busqueda
  - should
---

# US-DSK-017: El usuario encuentra un commit, una rama o un archivo escribiendo lo que busca

## Descripción

**Como** usuario de un repo con muchos commits y varios agentes, **quiero** escribir parte de un mensaje, de un autor, de un nombre de rama o de una ruta y saltar a lo que encuentre, **para** localizar en segundos qué cambió y quién lo hizo sin recorrer el grafo a mano.

**Valor**: BR-27 (UI-2, Should). Nivel **UI-2**. Imprescindible.

## Reglas

- La búsqueda solo lee: nunca escribe en el repo.
- El autor de un commit se busca por persona y por agente (D6, US-GRD-019).
- El historial de la Time Machine ya se filtra por copia de trabajo, agente y periodo (US-TMC-007); la búsqueda no lo duplica, lleva a él.

## Dependencias

- **Reutiliza**: US-TMC-007 (filtros del historial), US-GRD-019 (quién ejecutó y a nombre de quién).
- **Historias**: US-DSK-005 (el grafo, al que lleva un resultado), US-DSK-009 (el diff de un commit encontrado).
- **Qué falta en el motor**: DEP-DSK-3, la búsqueda de commits, ramas y rutas en repos de 100K commits (NFR-05). transversal (lo define el Arquitecto).

## Criterios de Aceptación

**Escenario: Buscar por mensaje**

Dado "tienda" con un commit "fix: rounding in invoices" de "claude-1"
Cuando el usuario busca "rounding"
Entonces encuentra ese commit con su autor, su agente y su rama

**Escenario: Buscar una rama**

Dado las ramas "feat-pagos" y "feat-pagos-v2"
Cuando el usuario busca "pagos"
Entonces encuentra las dos ramas y puede ir a cualquiera en el grafo

**Escenario: Sin resultados**

Dado "tienda" sin nada que contenga "kubernetes"
Cuando el usuario busca "kubernetes"
Entonces sabe que no hay resultados y en qué se buscó

## Requisitos Técnicos

Completado por el Arquitecto (2026-11-08). **Decisión del orquestador (2026-11-08), validada por Arquitecto**: la búsqueda de commits es el filtro de `log.page` y la de ramas el de `refs.list` (TS-CKP-007); el diff de un resultado es `diff.commit` (TS-CKP-006). La búsqueda de **rutas** solo la usa esta historia, así que va aquí. **Bloqueada por**: TS-CKP-006 y TS-CKP-007.

- Lectura `paths.search` en su módulo del canal (ADR-GRP-016): rutas del árbol de HEAD (o de un commit alcanzable) que contienen el texto, tratado como literal y sin distinguir mayúsculas, con tope de resultados y de tiempo; la respuesta dice si se cortó. Nunca lee el contenido de los archivos.
- Frontera de ADR-GRP-009 § 1: lectura sin filtros y sin proceso hijo, en el pool de consultas de US-CKP-012, con `query.cancel`. Perfil completo; nunca `raptor-mcp`.
- La búsqueda nunca escribe: sin operación del catálogo ni punto previo.
- Un resultado de autor o agente lleva al historial filtrado de la Time Machine (US-TMC-007), sin duplicarlo.
- Pruebas: los tres escenarios en repo temporal; en el repo de 100K commits de INF-GRP-002, la búsqueda de rutas y la de commits dentro de NFR-05 o cortadas por tope con su aviso.

## Diseño y Dev Spec

- **Diseño:** pendiente (UI-2 no está en DSF-DSK-001).
- **Dev Spec:** pendiente.
