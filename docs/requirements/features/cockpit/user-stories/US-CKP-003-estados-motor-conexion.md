---
id: US-CKP-003
title: "La TUI dice en qué estado está el motor y qué hacer en cada caso"
type: us
status: implemented
priority: high
created: 2026-10-04
updated: 2026-10-21
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

**Escenario: Sin daemon, la TUI lo arranca**

Dado el daemon parado y el autoarranque disponible
Cuando el desarrollador abre la TUI
Entonces el daemon queda en marcha y la TUI llega a la vista en vivo

**Escenario: Sin daemon y sin poder arrancarlo, instrucciones**

Dado el daemon parado y su arranque imposible
Cuando el desarrollador abre la TUI
Entonces la TUI muestra "Motor no disponible" con cómo arrancarlo
  Y el proceso de la TUI no observa el repo por su cuenta

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

- El diseño está en [DS-US-CKP-003](../dev-specs/US-CKP-003-estados-motor-conexion.md): un panel de estado nuevo (`StatePanel`) para una conexión sin motor (no disponible, incompatible, rechazada), Esperando Git y Sin repos, con una precedencia fija (D1, D2).
- La cabecera muestra "reconciliando" o "dormido" con el nivel del repo que publica el ámbito global (`observation.tiers`), y el motivo de cada resync, también el del cliente lento (D5, D6).
- Las filas dicen "no disponible: …" y "no calculable", y el título nombra la base publicada (D7, D8). Lo que el motor aún no publica (versión de Git rechazada, observación degradada) queda con dueño en la Dev Spec (D9).

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (estados vacíos y avisos); ADR-GRP-004 § 3 (errores accionables, vacíos útiles).
- **Dev Spec:** [DS-US-CKP-003](../dev-specs/US-CKP-003-estados-motor-conexion.md).

## Estado de la implementación (2026-10-21)

Implementado en: PR #PRN ([DS-US-CKP-003](../dev-specs/US-CKP-003-estados-motor-conexion.md)).

Verificado en macOS: panel de estado para motor no disponible, incompatible, rechazado y sin canal, con la acción (`raptor daemon`, `raptor status`); Esperando Git (gana a Sin repos y nunca pregunta por el repo de la carpeta); guía `raptor repo add <ruta>` sin repos; cabecera con "reconciliando", "dormido" y el motivo del resync (también el del cliente lento, con backoff creciente); fila "no disponible: …" sin acciones; "no calculable" con la base publicada. Pruebas de pantalla en en/es, sin color y en ASCII, el bucle con daemon falso y pruebas de proceso con el `raptor` real bajo `script`.

Diferido, con dueño en la Dev Spec: versión de Git rechazada y que el motor empiece solo al aparecer Git (US-GRP-014), observación degradada por worktree, hueco de observación en la fila (US-CKP-002), acciones de fila desactivadas con motivo (US-CKP-012) y el nivel de cada repo en el selector (US-CKP-025).

Linux y Windows: *Pendiente: etapa de validación multiplataforma* (XP-21; [`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).
