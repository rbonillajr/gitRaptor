---
id: US-TMC-001
title: "El desarrollador recupera su trabajo sin commitear tras cualquier operación lanzada por GitRaptor"
type: us
status: draft
priority: high
created: 2026-10-03
updated: 2026-10-03
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-GRP-001]
covers: [BR-TMC-CONS-001, BR-TMC-CONS-002, D-TMC-3, D-TMC-9, D-TMC-10, D-TMC-16]
blocked_by: []
tags: [time-machine, snapshots, nfr-01]
---

# US-TMC-001: El desarrollador recupera su trabajo sin commitear tras cualquier operación lanzada por GitRaptor

## Descripción

**Como** desarrollador orquestador
**Quiero** que toda operación que GitRaptor ejecute sobre mi repo quede precedida de un punto recuperable
**Para** no perder nunca trabajo sin commitear por una acción de GitRaptor (NFR-01)

**Valor**: cualquier acción de la CLI, del Cockpit o del MCP se puede revertir porque antes quedó guardado el estado completo.

## Reglas cubiertas

BR-TMC-CONS-001 (sin snapshot previo no hay operación) · BR-TMC-CONS-002 (qué contiene un snapshot) · D-TMC-9, D-TMC-10 nivel a, D-TMC-16 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Una operación de GitRaptor guarda antes el trabajo sin commitear**

Dado un worktree "feat-login" con "lib.rs" modificado sin commitear y "nuevo.rs" sin seguimiento
Cuando el desarrollador lanza desde GitRaptor una operación que descarta los cambios de "feat-login"
Entonces existe un punto recuperable anterior a la operación con el contenido de "lib.rs" y de "nuevo.rs"
  Y la operación se ejecuta

**Escenario: Descartar un worktree y su rama queda cubierto**

Dado la rama "feat-pagos" con su worktree y trabajo sin commitear
Cuando se descartan desde GitRaptor el worktree y la rama "feat-pagos"
Entonces el punto previo conserva la rama "feat-pagos", su worktree y su trabajo sin commitear

**Escenario: Los archivos ignorados no entran en el punto**

Dado un worktree con ".env" y "node_modules/" ignorados por el repo
Cuando el desarrollador lanza desde GitRaptor una operación sobre ese worktree
Entonces el punto previo no contiene ".env" ni "node_modules/"
  Y esos archivos siguen intactos en el worktree

**Escenario: Si el punto previo no se puede guardar, la operación no se ejecuta**

Dado que guardar el punto previo falla porque no hay espacio en disco
Cuando el desarrollador lanza desde GitRaptor el descarte del worktree "feat-pagos"
Entonces la operación no se ejecuta
  Y el worktree, la rama y su trabajo sin commitear siguen intactos
  Y el desarrollador recibe el motivo

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-GRP-001 (repo observado).
- **Externas**: F-001-02 Cockpit (descartar, merge, rebase; BR-07) y F-001-05 Servidor MCP usan esta garantía para sus acciones.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
