---
id: US-TMC-012
title: "Un undo nunca sobrescribe trabajo posterior de otro actor"
type: us
status: implemented
priority: high
created: 2026-10-03
updated: 2026-10-29
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-002, US-TMC-006]
  specs: [DS-US-TMC-012]
covers: [BR-TMC-CONS-005, D-TMC-13, D-TMC-27]
blocked_by: []
tags: [time-machine, undo, solape]
---

# US-TMC-012: Un undo nunca sobrescribe trabajo posterior de otro actor

## Descripción

**Como** desarrollador orquestador
**Quiero** que un undo se detenga y me muestre el solape cuando tocaría cambios posteriores de otro actor en los mismos archivos o ramas
**Para** que recuperar lo de uno nunca destruya el trabajo de otro

**Valor**: la red de seguridad no puede causar pérdida de datos (NFR-01), ni siquiera al deshacer.

## Reglas cubiertas

BR-TMC-CONS-005 (solape; alcance del MVP: sin continuar, por archivo y por rama, ediciones sin Git como "sin agente") · D-TMC-13 · D-TMC-27 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: El undo se detiene ante un solape**

Dado que "claude-1" hizo a las 10:00 una operación que cambió "api.rs"
  Y a las 10:05 se vio un cambio "sin atribuir" en otra función de "api.rs"
Cuando se pide deshacer la operación de "claude-1" de las 10:00
Entonces el undo se detiene sin cambiar el repo
  Y el solicitante recibe los dos cambios en conflicto con su actor y el momento en que se vio cada uno

**Escenario: Sin solape, el undo procede**

Dado que el cambio posterior de otro actor está en otro archivo
Cuando se pide deshacer la operación de "claude-1" de las 10:00
Entonces la operación de "claude-1" se deshace
  Y el cambio del otro actor sigue intacto

**Escenario: Tras un solape no se puede continuar**

Dado un undo detenido por solape
Cuando el desarrollador revisa el resultado
Entonces el repo queda exactamente como antes de pedir el undo
  Y no se le ofrece continuar a pesar del solape

**Escenario: Solo cuentan los cambios posteriores**

Dado que un cambio "sin atribuir" modificó "api.rs" a las 09:55
  Y "claude-1" hizo a las 10:00 una operación que cambió "api.rs"
Cuando se pide deshacer la operación de "claude-1" de las 10:00
Entonces la operación de "claude-1" se deshace
  Y "api.rs" queda como lo dejó el cambio de las 09:55

**Escenario: Una edición posterior sin Git cuenta como de otro actor, aunque la haga el mismo agente**

Dado que "claude-1" hizo a las 10:00 una operación que cambió "api.rs"
  Y después "claude-1" editó "api.rs" sin usar Git
Cuando "claude-1" pide deshacer su operación de las 10:00
Entonces el undo se detiene sin cambiar el repo
  Y el cambio posterior se presenta como "sin agente"

## Requisitos Técnicos

- Dueña de la detección de solape por archivo y por ref (ADR-TMC-005 § 5): cambios posteriores de otro actor según diferencias de árbol entre capturas y la atribución vigente de los eventos del intervalo.
- Atribución mixta en un intervalo cuenta como otro actor; los cambios anteriores no cuentan.
- Resultado: operación `rechazada` por solape con los cambios en conflicto (actor y momento); granularidad por archivo y por ref en el MVP (por fragmento, más adelante).
- Una ruta que solo cambió otro actor después se conserva y se informa.
- Sin poder separar el cambio deshecho de lo posterior, se detiene con `overlap-unverified`.

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** [DS-US-TMC-012](../dev-specs/US-TMC-012-solape-otro-actor.md)

## Dependencias

- **Historias**: US-TMC-002, US-TMC-006.
- **Externas**: ninguna. Las opciones que se ofrecen tras detenerse se precisan en el diseño (design-flow).
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.

## Estado de la implementación (2026-10-29)

