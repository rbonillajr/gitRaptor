---
id: TD-GRP-008
title: "Un daemon lanzado desde una sesión de agente cuelga de ella por el ppid y S4 puede atribuirle sus escrituras"
type: td
status: draft
feature: motor-local
domain: GRP
priority: medium
complexity: small
created: 2026-11-10
updated: 2026-11-10
source: inline
blocked_by: []
related:
  adrs: [ADR-GRP-012, ADR-GRD-003, ADR-GRP-005]
  stories: [TD-GRP-007, US-TMC-011, US-TMC-003, US-GRP-007]
ado:
  id: null
  url: null
tags: [motor-local, deuda-tecnica, deteccion, s4, atribucion, nfr-01, seguridad, daemon]
---

## TD-GRP-008: Un daemon lanzado desde una sesión de agente cuelga de ella por el ppid y S4 puede atribuirle sus escrituras

**Valor**: la Time Machine y el ejecutor nunca atribuyen a un agente una escritura que hizo el propio motor, aunque el motor naciera de un proceso que descendía de ese agente.

> **Reserva (2026-11-10).** Solo la ficha: sin Dev Spec ni código. Origen: condición 3 de la revisión de `security-review` (security-expert, opus) del PR #352 (TD-GRP-007), que quedó "por verificar, fuera de alcance". El siguiente número libre del dominio `motor-local` es TD-GRP-008 (TD-GRP-007 es el último de la serie).

### Descripción

**Como** desarrollador que deshace el trabajo de un agente con `undo --agent`
**Quiero** que una escritura del propio motor (un restore, una captura o un paso del ejecutor) nunca salga atribuida a una sesión de agente por haber arrancado el motor desde ella
**Para** que `undo --agent` no deshaga, ni deje de deshacer, trabajo por una atribución falsa (NFR-01)

**Origen: PR #352, `security-review` sobre el ajuste del margen de la ventana de escritura, condición 3.**

**Hecho** (`crates/core/src/daemon/mod.rs:1462`): al arrancar como proceso, el daemon hace `setsid()`. Eso lo saca de la sesión y del grupo de procesos de su lanzador, **pero no cambia su padre**: mientras el lanzador siga vivo, el daemon cuelga de él por el `ppid`. Cuando el lanzador sale, el kernel lo reasigna a `init`/`launchd`.

**Riesgo por verificar**: S4 (el hook `reference-transaction` de Guardrails, ADR-GRD-003 § 4) resuelve la atribución subiendo por los ancestros del `git` que mueve las refs. El `update-ref` del aplicador de la Time Machine lo lanza el daemon; si el lanzador del daemon (un cliente MCP, `raptor` o un script) descendía de una sesión de Claude Code y sigue vivo, la cadena `git → daemon → lanzador → … → claude` llega a la sesión y el `update-ref` podría resolverse por S4 hacia ella. La ventana de escritura del daemon (Enmienda 2026-11-09 de ADR-GRP-012) cierra el caso para S3, y esta ficha cubre S4, que no pasa por esa ventana.

**Qué no se sabe** (el primer trabajo es medirlo, no suponerlo):

- Si el recorrido de ancestros de S4 cruza el `ppid` del daemon hasta el lanzador, o se corta en el daemon.
- Qué lanzadores reales arrancan el daemon bajo un agente (`raptor-mcp` a demanda, `raptor` desde un shell del agente).
- Si el hook ya ignora las escrituras con la marca de un paso del ejecutor (la marca del ejecutor no prueba autoría, `detect/hook.rs`) y si eso alcanza al aplicador.

### Alcance Técnico

- **Reproducir** con un repo temporal y un lanzador simulado (un proceso que hace de sesión de agente en la tabla de sesiones, hijo del cual arranca el daemon y que sigue vivo): un restore del aplicador y un `update-ref` del ejecutor con el hook S4 instalado. Resultado esperado: sin actor de sesión.
- **Si se reproduce, decidir con el Arquitecto** entre: (a) cortar la subida de ancestros en el proceso del daemon (el daemon y sus hijos nunca son evidencia de sesión), (b) que el daemon se reparente al arrancar (doble `fork`), o (c) marcar el `git` del daemon de forma que el hook lo reconozca. Es una decisión de frontera: se manda un resumen antes de cerrar la Dev Spec.
- **Si no se reproduce**, dejar el caso como prueba de regresión y cerrar la ficha con la evidencia.
- **Fuera de alcance**: S3 y su ventana (TD-GRP-005, TD-GRP-007), la latencia de FSEvents (TD-GRP-006), la presencia de sesión del cliente (SPIKE-DSK-001) y cualquier cambio a Windows (sin lector de argv; la ventana sí se aplica).

### Plan de Verificación

- **Reproducción (automatizada)**: un restore y un paso del ejecutor con un lanzador vivo descendiente de una sesión simulada no quedan atribuidos a ella; el evento sale sin actor o como ajeno.
- **NFR-01**: `undo --agent` no deshace un commit del propio motor ni deja de deshacer uno del agente (corpus de TD-GRP-007).
- **SEC-04**: ninguna ruta, nombre, pid ni argv nuevo en los contadores de `s3_evidence`.
- **Manual**: en macOS, arrancar el daemon desde un `raptor` lanzado por un agente real de Claude Code y deshacer; comprobar que el registro de atribución no nombra la sesión.
- Linux y Windows: **Pendiente: etapa de validación multiplataforma**.
