---
id: US-GRP-001
title: "El desarrollador ve el estado de cada worktree del repo que añadió"
type: us
status: draft
priority: high
created: 2026-10-03
updated: 2026-10-03
feature: motor-local
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
tags:
  - motor-local
  - observacion
  - worktrees
---

# US-GRP-001: El desarrollador ve el estado de cada worktree del repo que añadió

## Descripción

**Como** desarrollador orquestador, **quiero** añadir un repo y consultar la rama, los cambios sin commitear y los archivos modificados de cada uno de sus worktrees, **para** dejar de reconstruir a mano el estado de cada directorio con `git status`.

**Valor**: una sola consulta sustituye la ronda de terminales, y añadir el repo no lo modifica en nada. El ahead/behind contra la rama base es de US-GRP-012.

## Reglas cubiertas

BR-AUTH-001 (incluido: un agente no cambia los repos observados, Q40) · BR-CONS-001 · BR-AUTH-002 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: ninguna (esqueleto andante, primera historia).
- **Externas**: ninguna bloqueante. El canal por el que un agente pide cambiar los repos es del Servidor MCP (F-001-05); el rechazo lo decide el motor y se verifica contra su capacidad (Q40).
- **Transversal**: todo escenario se cumple igual en Windows, macOS y Linux (BR-03) y sin escribir nada en el repo observado (BR-CONS-001); cómo se verifica lo define el plan técnico.

## Criterios de Aceptación

**Escenario: El estado de cada worktree queda disponible al añadir el repo**

Dado un repo "demo" con el worktree principal en la rama "main"
  Y un worktree "feat-login" en la rama "feat-login" con el archivo "login.txt" modificado sin commitear
Cuando el desarrollador añade "demo" a la observación
  Y se consulta el estado del motor
Entonces el estado lista los dos worktrees de "demo" con su rama
  Y "feat-login" figura con cambios sin commitear y el archivo "login.txt" entre los modificados
  Y el worktree principal figura sin cambios

**Escenario: Solo se observan los repos añadidos**

Dado un repo "demo" observado y un repo "otro" que el desarrollador no ha añadido
Cuando se consulta el estado del motor
Entonces el estado incluye "demo" y no incluye "otro"
Cuando el desarrollador retira "demo" de la observación
Entonces el estado ya no incluye "demo"

**Escenario: Un agente no puede añadir ni retirar repos**

Dado el repo "demo" observado y un repo "otro" que el desarrollador no ha añadido
Cuando un agente, y no el desarrollador, pide añadir "otro" o retirar "demo"
Entonces el motor rechaza la petición indicando que solo el desarrollador puede cambiar los repos observados
  Y la lista de repos observados no cambia

**Escenario: Un directorio que no es un repo Git no se añade**

Dado un directorio "notas" que no es un repo Git
Cuando el desarrollador intenta añadir "notas" a la observación
Entonces el motor rechaza la petición indicando que "notas" no es un repo Git
  Y la lista de repos observados no cambia

**Escenario: Observar el repo no lo modifica**

Dado un repo "demo" con un archivo modificado, un archivo preparado para el próximo commit, un archivo no rastreado, un stash y un hook de Git propio del desarrollador
  Y se toma una huella del repo: working tree, área de preparación, ramas, tags, HEAD, stash, ramas remotas conocidas, worktrees, hooks, configuración de Git del repo y metadatos de worktrees
Cuando el desarrollador añade "demo", el motor lo observa y el desarrollador lo retira
Entonces la huella del repo es idéntica a la inicial
  Y no existe ningún archivo ni carpeta nuevos dentro del repo
  Y fuera del repo lo único que cambió son los datos del motor en el perfil de GitRaptor

**Escenario: Preparar todos los cambios no recoge nada del motor**

Dado un repo "demo" observado en el que el desarrollador modificó "login.txt"
Cuando el desarrollador prepara todos los cambios del repo para el próximo commit
Entonces lo preparado contiene solo "login.txt"