Implementado en: PR #316 (**sin certificar**: la línea base de `/implement` no se pudo medir por la carga de la máquina; decisión del coordinador).

- **Hecho:** detección del solape en dos fases (DS-US-TMC-012 T001–T007). Fase 1 en el plan del undo, antes de la comprobación de ref en uso, y fase 2 bajo el bloqueo del repo, justo antes de escribir. Se comparan los árboles entre capturas (`StoreHandle::diff_trees`, `SnapshotStore::changed_between`) y se usa la atribución vigente de los eventos del intervalo. Cada ruta y cada rama queda como `conflict` (el undo se detiene con `overlap`), `kept` (el aplicador no la toca y se informa) o `written`. Si algo no se puede verificar (no hay base, un evento ilegible, se agota el plazo de 1,5 s o se supera el tope de rutas), el undo se detiene con `overlap-unverified`. El detalle solo llega a los clientes con la capacidad `timemachine.overlap`, nunca por MCP: el MCP recibe el rechazo y el número de rutas conservadas.
- **Tests:** los 20 criterios del contrato `docs/dev-briefs/us-tmc-012-overlap-v2.contract.json` (`crates/core/tests/us_tmc_012.rs`, `tm_apply_keep.rs`, `overlap_tests.rs`, `changed_between_tests.rs`, `diff_trees_tests.rs`, `timemachine_overlap_tests.rs`, `apps/cli/src/undo/overlap_tests.rs` y el e2e `apps/cli/tests/undo_overlap_process.rs`, solo en macOS). Además, los tests de endurecimiento de la ronda de revisión: `overlap_hardening_tests.rs`, `undo_raw_events_tests.rs`, `tm_overlap_hardening.rs`, `diff_trees_bounds_tests.rs`, `conn_kept_total_tests.rs` y `overlap_kept_total_tests.rs`.
- **Decisiones del orquestador (2026-10-28), validadas por la revisión de código y de seguridad (opus):**
  - `UndoError::Overlap` lleva el informe en `Box<OverlapReport>`.
  - `is_clean` por punto pasó a ser `clean_mask(points, event_seqs, floor)`, porque recalcular las ventanas en cada punto era demasiado lento con 1.000 puntos. `before_event` usa el mismo cálculo y da los mismos resultados.
  - El core expone `overlap::local_utc_offset_s()` para la CLI, sin dependencias nuevas.
  - En español, el texto dice "reemplazaría" en vez de "sobrescribiría".
  - La base B debe ser de la generación actual. Si faltan eventos con una referencia sin atribuir, el resultado es `unverified`. El aplicador conserva las etapas de conflicto del previo en las rutas `kept`. El total real de `kept` va en `kept_total`.
  - Los criterios SAME-ACTOR y SAME-SESSION se retiraron del contrato. Sus tests siguen como regresión fuera de los congelados; el e2e de la misma sesión está en `apps/cli/tests/undo_overlap_same_session.rs`.
- **Arreglo encontrado al probar (fuera del solape, necesario para él):** la cabeza del oplog (`oplog/chain.rs`, `write_head`) usaba un único archivo temporal compartido. Dos escritores concurrentes (el daemon y un hook) se pisaban: uno fallaba después de confirmar sus filas y una captura quedaba `Pending`, así que la base B del solape no era utilizable y el undo salía `overlap-unverified` de forma intermitente. Ahora cada escritura usa su propio temporal y la cabeza nunca retrocede (lock exclusivo `oplog.head.lock`). Lo revisó opus.
- **Pendiente:** cachear por segmento las diferencias de ramas (revisión L2). Con eventos incompletos y una referencia de agente, un movimiento de otro actor sobre una rama que la referencia también movió en el mismo segmento puede no detectarse (riesgo abierto de M-02). Restaurar un objetivo con entradas de conflicto propias puede fallar con "bad object", por un código previo a esta historia. El presupuesto con 1.000 puntos no está medido. El solape de la restauración es US-TMC-024. Linux y Windows están en XP-66.
