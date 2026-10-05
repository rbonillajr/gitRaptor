---
id: US-CKP-003
title: "La TUI dice en qué estado está el motor y qué hacer en cada caso"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
feature: cockpit
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
  stories:
    - US-CKP-001
    - US-GRP-003
    - US-GRP-004
    - US-GRP-012
    - US-GRP-014
    - US-GRP-015
tags:
  - cockpit
  - estados-del-motor
  - vacios-utiles
  - must
---

# US-CKP-003: La TUI dice en qué estado está el motor y qué hacer en cada caso

## Descripción

**Como** desarrollador orquestador, **quiero** que la TUI me diga si el motor espera a Git, no tiene repos, está reconciliando, perdió la conexión o no puede calcular la base, con la acción que lo resuelve, **para** no confundir una vista vacía o vieja con una flota parada.

**Valor**: BR-04 (Must). Una vista que miente sobre su propio estado destruye la confianza en todo lo demás.

## Reglas cubiertas

BR-CKP-WF-004 · BR-CKP-EDGE-001 (ahead/behind no calculable) · BR-CKP-EDGE-003 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-001; US-GRP-003 (no disponible), US-GRP-004 (observación continua y degradada), US-GRP-012 (rama base), US-GRP-014 (Git ausente), US-GRP-015 (primer repo).
- **Técnicas**: INF-CKP-001 (resync del cliente; DEP-CKP-6). Autoarranque del daemon: DEP-CKP-12; sin él, la TUI da las instrucciones.

## Criterios de Aceptación

**Escenario: Sin daemon, la TUI lo arranca o explica cómo, sin embeber el motor**

Dado que el daemon no está en marcha
Cuando el desarrollador abre la TUI
Entonces el daemon queda en marcha y la TUI muestra la vista normal, o la TUI muestra cómo arrancarlo
  Y en ningún caso el proceso de la TUI observa el repo por su cuenta

**Escenario: "Esperando Git" tiene prioridad sobre "Sin repos"**

Dado una máquina sin Git instalado y sin repos observados
Cuando el desarrollador abre la TUI
Entonces la TUI muestra "Esperando Git" con la versión mínima y cómo instalarla, y no la guía de primer repo

**Escenario: Sin repos, un vacío que guía**

Dado Git 2.38 o superior y ningún repo observado
Cuando el desarrollador abre la TUI
Entonces la TUI muestra cómo añadir el primer repo con "raptor repo add <ruta>"

**Escenario: Reconciliando o en resync, la vista lo dice**

Dado la TUI abierta en "shop"
Cuando el motor pasa a "Reconciliando", o el canal pide un resync por cliente lento
Entonces la vista muestra el aviso correspondiente mientras dure
  Y tras el resync la lista coincide con la instantánea nueva, sin eventos perdidos ni duplicados

**Escenario: Worktree no disponible, sin acciones**

Dado el directorio de "feat-x" borrado a mano
Cuando el motor lo publica como no disponible
Entonces su fila dice "no disponible" y no ofrece ninguna acción

**Escenario: La rama base no existe y nunca se elige otra**

Dado "develop" como rama base y sin esa rama en el repo
Cuando el desarrollador mira la lista
Entonces el ahead/behind de cada fila dice "no calculable: develop no encontrada"
  Y ningún valor se calcula contra otra rama

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec)._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (estados vacíos y avisos); ADR-GRP-004 § 3 (errores accionables, vacíos útiles).
- **Dev Spec:** pendiente.
