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

BR-TMC-CONS-001 (sin snapshot previo no hay operación) · BR-TMC-CONS-002 (qué contiene un snapshot, con la lista cerrada de credenciales excluida por defecto) · D-TMC-9, D-TMC-10 nivel a, D-TMC-16 (actualizada por TQ-16) — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Una operación de GitRaptor guarda antes el trabajo sin commitear**

Dado un worktree "feat-login" con "lib.rs" modificado sin commitear y "nuevo.rs" sin seguimiento
Cuando un solicitante lanza desde GitRaptor una operación que descarta los cambios de "feat-login"
Entonces existe un punto recuperable anterior a la operación con el contenido de "lib.rs" y de "nuevo.rs"
  Y la operación se ejecuta

**Escenario: Descartar un worktree y su rama queda cubierto**

Dado la rama "feat-pagos" con su worktree y trabajo sin commitear
Cuando se descartan desde GitRaptor el worktree y la rama "feat-pagos"
Entonces el punto previo conserva la rama "feat-pagos", su worktree y su trabajo sin commitear

**Escenario: Los archivos ignorados no entran en el punto**

Dado un worktree con ".env" y "node_modules/" ignorados por el repo
Cuando un solicitante lanza desde GitRaptor una operación sobre ese worktree
Entonces el punto previo no contiene ".env" ni "node_modules/"
  Y esos archivos siguen intactos en el worktree

**Escenario: Las credenciales sin seguimiento se excluyen y se declaran**

Dado un worktree con "deploy.pem" y ".env.local" sin seguimiento y sin ignorar, y "nuevo.rs" sin seguimiento
  Y un perfil sin opción para incluir credenciales
Cuando un solicitante lanza desde GitRaptor una operación sobre ese worktree
Entonces el punto previo contiene "nuevo.rs" y no contiene "deploy.pem" ni ".env.local"
  Y el punto previo declara "deploy.pem" y ".env.local" como excluidos por credenciales
  Y esos archivos siguen intactos en el worktree

**Escenario: El perfil permite incluir las credenciales**

Dado un worktree con "deploy.pem" sin seguimiento y sin ignorar
  Y un perfil que incluye las credenciales en los snapshots
Cuando un solicitante lanza desde GitRaptor una operación sobre ese worktree
Entonces el punto previo contiene "deploy.pem"
  Y no lo declara como excluido

**Escenario: Si el punto previo no se puede guardar, la operación no se ejecuta**

Dado que guardar el punto previo falla porque no hay espacio en disco
Cuando un solicitante lanza desde GitRaptor el descarte del worktree "feat-pagos"
Entonces la operación no se ejecuta
  Y el worktree, la rama y su trabajo sin commitear siguen intactos
  Y el solicitante recibe el motivo

## Requisitos Técnicos

- Toda operación que modifica el repo entra por la operación protegida del canal: intención, snapshot previo, ejecución y registro; sin snapshot no hay operación (ADR-TMC-004 § 1; TS-TMC-004).
- El snapshot incluye todos los worktrees del ámbito de la operación y el estado de refs del repo; los ignorados nunca entran (ADR-TMC-001 § 1-2; TS-TMC-001).
- El catálogo de operaciones (descartar cambios, descartar worktree y rama, merge, rebase), con su ámbito y si son destructivas, es un contrato de interfaz de F-001-02 y F-001-05. Las ejecuta el ejecutor del daemon con los hooks del usuario; esta historia solo garantiza el snapshot previo y el registro (ADR-TMC-002 § 5, ADR-TMC-007 § 2).
- El presupuesto del snapshot previo y el repo de referencia los fija SPIKE-TMC-001 (ADR-TMC-006).
- Fallo del snapshot (sin espacio, tiempo máximo, almacén no disponible): operación `abortada` con motivo tipado en/es y repo sin cambios (ADR-TMC-003 § 3).
- Verificación: arnés INF-TMC-001 y test de contrato de `crates/api` sin vías de escritura fuera de la operación protegida.

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** [DS-US-TMC-001](../dev-specs/US-TMC-001-snapshot-previo-operaciones.md)

## Dependencias

- **Historias**: US-GRP-001 (repo observado).
- **Externas**: F-001-02 Cockpit (descartar, merge, rebase; BR-07) y F-001-05 Servidor MCP usan esta garantía para sus acciones.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
