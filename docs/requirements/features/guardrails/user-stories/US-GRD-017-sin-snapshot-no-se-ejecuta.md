---
id: US-GRD-017
title: "Una operación destructiva permitida no se ejecuta sin un punto de recuperación"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-09
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRD-001
    - US-TMC-005
  specs:
    - DS-US-GRD-017
tags:
  - guardrails
  - snapshot
  - nfr-01
  - time-machine
---

# US-GRD-017: Una operación destructiva permitida no se ejecuta sin un punto de recuperación

## Descripción

**Como** desarrollador orquestador, **quiero** que una operación destructiva que las reglas permiten solo se ejecute si antes se guardó un punto de recuperación, **para** poder deshacer siempre lo que un agente destruya con permiso.

**Valor**: permitir nunca significa perder trabajo (NFR-01, Q-GRD-11).

## Reglas cubiertas

BR-EDGE-005 (sin snapshot previo, se deniega con motivo; el humano puede usar la excepción consciente) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-001 (decisión); US-TMC-005 (Time Machine: snapshot `previo_hook` pedido desde los hooks, ADR-TMC-004 § 3).
- **Externas**: ninguna. Desbloqueada el 2026-10-04 por decisión de Rene Bonilla, 2026-10-04: la Time Machine (F-001-03) ya tiene historias y ADRs aceptados, y el contrato con Guardrails está en ADR-GRD-003 (con `allow` y una operación destructiva, el daemon pide el `previo_hook` en la misma llamada). El bloqueo por F-001-03 pasa a ser dependencia de US-TMC-005. Guardrails no toma el snapshot.
- **Dependencia blanda**: US-GRD-006 (excepción consciente) debe cerrar en el mismo hito. Sin ella, el escenario "El humano decide seguir sin punto de recuperación" no tiene recurso y la persona no puede seguir sin punto de recuperación.
- **Transversal**: Windows, macOS y Linux; pruebas de caos (NFR-12). Los escenarios usan una operación que la capa de hooks puede interceptar; `reset --hard` con Git directo no lo es (BR-EDGE-003) y por MCP se cubre cuando exista F-001-05.
- **Límites declarados** (BR-EDGE-005):
  - Force-push y borrado de una rama remota: fuera de esta historia, porque la Time Machine no guarda el remoto. Se reabre cuando lo guarde; complementa US-TMC-014. El force-push ya lo deniega el mínimo seguro.
  - `reset --hard` y `checkout` con Git crudo, y mover una rama hacia atrás sin borrarla (`branch -f`): Git no ofrece un aviso previo que GitRaptor pueda interceptar. Los cubre la observación continua de la Time Machine, no esta historia.

## Criterios de Aceptación

**Escenario: Con snapshot, la operación permitida se ejecuta**

Dado el repo "demo" protegido, donde borrar ramas está permitido, y la rama "feat-x" con 3 commits que no están en ninguna otra rama
Cuando un proceso borra "feat-x" con Git directo y la Time Machine guarda antes el punto de recuperación
Entonces la operación se ejecuta
  Y los 3 commits de "feat-x" se pueden recuperar desde ese punto

**Esquema del escenario: Sin snapshot, la operación se deniega y el motivo dice por qué**

Dado el repo "demo" donde borrar ramas está permitido, y la rama "feat-x" con commits que no están en ninguna otra rama
  Y <situación>
Cuando un proceso borra "feat-x"
Entonces la operación no se ejecuta
  Y el motivo dice que no se pudo guardar un punto de recuperación y por qué: "<por qué>"
  Y "feat-x" sigue existiendo con sus commits

Ejemplos:

| situación | por qué |
| la Time Machine no termina de guardar el punto de recuperación a tiempo | se agotó el plazo para guardarlo |
| GitRaptor no está en marcha (modo degradado) | GitRaptor no está en marcha; arranca GitRaptor y repite la operación |

**Escenario: Un rebase sin snapshot tampoco se ejecuta**

Dado el repo "demo" donde el rebase está permitido, y la rama "feat-y" con 2 commits propios
Cuando un proceso hace rebase de "feat-y" y la Time Machine no puede guardar el punto de recuperación
Entonces el rebase no se ejecuta
  Y "feat-y" conserva sus 2 commits sin cambios

**Escenario: El humano decide seguir sin punto de recuperación**

Dado la rama "feat-x" sin snapshot posible y GitRaptor en marcha
Cuando el desarrollador usa la excepción consciente para esa operación
Entonces la operación se ejecuta y la excepción queda en el registro

## Requisitos Técnicos

- **Gobierno**: ADR-GRD-003 § 3 (razón de sistema `snapshot-failed`), § 4 (modo degradado) y § 7 (con `allow` y una operación destructiva, el daemon pide el `previo_hook` en la misma llamada); ADR-TMC-004 § 3; ADR-GRD-007 § 3 (excepción consciente, nunca en modo degradado). Detalle en [DS-US-GRD-017](../dev-specs/US-GRD-017-sin-snapshot-no-se-ejecuta.md).
- **Alcance**: el de US-TMC-005: rebase (también `pull --rebase`) y borrado de una rama local. El force-push y el borrado remoto quedan fuera (la Time Machine no guarda el remoto). `reset --hard`, `checkout` y `branch -f` con Git crudo no tienen hook previo (BR-EDGE-003); ver "Límites declarados".
- **Decisión**: en el daemon, entre el snapshot y el registro de decisiones, un `failed` (cualquier causa: plazo, disco, cuota, repo sin observar) convierte la decisión permitida en `deny` con `system.snapshot-failed` y la causa. Sin daemon, el cliente del hook deniega la misma operación (modo degradado). Un archivo enorme no tiene tope: el plazo vencido deniega, con una pista neutra en el mensaje.
- **Contrato**: capacidad `guard.snapshot-required` (ADR-GRP-016). Un cliente sin ella recibe la misma denegación como `system.internal-error`, nunca un permiso.
- **Cupos** (Q-GRD-37): los cupos globales del snapshot previo (por worktree y por repo) solo cuentan las peticiones de agentes; la persona solo gasta su cupo por solicitante. Lo trae US-TMC-005 o, si no, la DS (T008).
- **Excepción consciente** (escenario "El humano decide seguir sin punto de recuperación"): con la excepción aplicada, el previo se intenta y, si falla, la operación sigue y el registro anota la regla saltada. Lo verifica US-GRD-006; hasta entonces la historia queda `partially-implemented`.
- **Crates**: `crates/api` (regla, causas, capacidad), `crates/core` módulos `guardrails` (conversión, cliente del hook) y `timemachine` (cupos, T008) y el canal, `apps/cli` (mensajes en un archivo i18n propio).
- **Dependencia**: implementable solo cuando US-TMC-005 esté mergeada.

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** [DS-US-GRD-017](../dev-specs/US-GRD-017-sin-snapshot-no-se-ejecuta.md) (status `draft`).
