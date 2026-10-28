---
id: DS-US-CKP-014
title: "Dev Spec — US-CKP-014: integrar la rama de un agente con una tecla y poder deshacerlo"
type: dev-spec
status: approved
created: 2026-10-27
updated: 2026-10-27
story: US-CKP-014
feature: cockpit
domain: GRP
scope: fullstack
frontend_surface: true
stack: rust
profile: fullstack-feature
tooling: [cargo]
related:
  context: ../context.md
  story: ../user-stories/US-CKP-014-integrar-rama-agente.md
  adrs: [ADR-CKP-002, ADR-CKP-003, ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-016, ADR-TMC-002, ADR-TMC-004, ADR-TMC-005, ADR-GRD-002, ADR-GRD-003]
  api_spec: null
  design_spec: null
  contracts: [./TS-CKP-002-catalogo-ejecutor.md#2-contrato-protocolo-4, ./TS-CKP-003-capa-cockpit-guardrails.md#tipos-y-datos-compartidos]
must_read:
  - ../user-stories/US-CKP-014-integrar-rama-agente.md
  - ../user-stories/US-CKP-024-integrar-casos-limite.md
  - ../business-rules.md
  - ./TS-CKP-002-catalogo-ejecutor.md
  - ./TS-CKP-003-capa-cockpit-guardrails.md
  - ./US-CKP-002-orden-atencion-terminadas.md
  - ../../../../architecture/decisions/ADR-CKP-002-catalogo-operaciones-ejecutor.md
  - ../../../../architecture/decisions/ADR-CKP-003-arquitectura-tui.md
  - ../../../../architecture/extender-sin-archivos-compartidos.md
  - ../../../../design-system/README.md
  - ../../../../../crates/api/src/catalog.rs
  - ../../../../../crates/api/src/methods/operation.rs
  - ../../../../../crates/api/src/timemachine.rs
  - ../../../../../crates/core/src/executor/mod.rs
  - ../../../../../crates/core/src/executor/facts.rs
  - ../../../../../crates/core/src/executor/git.rs
  - ../../../../../crates/core/src/executor/ops/mod.rs
  - ../../../../../crates/core/src/executor/ops/snapshot.rs
  - ../../../../../crates/core/src/executor/ops/test_arms.rs
  - ../../../../../crates/core/src/timemachine/protected/mod.rs
  - ../../../../../crates/core/src/timemachine/protected/backend.rs
  - ../../../../../crates/core/src/channel/marks.rs
  - ../../../../../crates/core/src/channel/requester.rs
  - ../../../../../crates/git/src/preflight.rs
  - ../../../../../crates/policy/src/guard/fastpath.rs
  - ../../../../../crates/core/src/daemon/serve.rs
  - ../../../../../crates/core/src/observe.rs
  - ../../../../../crates/git/src/user_ops.rs
  - ../../../../../crates/git/src/reader.rs
  - ../../../../../crates/policy/src/guard/plan.rs
  - ../../../../../apps/cli/src/model.rs
  - ../../../../../apps/cli/src/client/mod.rs
  - ../../../../../apps/cli/src/tui/update.rs
  - ../../../../../apps/cli/src/tui/keymap.rs
  - ../../../../../apps/cli/src/tui/widgets/confirm.rs
  - ../../../../../apps/cli/src/tui/widgets/notification.rs
  - ../../../../../apps/cli/tests/live_fleet.rs
lineage:
  supersedes: []
  superseded_by: []
  migration_adr: null
  migration_guide: null
constitution_gates: []
validation:
  must_read_resolved: true
  gaps_blocking: 0
  ready_to_implement: true
  gaps_release: 1
  ready_to_release: false
tags: [cockpit, tui, merge, merge-into-base, operacion-protegida, esqueleto-escritura, deshacer, ejecutor, guardrails, nfr-01, nfr-02]
---

# DS-US-CKP-014 · Integrar la rama de un agente con una tecla y poder deshacerlo

## Contexto rápido

Al terminar, el desarrollador selecciona la fila de un agente, pulsa `m`, ve el plan "integrar feat-pagos (3 commits de claude-1) en main", lo confirma y la rama queda integrada en local en el worktree que tiene la base sacada. Un toast ofrece Deshacer durante 5 s con `u`. Si una precondición falla, "Integrar" aparece desactivado con el motivo y la acción que lo desbloquea. Sin snapshot previo no se integra nada, y nunca se toca el remoto.

Es la primera operación gobernada que lanza `git` en producción y fija el esqueleto de toda escritura del Cockpit: preparar → confirmar el plan → ejecutar → Deshacer. Lo reutilizan US-CKP-015, 017, 018 y 024.

Hoy:
- El catálogo, el ejecutor, la cola, la revalidación bajo el cerrojo, la puerta de Guardrails con capa `cockpit` y el snapshot previo están en `main` (DS-TS-CKP-002, DS-TS-CKP-003).
- `merge-into-base` está en el catálogo (`crates/api/src/catalog.rs:59`, `NoArgs`, gobernada como `Merge`), pero el catálogo de producción solo arma `snapshot` (`crates/core/src/executor/ops/mod.rs:23`, `ARMS`). `operation.prepare` de un merge responde `-32004` con `implemented_by = US-CKP-014`.
- El brazo de prueba de `test_arms.rs` solo hace avance rápido de la ref y no es lógica de producto (DS-TS-CKP-003 D14).
- La TUI no tiene ninguna vía de escritura: ni `operation.prepare`, ni ConfirmPrompt de operaciones, ni Deshacer.

Sin `architecture-constitution.md` en la cascada: las restricciones activas salen de [AGENTS.md](../../../../../AGENTS.md) y de los ADR citados (`unsafe_code = forbid` y escritura con la Git CLI del sistema, ADR-GRP-001; protocolo congelado en 9 y códigos en `-32016`, ADR-GRP-016; argv fijo y sin shell, NFR-02). Ninguna tarea las viola.

### Decisiones

Todas son **Decisión del orquestador (2026-10-27), validada por Arquitecto**. Validadas por PO el 2026-10-27: cambian I6 e I11, y se precisa la regla de la sesión Activa (T003 regla 8, T006). Ver Q-CKP-36 a Q-CKP-38.

**Revisión de arquitectura (2026-10-27)**, contra el código de `main`. Lo ratificado y lo corregido queda como **Decisión del orquestador (2026-10-27), validada por Arquitecto (revisión)**:
- Ratificadas sin cambios: I2 a I7 e I10 a I13. Los tipos y métodos que se citan como existentes están en `main` con ese nombre.
- I9 ratificada con otra justificación: la barrera ya está implementada y ADR-GRP-001 admite un crate con `unsafe` confinado. Se añaden los disparadores de la enmienda pendiente y un test con Git real.
- I1, I8 e I14 corregidas o añadidas: cómo se adjunta el contexto, contra qué se clasifica el desenlace y por dónde viaja `MergePlanView`.
- I15 añadida: el `git_dir` del destino.
- T003, regla 5: la base en rebase o bisect cuenta como sacada.

| ID | Decisión | Por qué / descartado |
|----|----------|----------------------|
| I1 | **Contexto de las operaciones adjunto al montar el canal.** El trait `OperationCatalog` (`timemachine/protected/backend.rs`) gana `fn attach(&self, ctx: OpsContext) {}`, con implementación por defecto vacía como `armed()`. `ProductionCatalog` guarda el contexto en un `OnceLock<OpsContext>`. `serve_channel` (`daemon/serve.rs`, junto a `ops.gate.attach`) llama una vez a `ops.catalog.attach(..)` con el Git resuelto del arranque (`report.git`), el `HOME` del daemon y un `EngineFacts` (base confirmada y sesiones presentes por worktree). Sin contexto (también sin Git resuelto), el brazo de merge responde `PlanError::NotImplemented` (`-32004`, lo de hoy) | El brazo necesita estado del motor que `plan_op(op, args, repo, facts)` no recibe, y el binario de Git resuelto, que `StepPlan` no lleva (DS-TS-CKP-002 § 9 lo dejaba pendiente). Descartados: ampliar la firma de `plan_op`, que obliga a tocar todos los backends de prueba, y un campo nuevo en `OperationsWiring`, que rompe sus 12 construcciones literales en los tests. Con un método por defecto no cambia ningún backend ni ningún test. Corregido en la revisión de arquitectura (2026-10-27) |
| I2 | **Capacidad `operation.merge`** en `methods/operation.rs`. Solo con ella el daemon prepara `merge-into-base`, devuelve `PrepareResult.merge` y los motivos nuevos de I3. Sin ella, `-32004` con `implemented_by = US-CKP-014`, como hoy | Es un cambio de forma de un método que ya existe (ADR-GRP-016, "Añadir una capacidad"). Un cliente viejo sigue viendo lo que veía |
| I3 | **Seis motivos nuevos en `RejectReason`**, aditivos y solo para conexiones con `operation.merge`: `base-unconfirmed`, `main-worktree`, `nothing-to-integrate`, `target-dirty`, `target-session-present` y `base-not-checked-out`. "Operación en curso" en el destino reutiliza `operation-in-progress` | La TUI necesita el motivo exacto para el texto y la acción que desbloquea. Patrón de `ConfirmationUnavailable` y `WriteInProgress` |
| I4 | **Trabajo afectado = `Affected::Other` siempre.** Sin la atribución commit→evento (DEP-CKP-2) el daemon no sabe de quién son los commits que entran, y ADR-CKP-002 § 3 manda contarlo como otro actor. Con capa `cockpit`, eso da el reto ligado a la huella: es la confirmación del plan del camino feliz (BR-CKP-AUTH-003). En Windows, `confirmation-unavailable` | Es lo conservador y coincide con la historia ("pide confirmarlo"). Descartado: inferir el actor de la sesión del worktree, que atribuiría a un agente commits de otro |
| I5 | **El oid integrado es el HEAD del worktree del agente al preparar.** `merge-into-base` sigue sin argumentos. La huella incluye ese HEAD, la punta de la base y el HEAD del destino, así que un commit entre preparar y ejecutar da `state-changed` (lo prueba US-CKP-024). El prompt muestra el plan del daemon (`MergePlanView`), no lo que la TUI creía ver | El usuario confirma exactamente lo que se ejecuta ("hasta el commit a1b2c3"). Descartado: un argumento `expected_head`, que duplica la huella |
| I6 | **Destino limpio = sin cambios en archivos seguidos**: nada preparado, modificado, borrado ni en conflicto. Los archivos sin seguimiento del destino **no** bloquean (Q-CKP-37, validada por PO). El worktree del agente no necesita estar limpio (BR-CKP-ELIG-001): sus cambios sin commitear no entran, porque se integra un oid | Un archivo sin seguimiento no se mezcla con el merge: si el merge fuera a sobrescribirlo, Git se niega y el desenlace es `failed-unchanged`; además, el snapshot previo lo guarda. Lectura fijada (Decisión del orquestador (2026-10-27), validada por Arquitecto): destino sucio ⇔ `status.staged` o `status.unstaged` no vacíos (`Status { staged, unstaged, untracked }`, `crates/git/src/reader.rs`), con el método a crear `Status::has_tracked_changes()` junto a `is_clean` en ese mismo archivo (no está en la tabla de archivos compartidos de ADR-GRP-016). `untracked` no se lee. Los conflictos ya caen antes en `operation-in-progress`. La TUI (T004 regla 7) aplica el mismo criterio con los recuentos de la instantánea sin los sin seguimiento |
| I7 | **Mensaje de merge con plantilla fija en inglés**: `Merge branch '<rama>' into <base>`, o `Merge commit <oid12> into <base>` con HEAD separado. Los nombres pasan por `RefName` y viajan como argumento de `-m`, nunca por un shell | ADR-CKP-002 § 6 (L-01): plantilla fija con el nombre validado. Es el formato por defecto de Git |
| I8 | **Desenlace leído después de `git`**, en `merge.rs`. Antes de lanzar `git`, ya con el snapshot previo tomado, el paso lee la punta de la base, el HEAD del destino y su estado (`before`). Después: éxito → `done`; fallo con merge en curso en el destino → `stopped`; fallo con los tres iguales a `before` → `failed-unchanged`; cualquier otro fallo → `failed-changed` (Deshacer). La salida de Git va a `StepOutput.git_output` | ADR-CKP-002 § 2, paso 5: `failed-unchanged` es "el ámbito es igual al previo". Si se comparara con el plan, un cambio ajeno entre el cerrojo y el lanzamiento saldría como `failed-changed` y Deshacer revertiría el trabajo de otro. `stopped` se presenta con el texto genérico hasta US-CKP-016, que pone Abortar y el editor. Corregido en la revisión de arquitectura (2026-10-27) |
| I9 | **Barrera de arranque del lado del lector** para este brazo; el *spawn* suspendido de ADR-CKP-002 § 4 queda aplazado. La barrera ya está en `main`: `spawn_registered` (`timemachine/protected/mod.rs`) abre el *spawn* pendiente antes de lanzar, marca el hijo con su identidad y las transiciones del plan en la misma sección crítica, y mata al hijo si su identidad no se lee. Un llamante que desciende del daemon sin marca espera el registro hasta 2 s; pasado ese tope, su identidad no se verifica y se rechaza (`channel/requester.rs`). El hook falla y Git falla. Con esta barrera el hook **puede haber corrido** antes de que falle el registro, así que el desenlace se clasifica por el estado real (I8: `failed-unchanged` o `failed-changed`), con el snapshot previo intacto; nunca se fija `failed-unchanged` por suponer que no corrió. El fin de la espera deja una marca de auditoría propia, `barrier-timeout`, distinta de `IdentityUnverified`. Vale **solo para la capa `cockpit`**. En Windows el merge no se libera hasta la validación multiplataforma de la barrera; mientras, falla cerrado. Falla cerrado | Frente al *confused deputy* (DEP-MCP-3, R-MCP-1) da la misma garantía en el canal: ninguna petición de un descendiente del `git` se resuelve antes de la marca, y el rechazo de lo reservado a todo descendiente del daemon (`daemon-descendant`, TS-GRP-004 D21) va por ascendencia, no por la marca. El *spawn* suspendido solo añade que ningún código del hook corra antes del registro. Ese código actúa con la autoridad del propio usuario sobre sus archivos, y lo cubre el snapshot previo (NFR-01). El riesgo residual (nieto que cambia de grupo de procesos) es el mismo con las dos barreras (ADR-GRP-005 § 6). El coste de la barrera del lector es de disponibilidad: con un registro de más de 2 s, el merge falla cerrado. ADR-GRP-001 no lo prohíbe de forma absoluta: ADR-CKP-002 § 4 admitía "una excepción auditada o un crate auditado", y ese crate existe (`crates/macsys`, `unsafe` confinado, Enmienda 2026-10-05 de ADR-GRP-002). Se aplaza por coste y beneficio, no porque esté prohibido. **Decisión del orquestador (2026-10-27), validada por Arquitecto (revisión) y security-expert, con condiciones** (Q-CKP-35 cerrada). La historia no pasa a `implemented` sin la enmienda de ADR-CKP-002 aplicada ([Enmiendas que implica](#enmiendas-que-implica)) |
| I10 | **Flujo de escritura genérico en la TUI**: `tui/write_flow.rs` con la máquina de estados de ADR-CKP-003 § 2 (`Inactivo → Preparando → Confirmando(plan) → Ejecutando → Hecha(deshacer) | Rechazada(motivo) | Detenida | Fallida`). El canal gana **una** variante genérica en cada sentido: `LinkCmd::Operation(OpRequest)` y `EngineMsg::Operation(OpReply)`. Lo propio del merge vive en `tui/merge.rs` | US-CKP-015, 017 y 018 añaden su archivo y no tocan `client/mod.rs` ni `model.rs` (ADR-GRP-016) |
| I11 | **Teclas**: `m` integra y `u` deshace mientras el toast está visible. `u` ya la fijan el design system (Notification, "u: deshacer") y BR-CKP-ELIG-004; `m` la fija esta DS (validada por PO). La salida de Git del toast se abre con `o`, no con `?`, que es la Ayuda | `r` ya es Reintentar (`keymap.rs`), así que el rebase futuro necesitará otra tecla. La tabla única de `keymap.rs` alimenta la ayuda y `KeyHints` |
| I12 | **Deshacer desde la TUI** = `timemachine.undo {worktree: raíz del worktree del agente, operation_id}` con el `operation_id` del resultado. El oplog guarda el ámbito con el destino y la ref de la base, así que Deshacer restaura los dos. Si el daemon pide una confirmación de la Time Machine, la TUI muestra el motivo y remite a `raptor undo`; después de los 5 s, Deshacer vive en el historial (US-CKP-021) | Contrato que esta historia fija para US-CKP-021. La confirmación de trabajo ajeno del undo en la TUI no es de esta historia |
| I13 | **El actor del prompt es el de la fila** (su sesión presente o la última), con la presentación de BR-CKP-CONS-003; el recuento de commits viene del daemon | El contrato no lleva el actor por commit (I4) |
| I14 | **`MergePlanView` viaja dentro del plan.** `plan_op` lo serializa en `OpPlan.expected["view"]`. `Prepared` (`executor/mod.rs`) gana `view: Option<serde_json::Value>`, copiado de `expected["view"]`. `conn.rs` lo convierte en `PrepareResult.merge` solo para `merge-into-base` y una conexión con `operation.merge` | `plan_op` devuelve un `OpPlan` y `PrepareResult` se arma en `conn.rs` desde `Prepared`: la DS no decía por dónde pasaba la vista. Un campo nuevo en `OpPlan` rompería sus construcciones literales en 19 archivos. La vista sale de datos que ya están en la huella, así que no cambia lo que la huella compara. `view` es genérico y lo reutilizan US-CKP-015, 017 y 018. Añadido en la revisión de arquitectura (2026-10-27) |
| I15 | **El destino es el `git_dir` propio del worktree que tiene la base sacada**: `RepoTarget { git_dir: RepoFacts::git_dir del destino, work_tree: Some(raíz del destino) }`. El directorio común solo cuando el destino es el worktree principal, porque ahí coinciden | `crates/git` lanza `--git-dir=<git_dir> --work-tree=<raíz>`. Con el directorio común y un destino enlazado, Git usaría el HEAD y el índice del worktree principal e integraría en otra rama. Corregido en la revisión de arquitectura (2026-10-27) |

## ⚠️ Gaps y violaciones de la constitución

| ID | Qué falta | Severidad | Acción | Owner |
|----|-----------|-----------|--------|-------|
| G1 | US-TMC-019: el Deshacer desde la TUI debe entregar los avisos pendientes de `timemachine.notices` del worktree (deuda anotada en la historia el 2026-10-09). Comprobado en la revisión (2026-10-27): `UndoResult` no lleva esos avisos, solo `warnings: Vec<String>` del aplicador (`crates/api/src/timemachine.rs:362`). Los pendientes salen de `timemachine.notices` (`NoticesParams`/`NoticesResult`, `timemachine.rs:947`), que ya existe. Esta DS muestra los `warnings` del resultado; pedir `timemachine.notices` después del Deshacer es de US-TMC-019 | Release | Cerrar con US-TMC-019 | Orquestador |
| ~~G2~~ | **Cerrado.** **Decisión del orquestador (2026-10-27), validada por Arquitecto (revisión) y security-expert, con condiciones**: la barrera del lector basta para la capa `cockpit` con las condiciones de I9 y los tests de § 9.4. El *spawn* suspendido queda como mejora con disparadores (a), (b) y (c) | No bloqueante para implementar; la historia no pasa a `implemented` sin la enmienda de ADR-CKP-002 aplicada | Enmienda en el PR de la historia | Orquestador |

Ninguna violación: sin constitución, ver Contexto rápido.

## 🔭 La forma

```mermaid
sequenceDiagram
    autonumber
    participant T as TUI (update + write_flow)
    participant C as hilo del canal
    participant D as daemon (executor)
    participant M as ops/merge.rs
    participant TM as Time Machine
    participant G as git (hijo directo)
    T->>T: m sobre la fila: merge_eligibility (T004)
    T->>C: LinkCmd::Operation(Prepare merge-into-base, worktree = agente) (T005)
    C->>D: operation.prepare (+ session_env)
    D->>M: plan_op: base confirmada, destino, limpieza, sesiones, oid (T002)
    D-->>C: PrepareResult {warnings, challenge, merge: MergePlanView}
    C->>T: EngineMsg::Operation(Prepared)
    T->>T: ConfirmPrompt con el plan (default No) (T006)
    T->>C: LinkCmd::Operation(Run {plan_id, accepted_warnings, confirmation})
    C->>D: operation.run
    D->>D: cola, cerrojo, plan rehecho, huella, Guardrails (la que cuenta)
    D->>TM: intención + snapshot previo garantizado (falla → aborted)
    D->>M: step: git merge --no-edit -m <plantilla> <oid> en el destino (T003)
    M->>G: run_git (barrera, transición MergeInto ya registrada)
    G-->>M: salida; M lee el desenlace (I8)
    D-->>C: OperationRunResult {operation_id, outcome}
    C->>T: Hecha → toast "Integrado feat-pagos en main · u Deshacer" (5 s)
    T->>C: u → LinkCmd::Operation(Undo {worktree, operation_id}) (T007)
    C->>D: timemachine.undo
```

---

## 🚀 Plan de implementación

### El trabajo de un vistazo

| # | Tarea | Depende | Aterriza en |
|---|---|---|---|
| T001 | Escribir en rojo los tests de contrato del daemon y de la TUI | — | `crates/core/tests/us_ckp_014_merge.rs`, `apps/cli/tests/tui_merge.rs`, stubs |
| T002 | Crear el contrato (capacidad, motivos, `MergePlanView`) y el contexto adjunto | T001 | `crates/api/src/merge.rs`, `catalog.rs`, `methods/operation.rs`, `executor/ops/context.rs`, `daemon/operation_facts.rs`, `daemon/serve.rs` |
| T003 | Crear el brazo `merge-into-base`: plan y paso con `git merge` en el destino | T002 | `crates/core/src/executor/ops/merge.rs`, `ops/mod.rs`, `channel/conn.rs` |
| T004 | Crear la elegibilidad de "Integrar" en la TUI | T002 | `apps/cli/src/tui/merge.rs` |
| T005 | Crear el flujo de escritura genérico y su variante del canal | T002 | `apps/cli/src/tui/write_flow.rs`, `client/mod.rs`, `model.rs` |
| T006 | Pintar la acción, el ConfirmPrompt del plan y el resultado, con textos en/es | T004, T005 | `tui/update.rs`, `tui/view.rs`, `tui/keymap.rs`, `tui/help.rs`, `present/write_text.rs`, `present/merge_text.rs` |
| T007 | Añadir Deshacer desde el toast | T005 | `apps/cli/src/tui/undo.rs` |
| T008 | Cerrar la historia | T001–T007 | `docs` |

T003 y T004–T007 van en paralelo una vez está T002.

### T001 — Escribir en rojo los tests de contrato

**Objetivo.** Los seis escenarios de la historia, en rojo, contra el daemon real con repos y perfil temporales (NFR-01) y contra la `App` real sobre `TestBackend`.

**Ubicación.**
- `crates/core/tests/us_ckp_014_merge.rs` (**CREATE**): daemon en proceso, `ProtectedWiring` de producción con `OperationsWiring::production()` y el contexto adjunto, Git real del sistema.
- `apps/cli/tests/tui_merge.rs` (**CREATE**): el arnés de `apps/cli/tests/live_fleet.rs` (`#![cfg(all(target_os = "macos", debug_assertions))]`, `gitraptor_testkit::Fixture`, `raptor` de debug, `raptor-fake-agent`).
- Stubs que compilan (`todo!()` o `PlanError::NotImplemented`) en los archivos de T002 a T007, para que los tests compilen.

**Reglas**
- Los tests rojos van en su archivo propio y no se editan durante la implementación (regla R4 de `/implement`).
- Nunca este repo ni el perfil real. `GITRAPTOR_PROFILE_DIR` solo en depuración.
- La capa `cockpit` sale de la resolución real del solicitante, nunca de `test_layer_override` en los tests de punta a punta de la TUI.

**Criterios de aceptación de la tarea:** los tests de la tabla de [§ 9.4](#94-comportamientos-críticos-verificados) existen, compilan y fallan por la razón esperada.

### T002 — Crear el contrato y el contexto adjunto

**Objetivo.** El daemon sabe preparar un merge para una conexión que lo entiende, con el estado del motor a mano.

**Ubicación.**
- `crates/api/src/merge.rs` (**CREATE**): `MergePlanView`, `MergeTargetView`. Una línea `pub mod merge;` en `crates/api/src/lib.rs` (registro de módulo; no toca `PROTOCOL_VERSION`).
- `crates/api/src/catalog.rs` (**MODIFY**): los seis motivos de I3 en `RejectReason`, con su doc; `PrepareResult.merge: Option<MergePlanView>` con `#[serde(default, skip_serializing_if = "Option::is_none")]`, como `guardrails`. `McpPrepareResult` no cambia: el merge no tiene marca MCP.
- `crates/api/src/methods/operation.rs` (**MODIFY**): `CAP_OPERATION_MERGE = Capability::new("operation.merge")` en el `GROUP`.
- `crates/core/src/executor/ops/context.rs` (**CREATE**): `OpsContext` y el trait `EngineFacts`.
- `crates/core/src/daemon/operation_facts.rs` (**CREATE**): `DaemonEngineFacts`, la implementación de producción. Una línea `mod operation_facts;` en `daemon/mod.rs`.
- `crates/core/src/timemachine/protected/backend.rs` (**MODIFY**): `OperationCatalog::attach` con cuerpo por defecto vacío (I1).
- `crates/core/src/daemon/serve.rs` (**MODIFY**): una llamada `ops.catalog.attach(..)` junto a `ops.gate.attach`.
- `crates/core/src/executor/mod.rs` (**MODIFY**): `Prepared.view` copiado de `op_plan.expected["view"]` (I14). `Prepared` solo se construye en `prepare`.
- Cliente: el cliente de `crates/core` pide `operation.merge` en `connection.accept` (ADR-GRP-016: toda capacidad que conoce).

**Reglas**
- `EngineFacts::base(repo_id)` devuelve `BaseBranch` con la misma fuente que `repo_base` (`daemon/mod.rs:1293`): `RepoStore::confirmed_team_baseline` y `observe::base_branch`. Solo `BaseStatus::Confirmed` habilita el merge.
- `EngineFacts::present_sessions(repo_id, worktree_root)` devuelve las sesiones presentes (Activas e Inactivas, con su estado) de ese worktree, de la misma fuente que `sessions.list` (`daemon/sessions.rs:853`). **A crear**: el accesor sobre el detector, si no existe uno que dé el estado; nunca una segunda detección.
- `EngineFacts` no hace E/S de Git: lo que se lee del repo se lee en el brazo con la capa de lectura.
- Sin contexto adjunto, `ProductionCatalog::plan_op(MergeIntoBase, …)` devuelve `PlanError::NotImplemented`.
- `channel/conn.rs`: `operation.prepare` de `merge-into-base` en una conexión sin `operation.merge` responde `-32004` con `implemented_by = US-CKP-014` antes de llamar al ejecutor; los motivos de I3 nunca llegan a esa conexión.

**Criterios de aceptación de la tarea**
- [ ] `describe` y el esquema de `RejectReason` incluyen los motivos nuevos; `crates/api/tests/architecture.rs` sigue en verde (protocolo 9, sin códigos nuevos).
- [ ] Un cliente sin la capacidad recibe `-32004` con `implemented_by = US-CKP-014` (test en `conn.rs`).

### T003 — Crear el brazo `merge-into-base`

**Objetivo.** El ejecutor de producción integra un oid en el worktree que tiene la base sacada, dentro de la operación protegida.

**Ubicación.**
- `crates/core/src/executor/ops/merge.rs` (**CREATE**): `plan_op`, `MergeStep` (`ProtectedStep`) y `classify_after`.
- `crates/core/src/executor/ops/mod.rs` (**MODIFY**): `OperationId::MergeIntoBase` en `ARMS` y su brazo en `plan_op` y en `step` (hoy `step` solo despacha los brazos de prueba: se añade el despacho de los brazos reales antes de ellos).

**Reglas de `plan_op`** (orden fijo; lo común ya pasó por `check_common` sobre el worktree del agente: operación en curso, locks, worktree bloqueado, rama sacada en otro worktree, grafts)
1. Contexto adjunto, o `NotImplemented`.
2. El worktree pedido no es el principal (`facts.linked == false` → `main-worktree`).
3. Base: `EngineFacts::base`; si no es `Confirmed` o no tiene nombre → `base-unconfirmed`. La punta se lee de `refs/heads/<base>` en el directorio común.
4. Oid integrado = `facts.head_commit`. Si la rama del agente es la base, o el oid ya es ancestro de la punta de la base (`RepoReader::merge_bases`) → `nothing-to-integrate`.
5. Destino = el único worktree registrado (`RepoReader::worktrees` más el principal) que tiene la base sacada: su HEAD es `refs/heads/<base>`, **o** está rebasando o bisecando la base (`rebase-merge/head-name`, `rebase-apply/head-name` o `BISECT_START` de su `git_dir` = `refs/heads/<base>`), como hace Git en `branch -f`. **A crear**: esa lectura en la capa de lectura de `crates/git`, porque hoy `branches_elsewhere` (`preflight.rs`) solo lee `HEAD` y no ve una base en rebase, que tiene el HEAD separado. Se abre **solo** con `observe::open_registered_worktree(common_dir, raíz)` (#223 I-03) y se leen sus hechos con `RepoFacts::read`. Sin destino → `base-not-checked-out` (US-CKP-024 añade el avance rápido sin worktree).
6. Destino: operación en curso (incluida la base en rebase o bisect de la regla 5) → `operation-in-progress`; locks de Git → `git-busy`; `status.has_tracked_changes()` del destino (I6: `!staged.is_empty() || !unstaged.is_empty()`; `untracked` no cuenta) → `target-dirty`; `EngineFacts::present_sessions` no vacío → `target-session-present`.
7. Recuento: `RepoReader::ahead_behind_commits` del oid frente a la punta, acotado (`Count::AtLeast` → `commits_capped = true`).
8. `OpPlan`:
   - `expected`: `{base: "refs/heads/<base>", base_old, integrated, target: {root, head, root_id, dot_git_id, gitdir_linked_back}, mode: "merge", commits}`. Entra en la huella: la identidad del destino se revalida bajo el cerrojo al rehacer el plan.
   - `warnings`: vacío en esta historia (⚡ es de US-CKP-024). Una sesión Activa en el worktree del agente da `ActiveSession` por la vía que ya existe (`other_session = true`; ver regla siguiente).
   - `other_session`: `EngineFacts::present_sessions` del worktree del agente no vacío. Con capa `cockpit` el ejecutor lo convierte en el aviso `active-session` (`executor/mod.rs:580`). El aviso se pone con sesión Activa **o** Inactiva, aunque BR-CKP-ELIG-001 solo pide un aviso visible con la Activa. La TUI pinta "el agente sigue activo; se integra hasta el commit X" si alguna sesión del worktree está Activa (con worktree compartido manda la más restrictiva, BR-CKP-EDGE-004). Si solo hay sesiones Inactivas, acepta el código sin texto propio. No hay código nuevo (Q-CKP-38, validada por PO).
   - `affected`: `Affected::Other` (I4).
   - `governed`: `Some(PlanShape::Merge { base, old: base_old, integrated })`.
9. `MergePlanView` para `PrepareResult.merge`, serializado en `expected["view"]` (I14): nombre corto de la base, rama del agente (`None` con HEAD separado), oid completo, recuento y `MergeTargetView::Worktree { name }` con el nombre de la carpeta del destino, nunca su ruta.

**Reglas del paso (`MergeStep`)**
- `scope()`: `worktrees = [raíz del destino]`, `refs = ["refs/heads/<base>"]`. El ejecutor lo valida con `ProtectedRequest::for_step` (D12 de DS-TS-CKP-002).
- `subtype()`: `"merge-into-base"`.
- `scope()` declara el destino; `ProtectedRequest::for_step` añade siempre el worktree pedido (el del agente), así que el snapshot previo cubre los dos.
- `run()`: lee `before` (I8) y luego `UserGitCommand::new(git, RepoTarget { git_dir: git_dir propio del destino, work_tree: Some(raíz del destino) }, UserOp::MergeOid { oid, message }, None, session_env, rejecting_editor(None), home)` y `run_git(ctx, cmd, cancel)` (I15). Sin transiciones propias: la del plan (`Expected::MergeInto`) ya la registra la ligadura de Guardrails (DS-TS-CKP-003 T006).
- El Git y el `HOME` salen de `OpsContext`, nunca del entorno del daemon ni del de la TUI.
- `classify_after` (I8) relee con la capa de lectura la punta de la base, el HEAD del destino, su operación en curso y su estado, y los compara con `before`, nunca con el plan. `changed_refs = ["refs/heads/<base>"]` si la punta cambió.
- Nada de `merge --abort` automático: un merge que choca queda como lo deja Git (BR-CKP-WF-003).

**Criterios de aceptación de la tarea**
- [ ] `ops::tests::production_wiring_has_no_governed_arm_without_guardrails` se adapta: `MergeIntoBase` se arma solo si la puerta decide.
- [ ] Tests unitarios de `merge.rs` para cada motivo del orden de arriba, con repos temporales.
- [ ] Un repo con `refs/replace` que sustituye la punta del agente integra el commit real (L-04, Validación 25 de ADR-CKP-002, delegada aquí por DS-TS-CKP-002 § 9).

### T004 — Crear la elegibilidad de "Integrar" en la TUI

**Objetivo.** La columna "Merge" de BR-CKP-ELIG-001, como función pura, con el motivo y la acción que desbloquea.

**Ubicación.** `apps/cli/src/tui/merge.rs` (**CREATE**), registrado con una línea en `tui/mod.rs`.

**Reglas**
- `merge_eligibility(repo: &RepoView, row: &WorktreeRow, layer: Option<Layer>, live: bool) -> Eligibility`, con `Eligibility::{Offered, Disabled { reason: MergeBlock, unblock: Option<Unblock> }}`. Orden de evaluación (el primero que falla manda):
  1. Conexión fuera de "En vivo" → `NotLive` (ADR-CKP-003 § 2, escrituras solo con datos frescos).
  2. Capa distinta de `cockpit` → `AgentLayer` (M-03).
  3. Worktree no disponible → `Unavailable` (BR-CKP-EDGE-003).
  4. Fila del worktree principal, o cuya rama es la base → `IsTarget`.
  5. Base no confirmada o pendiente → `BaseUnconfirmed`, con la acción de confirmarla (BR-CKP-WF-005).
  6. Operación en curso en la fila o en el destino → `InProgress`.
  7. Destino con cambios preparados o sin preparar en archivos seguidos (criterio de I6, el mismo que `Status::has_tracked_changes()`: el recuento de sin seguimiento de la instantánea no cuenta) → `TargetDirty`, con "commitea o guarda los cambios de <destino>".
  8. Sesión presente en el destino → `TargetSession`.
- Sin fila con la base sacada, "Integrar" se ofrece: la TUI no sabe si hay avance rápido, así que decide el daemon (`base-not-checked-out` en esta historia; US-CKP-024 añade el avance rápido).
- Son precondiciones de presentación: el daemon las revalida siempre (BR-CKP-ELIG-001).
- Sin texto: devuelve códigos; los textos son de T006.

**Criterios de aceptación de la tarea:** tests unitarios en `tui/merge.rs`, uno por regla y uno del orden.

### T005 — Crear el flujo de escritura genérico

**Objetivo.** Una máquina de estados reutilizable para toda acción de escritura, y una sola vía nueva por el canal.

**Ubicación.**
- `apps/cli/src/tui/write_flow.rs` (**CREATE**).
- `apps/cli/src/model.rs` (**MODIFY**): `Cmd::Operation(OpRequest)` y `EngineMsg::Operation(OpReply)`; `Model.ui` gana `write: WriteFlow`.
- `apps/cli/src/client/mod.rs` (**MODIFY**): `LinkCmd::Operation(OpRequest)`; el hilo llama a `operation.prepare`, `operation.run` o `timemachine.undo` con `Link::call` y devuelve `OpReply`.

**Reglas**
- `OpRequest::{Prepare { operation, worktree, session_env }, Run { plan_id, accepted_warnings, confirmation }, Undo { worktree, operation_id }}`. `OpReply::{Prepared(Box<PrepareResult>), Ran(Box<OperationRunResult>), Undone(Box<UndoResult>), Refused { reason: RefusalView }, Failed(LinkErrorView)}`. `RefusalView` lleva el `RejectReason` tipado (o el código `-32004`, `-32006`, el de la Time Machine) y la `GuardrailsDenial` si viene.
- `session_env` (M-01): `PATH`, `HOME` y las locales de la propia TUI, las únicas que el daemon acepta (`SessionVar`). El daemon las valida otra vez.
- Estados: `Idle → Preparing { op, row } → Confirming { plan } → Running { plan_id } → Done { operation_id, undo_until } | Rejected { reason } | Stopped | Failed { outcome }`. Una sola escritura a la vez por TUI: con un flujo que no está en `Idle`, `Done`, `Rejected` o `Failed`, otra tecla de escritura no hace nada y muestra "hay una acción en curso".
- `accepted_warnings` = exactamente `plan.warnings`; `confirmation` = `plan.challenge`. La TUI no los compone.
- Responder No en el prompt, o `Esc`, vuelve a `Idle` sin llamar a `operation.run`. El plan caduca solo en el daemon (60 s) o al cerrar la conexión.
- Feedback < 100 ms: la tecla pasa a `Preparing` y se pinta en la siguiente iteración; nada espera en el hilo principal (ADR-CKP-003 § 3).
- Todo texto que venga del daemon (nombres de rama, salida de Git) pasa por `SafeText` antes de llegar al modelo (ADR-CKP-003 § 8, SEC-12).

**Criterios de aceptación de la tarea:** tests unitarios de transición en `write_flow.rs`, incluido "No no ejecuta" y "una acción a la vez".

### T006 — Pintar la acción, el prompt y el resultado

**Ubicación.**
- `apps/cli/src/tui/keymap.rs` (**MODIFY**): `Action::Integrate` (`m`) y `Action::Undo` (`u`) en la tabla única.
- `apps/cli/src/tui/update.rs` (**MODIFY**): despacha `Integrate`, `Yes`/`No` en `Confirming` y `EngineMsg::Operation` a `write_flow`; nada de lógica propia.
- `apps/cli/src/tui/view.rs`, `tui/help.rs` (**MODIFY**): la acción en `KeyHints` (desactivada con su motivo), el ConfirmPrompt y el toast.
- `apps/cli/src/present/write_text.rs` (**CREATE**): textos tipados de los motivos de rechazo (`RejectReason` completo, `match` exhaustivo en/es, `every()` para el test de idiomas) y de los desenlaces. Una línea en `present/mod.rs`.
- `apps/cli/src/present/merge_text.rs` (**CREATE**): textos del merge (plan, bloqueos de T004, toast).

**Reglas**
- ConfirmPrompt (`widgets/confirm.rs`, default No): título "Integrar", pregunta "integrar <rama> (<n> commits de <actor>) en <base>". Si el plan trae `active-session` y alguna sesión del worktree del agente está Activa (BR-CKP-EDGE-004), una línea antes: "el agente sigue activo; se integra hasta el commit <oid7>". Si trae `other-actors-work`, ninguna línea extra: la confirmación del plan ya es esa (I4). Todos los motivos van en el mismo prompt (ADR-CKP-002 § 3).
- `commits_capped` → "<n>+ commits".
- Toast al terminar (`widgets/notification.rs`, 5 s): `done` → "Integrado <rama> en <base> · u Deshacer"; `aborted` → "no se pudo tomar el snapshot previo; no se integró nada"; `failed-unchanged` → el motivo y "no cambió nada"; `failed-changed` → el motivo y "u Deshacer"; `stopped` → "Merge detenido" (US-CKP-016 añade rutas, Abortar y editor); `rejected` → el texto de `write_text.rs`.
- La salida de Git (`git_output`) se ve con `o` sobre el toast (`?` es la Ayuda, I11), saneada y truncada; nunca se pinta entera en la fila.
- Ninguna acción de push, en ningún estado (BR-CKP-CONS-002).
- Plegado ASCII y sin color como el resto de la TUI (DS-US-CKP-005).

**Criterios de aceptación de la tarea:** snapshots en en/es del prompt, de "Integrar" desactivado y de cada toast, en `apps/cli/src/tui/snapshots/`.

### T007 — Añadir Deshacer desde el toast

**Ubicación.** `apps/cli/src/tui/undo.rs` (**CREATE**), una línea en `tui/mod.rs`.

**Reglas**
- `u` solo hace algo si hay un `Done` o `FailedChanged` con `undo_until` vigente: envía `OpRequest::Undo { worktree: raíz del worktree del agente, operation_id }` (I12).
- `UndoResult` hecho → toast "Deshecho: <base> vuelve a <oid7>". Un rechazo de la Time Machine (`TmRejectReason`) → su texto; `git-operation-in-progress` → "aborta primero el merge en curso" (lo usa US-CKP-016).
- Una petición de confirmación de la Time Machine → "Deshacer necesita confirmación: usa raptor undo" (I12).
- Los `warnings` de `UndoResult` se muestran en el toast (G1).

**Criterios de aceptación de la tarea:** test de punta a punta "al deshacer, main vuelve exactamente al commit anterior".

### T008 — Cerrar la historia

- `cargo clippy` y `cargo test` (o `nx affected -t lint test`) en verde; contrato de `/implement` con `node tools/test/nextest-junit.mjs`.
- La historia pasa a `implemented` y enlaza el PR; esta DS añade `## Estado de la implementación`.
- Las enmiendas de [Enmiendas que implica](#enmiendas-que-implica) se aplican en el mismo PR o quedan anotadas en él.

---

## Estructura de ficheros

| Archivo | Acción | Tarea |
|---|---|---|
| `crates/api/src/merge.rs` | CREATE | T002 |
| `crates/api/src/lib.rs` | MODIFY (1 línea `pub mod`) | T002 |
| `crates/api/src/catalog.rs` | MODIFY (motivos, campo `merge`) | T002 |
| `crates/api/src/methods/operation.rs` | MODIFY (capacidad) | T002 |
| `crates/core/src/executor/ops/context.rs` | CREATE | T002 |
| `crates/core/src/daemon/operation_facts.rs` | CREATE | T002 |
| `crates/core/src/daemon/mod.rs` | MODIFY (1 línea `mod`) | T002 |
| `crates/core/src/daemon/serve.rs` | MODIFY (1 llamada) | T002 |
| `crates/core/src/executor/ops/merge.rs` | CREATE | T003 |
| `crates/core/src/executor/ops/mod.rs` | MODIFY (brazo, `OnceLock<OpsContext>` y `attach`) | T002, T003 |
| `crates/core/src/timemachine/protected/backend.rs` | MODIFY (`OperationCatalog::attach` por defecto) | T002 |
| `crates/core/src/executor/mod.rs` | MODIFY (`Prepared.view`) | T002 |
| `crates/git/src/preflight.rs` o `reader.rs` | MODIFY (rama en rebase o bisect de un worktree, regla 5) | T003 |
| `crates/core/src/channel/conn.rs` | MODIFY (capacidad en `operation.prepare`) | T002 |
| `crates/core/tests/us_ckp_014_merge.rs` | CREATE | T001 |
| `apps/cli/src/tui/merge.rs` | CREATE | T004 |
| `apps/cli/src/tui/write_flow.rs` | CREATE | T005 |
| `apps/cli/src/tui/undo.rs` | CREATE | T007 |
| `apps/cli/src/model.rs`, `client/mod.rs` | MODIFY (una variante genérica cada uno) | T005 |
| `apps/cli/src/tui/{update,view,help,keymap,mod}.rs` | MODIFY | T006 |
| `apps/cli/src/present/{write_text,merge_text}.rs` | CREATE | T006 |
| `apps/cli/tests/tui_merge.rs` | CREATE | T001 |

## Contratos compartidos

### Tipos y datos compartidos

```rust
// crates/api/src/merge.rs (T002) — a crear
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergePlanView {
    pub base: UntrustedName,                  // nombre corto de la base
    pub source_branch: Option<UntrustedName>, // None con HEAD separado
    pub integrated: String,                   // oid completo (hex)
    pub commits: u32,
    pub commits_capped: bool,
    pub target: MergeTargetView,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum MergeTargetView {
    Worktree { name: UntrustedName }, // carpeta del destino, nunca su ruta
    RefOnly,                          // avance rápido sin worktree (US-CKP-024)
}

// crates/api/src/catalog.rs (T002) — RejectReason, variantes nuevas (aditivas, solo con operation.merge)
BaseUnconfirmed, MainWorktree, NothingToIntegrate, TargetDirty, TargetSessionPresent, BaseNotCheckedOut,
// PrepareResult, campo nuevo
#[serde(default, skip_serializing_if = "Option::is_none")]
pub merge: Option<crate::merge::MergePlanView>,

// crates/core/src/executor/ops/context.rs (T002) — a crear
pub struct OpsContext {
    pub git: gitraptor_git::resolve::SystemGit,
    pub home: Option<std::path::PathBuf>,
    pub engine: std::sync::Arc<dyn EngineFacts>,
}
pub trait EngineFacts: Send + Sync {
    fn base(&self, repo_id: &str) -> gitraptor_policy::team::BaseBranch;
    fn present_sessions(&self, repo_id: &str, worktree: &std::path::Path) -> Vec<PresentState>;
}
pub struct PresentState { pub session_id: String, pub state: gitraptor_api::messages::SessionStateView }

// apps/cli/src/tui/write_flow.rs (T005) — a crear
pub enum OpRequest { Prepare { .. }, Run { .. }, Undo { .. } }
pub enum OpReply { Prepared(..), Ran(..), Undone(..), Refused { .. }, Failed(..) }
pub enum WriteFlow { Idle, Preparing { .. }, Confirming { .. }, Running { .. }, Done { .. }, Rejected { .. }, Stopped { .. }, Failed { .. } }

// apps/cli/src/tui/merge.rs (T004) — a crear
pub enum Eligibility { Offered, Disabled { reason: MergeBlock, unblock: Option<Unblock> } }
pub enum MergeBlock { NotLive, AgentLayer, Unavailable, IsTarget, BaseUnconfirmed, InProgress, TargetDirty, TargetSession }
```

Lo que ya existe y se usa tal cual: `OperationId::MergeIntoBase`, `OperationArgs::MergeIntoBase`, `PrepareResult`, `RunParams`, `OperationRunResult` (`crates/api/src/timemachine.rs:149`), `OperationOutcome`, `WarningCode::{ActiveSession, OtherActorsWork}`, `UserOp::MergeOid`, `run_git`, `PlanShape::Merge`, `Expected::MergeInto`, `UndoParams`, `RepoReader::{worktrees, status, merge_bases, ahead_behind_commits, resolve_ref}`, `observe::open_registered_worktree`.

### Ciclos de vida (DI)

- `OpsContext`: uno por daemon, escrito una vez en `serve_channel` con `OperationCatalog::attach` (`OnceLock` dentro de `ProductionCatalog`), leído por cada `plan_op` y cada paso. Sin contexto, el merge no se sirve.
- `WriteFlow`: uno por `App`, en `Model.ui`.

## Contrato de API

| Método | Cambio | Con `operation.merge` | Sin ella |
|---|---|---|---|
| `operation.prepare {operation: "merge-into-base", worktree: <raíz del agente>, args: {}, session_env}` | Forma: `merge` y motivos nuevos | `PrepareResult` con `merge`, `warnings`, `challenge` | `-32004`, `implemented_by = US-CKP-014` |
| `operation.run {plan_id, accepted_warnings, confirmation}` | Ninguno | `OperationRunResult` con `outcome` y `git_output` (capa `cockpit`) | — |
| `timemachine.undo {worktree, operation_id}` | Ninguno | `UndoResult` | — |

### Forma del error y del cuerpo de respuesta

Un rechazo es `-32014 OPERATION_REJECTED` con `data.reason` (`RejectedData`), y `data.guardrails` con `operation.guardrails` y `guardrails-denied`. No hay códigos nuevos (`rpc.rs` congelado).

### Valores numéricos

| Valor | Cifra | Fuente |
|---|---|---|
| Caducidad del plan | 60 s | `PLAN_TTL_MS`, DS-TS-CKP-002 D8 |
| Toast con Deshacer | 5 s | BR-CKP-WF-002 |
| Espera de la barrera | 2 s | `REGISTRATION_WAIT`, DS-TS-CKP-002 § 5 |
| Tope del recuento de commits | el de `ahead_behind_commits` | `crates/git/src/reader.rs` |

## Estrategia de pruebas y cobertura

### 9.1 Pirámide de pruebas

| Nivel | Dónde | Qué |
|---|---|---|
| Unit | `ops/merge.rs`, `tui/merge.rs`, `tui/write_flow.rs`, `present/*_text.rs` | Motivos y su orden, elegibilidad, transiciones, idiomas |
| Integración (daemon + Git real) | `crates/core/tests/us_ckp_014_merge.rs` | Escenarios del daemon |
| E2E (TUI + daemon) | `apps/cli/tests/tui_merge.rs` (macOS, debug) | Escenarios de la historia de punta a punta |
| Snapshots | `apps/cli/src/tui/snapshots/` | Prompt, acción desactivada, toasts, en/es |

### 9.2 Umbrales de cobertura

Toda regla de `plan_op` y de `merge_eligibility` con su test. Sin cifra de cobertura de líneas en el repo.

### 9.3 Datos de prueba

Repos temporales con `gitraptor_testkit::Fixture`: `main` sacada en el worktree principal y `feat-pagos` en un worktree enlazado con 3 commits; sesión de `raptor-fake-agent` como "claude-1". Un remoto `origin` local (`file://`) para el escenario del remoto. Nunca este repo (NFR-01).

### 9.4 Comportamientos críticos verificados

| Escenario de la historia | Test |
|---|---|
| Integrar, confirmando el plan, y deshacer | `tui_merge::integrate_confirm_and_undo` (E2E): prompt con "integrar feat-pagos (3 commits de claude-1) en main", tras `y` `main` = merge de `feat-pagos`, snapshot previo en el oplog y toast con Deshacer; tras `u`, `main` = commit anterior. `us_ckp_014_merge::a_merge_runs_in_the_target_worktree_with_a_prior_snapshot` (daemon) |
| Agente activo, se integra hasta el commit visto | `tui_merge::an_active_agent_is_integrated_up_to_the_seen_commit`: aviso y plan en el mismo prompt con el oid; `main` contiene `a1b2c3` y ningún commit posterior del agente (el agente commitea después de terminar el merge) |
| Destino con cambios, acción desactivada | `tui::merge::tests::a_dirty_target_disables_integrate` (unit) y `us_ckp_014_merge::a_dirty_target_is_refused` (`target-dirty`) |
| Base pendiente, acción desactivada | `tui::merge::tests::a_pending_base_disables_integrate` y `us_ckp_014_merge::an_unconfirmed_base_is_refused` (`base-unconfirmed`) |
| Sin snapshot no hay merge | `us_ckp_014_merge::without_a_prior_snapshot_main_does_not_change`: `prior_layer` que falla (solo depuración); `outcome = aborted`, punta de `main` igual; texto del toast en `tui_merge::a_failed_prior_snapshot_integrates_nothing` |
| Nunca se contacta con el remoto | `us_ckp_014_merge::the_remote_refs_never_change`: `refs/remotes/origin/*` y el repo remoto iguales tras el merge; ningún `KeyHint` de push en ningún estado (`tui_merge`). La captura de red es de la suite "ejecutor" de INF-GRP-001 |

Además:
- `us_ckp_014_merge::a_client_without_the_capability_gets_not_implemented`.
- `us_ckp_014_merge::a_merge_is_governed_once`: un merge permitido sin regla no deja entrada en el registro; base protegida con `everyone` → `guardrails-denied` antes de cualquier efecto.
- `us_ckp_014_merge::replace_refs_do_not_change_what_is_merged` (L-04).
- `us_ckp_014_merge::a_conflicting_merge_stops_as_git_leaves_it`: `outcome = stopped`, `MERGE_HEAD` presente, nada abortado (lo amplía US-CKP-016).
- `us_ckp_014_merge::an_agent_layer_cannot_merge`: capa `mcp` → `not-available-for-layer`.
- `us_ckp_014_merge::a_linked_target_worktree_gets_the_merge` (I15): `main` sacada en un worktree enlazado y el principal en otra rama; el merge cambia `main` y el HEAD del principal queda igual.
- `us_ckp_014_merge::a_base_being_rebased_is_in_progress` (regla 5): un worktree rebasando `main` (HEAD separado) → `operation-in-progress`, nunca `base-not-checked-out`.
- `us_ckp_014_merge::a_hook_of_the_merge_acts_as_the_requester` (I9, Validación 17 de ADR-CKP-002 con Git real): un `pre-merge-commit` que llama al canal se atribuye al solicitante del plan; un comando reservado da `daemon-descendant`, con el solicitante en la auditoría.
- Condiciones del security-expert para I9, con Git real y repos temporales:
  - (i) `a_hook_asking_a_reserved_command_is_daemon_descendant`: un hook del merge pide un comando reservado → `daemon-descendant`, con el solicitante del plan en la auditoría.
  - (ii) `a_hook_asking_a_catalog_operation_is_refused_without_waiting`: un hook pide una operación del catálogo → `executor-descendant` sin esperar al cerrojo del repo ni interbloquear (el merge termina).
  - (iii) `a_failed_identity_read_kills_the_child`: fallo inyectado de la lectura de procesos (`procs.read`, solo en depuración) → el hijo muere, desenlace `failed-*` por estado real, snapshot previo intacto y ningún dato perdido.
  - (iv) `a_barrier_wait_over_two_seconds_fails_closed`: espera de más de 2 s inyectada → identidad no verificada, el merge falla cerrado y la auditoría lleva `barrier-timeout`.
- `us_ckp_014_merge::a_real_merge_commit_matches_the_registered_transition`: base con una regla de Guardrails que deja integrar a la capa `cockpit` y un merge con commit de merge (no avance rápido). El `reference-transaction` reconoce `Expected::MergeInto` (padres `[old, integrated]`) junto a las actualizaciones de `HEAD` y `ORIG_HEAD` que escribe `git merge`. El brazo de prueba solo había probado `update-ref`.
- `merge.rs::tests::classify_compares_with_before_not_with_the_plan` (I8): una ref ajena movida antes del lanzamiento y un `git` que falla dan `failed-unchanged`.

### 9.5 Plataformas

Se ejecuta en macOS. Linux y Windows: **Pendiente: etapa de validación multiplataforma** ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)). En Windows el merge se rechaza con `confirmation-unavailable` por I4 hasta que exista la confirmación de trabajo ajeno (TQ-14 b). El texto del rechazo dice que en Windows todavía no se puede confirmar una integración desde GitRaptor (BR-CKP-AUTH-003); nunca es un error genérico (Q-CKP-36). Además, el merge en Windows no se libera hasta la etapa multiplataforma de la barrera de arranque (I9, disparador (c)); hasta entonces falla cerrado.

## Gate de seguridad

- [ ] **NFR-02, sin shell**: el único lanzamiento es `run_git` con `UserOp::MergeOid`; el mensaje es plantilla fija con nombres validados por `RefName`; el oid sale del plan, nunca de texto libre. Comprobación estática de `crates/core/tests/executor_boundary.rs` en verde.
- [ ] **Entradas validadas**: `merge-into-base` sigue con `NoArgs` y `deny_unknown_fields`; el worktree pedido debe ser la raíz de un worktree observado (`repo_of`); el destino se abre solo por el registro de worktrees.
- [ ] **Texto no confiable**: nombres de rama, carpeta y salida de Git pasan por `UntrustedName`/`Untrusted` en el contrato y por `SafeText` en la TUI. La salida de Git solo va a capa `cockpit`.
- [ ] **NFR-01**: sin snapshot previo no hay merge (`aborted`); Deshacer restaura destino y ref; los tests usan repos temporales.
- [ ] **Capa**: un agente (capa `mcp`) nunca integra, aunque abra la TUI.
- [ ] **Nunca red**: ningún `fetch` ni `push`; `protocol.allow=never` en *partial clone* (Enmienda 2026-10-17 de ADR-CKP-002).

## Cómo extender

- **Otra acción de escritura del Cockpit** (rebase, descartar, crear worktree): un archivo de brazo en `crates/core/src/executor/ops/<op>.rs` con una línea en `ops/mod.rs` ([extender sin archivos compartidos](../../../../architecture/extender-sin-archivos-compartidos.md), "Añadir una operación del catálogo"); en la TUI, `tui/<op>.rs` con su elegibilidad y `present/<op>_text.rs` con sus textos. El flujo (`write_flow.rs`), la vía del canal (`OpRequest`/`OpReply`) y Deshacer (`undo.rs`) se reutilizan sin tocarlos.
- **Un motivo de rechazo nuevo**: variante aditiva en `RejectReason` detrás de la capacidad de su operación, y su texto en `present/write_text.rs` (el `match` exhaustivo obliga).
- **Un dato del motor que necesita un brazo**: método nuevo en `EngineFacts` con implementación en `daemon/operation_facts.rs`. Nunca E/S de Git en `EngineFacts`.
- **Una tecla**: una fila en la tabla de `keymap.rs` (archivo compartido por diseño, ADR-CKP-003 § 10).

## Enmiendas que implica

No aplicadas por esta DS; las aplica el PR de la historia o quedan anotadas en él.

- **ADR-CKP-002 § 4 y Pendientes** (pendiente, no aplicada; I9). Texto propuesto: "La barrera del lado del lector cumple I-02 para las operaciones que lanzan `git` en producción, solo en la capa `cockpit`: ningún descendiente se resuelve antes de la marca, falla cerrado, y el hijo muere si su identidad no se lee. El *spawn* suspendido deja de ser requisito de la primera historia que lance `git` y pasa a mejora. Vuelve a ser requisito si se da uno de estos casos: (a) rechazos por fin de la espera de 2 s en uso real, medidos con la marca de auditoría propia `barrier-timeout` (distinta de `IdentityUnverified`); (b) cualquier control que suponga que el hook no actuó antes del registro (Guardrails sobre lo que el hook escribe, confirmaciones bajo un plan "sin atribuir"); (c) la validación multiplataforma de la barrera en Windows, donde el merge no se libera hasta entonces y falla cerrado. Toda operación de la capa `mcp` que lance `git` con los hooks del repo exige una nueva ratificación del security-expert y, antes, *spawn* suspendido o hooks desactivados en esa capa (`-c core.hooksPath=` vacío, argv fijo). **Validación 18** se corrige: con la barrera del lector el hook puede haber corrido antes de fallar el registro, así que el desenlace se clasifica por el estado real (`failed-unchanged` o `failed-changed`), con el snapshot previo intacto. Si se hace, irá en un crate con `unsafe` confinado, como `crates/macsys` (ADR-GRP-002, Enmienda 2026-10-05)". Q-CKP-35 está ratificada con condiciones: la aplica el PR de la historia, y sin ella aplicada la historia no pasa a `implemented`.
- **DS-TS-CKP-002 § 9**: "el Git resuelto y el `HOME` llegan a los brazos por `OpsContext`, adjunto con `OperationCatalog::attach`" (I1).

## Fuera de alcance

| Qué | Dueño |
|---|---|
| ⚡ en el plan, avance rápido sin worktree, pruebas de dos TUIs y del commit entre fases | US-CKP-024 |
| Rutas en conflicto, Abortar, Abrir en el editor, Cancelar y tiempo transcurrido | US-CKP-016 (y US-CKP-013) |
| Deshacer desde el historial | US-CKP-021 |
| Excepción consciente tras una denegación | US-CKP-019 |
| Cola de confirmación de Guardrails | US-CKP-023 |
| Atribución commit→evento para el trabajo afectado | DEP-CKP-2 (motor-local) |
| Merge por MCP | Fuera del MVP (Q-MCP-1) |

## Notas del autor

- Riesgo: el hook `pre-merge-commit` del usuario corre bajo el ejecutor y retiene el cerrojo del repo mientras dure; la cola se ve en todas las TUIs y Cancelar llega con US-CKP-016.
- Riesgo: por I4, en Windows no hay merge hasta que llegue TQ-14 b o DEP-CKP-2. Se acepta para esta historia (Q-CKP-36); para M4 (v0.1.0 firmada en Windows) queda abierto.
- `stopped` aparece en esta historia aunque su presentación completa sea de US-CKP-016: el paso tiene que clasificarlo bien desde el primer día.

### Preguntas abiertas

| ID | Pregunta | Para |
|----|----------|------|
| Q-CKP-35 | ¿Se ratifica que la barrera del lado del lector basta para la primera operación gobernada que lanza `git` en producción (I9), y se enmienda ADR-CKP-002 § 4 con el texto de [Enmiendas que implica](#enmiendas-que-implica)? Arquitecto (revisión, 2026-10-27): sí. Es reversible y no hay datos en juego. **Cerrada.** **Decisión del orquestador (2026-10-27), validada por Arquitecto (revisión) y security-expert, con condiciones**: solo capa `cockpit`; desenlace por estado real; marca `barrier-timeout`; disparadores (a), (b) y (c); tests (i) a (iv) de § 9.4; Windows sin merge hasta la validación de la barrera | — |
| ~~Q-CKP-36~~ | ~~Sin DEP-CKP-2, todo merge cuenta como trabajo de otro actor (I4): siempre pide confirmación y en Windows se rechaza. ¿Aceptable para el MVP?~~ **Cerrada.** Decisión del orquestador (2026-10-27), validada por PO: se acepta. Confirmar siempre es lo que ya pide la historia ("pide confirmarlo") y BR-CKP-AUTH-003. Es una sola tecla y muestra lo que se integra, así que no le quita valor a la acción. El rechazo en Windows coincide con BR-CKP-AUTH-003 y con lo que hoy hace el ejecutor en Windows (XP-19); su texto tiene que ser explícito (9.5). Queda abierto para el orquestador: BR-03 y BR-07 son Must y M4 se firma en Windows, así que TQ-14 b o DEP-CKP-2 tiene que entrar en el plan de M4, o la release tiene que declarar que en Windows no hay merge | PO |
| ~~Q-CKP-37~~ | ~~¿Un archivo sin seguimiento en el destino debe bloquear el merge (I6), o solo los cambios de archivos seguidos?~~ **Cerrada.** Decisión del orquestador (2026-10-27), validada por PO: solo bloquean los cambios en archivos seguidos. El worktree principal suele tener archivos sin seguimiento (notas, salidas locales), y bloquear por ellos obliga a limpiar sin ganar seguridad: Git no los sobrescribe y el snapshot previo los guarda. Cambian I6, T003 y T004; queda aclarado en BR-CKP-ELIG-002 | PO |
| ~~Q-CKP-38~~ | ~~Con sesión Inactiva en el worktree del agente, el ejecutor exige enumerar `active-session` (T003, regla 8). ¿Se mantiene, o se separa un aviso propio de "sesión Activa"?~~ **Cerrada.** Decisión del orquestador (2026-10-27), validada por PO: se mantiene, sin aviso propio. Lo que ve el usuario cumple BR-CKP-ELIG-001 (la línea "hasta el commit X" solo sale con sesión Activa), y el plan ya muestra el oid que se integra, así que con una sesión Inactiva no se pierde información. Se precisa un caso: con worktree compartido manda la sesión más restrictiva (BR-CKP-EDGE-004) | Arquitecto / PO |

## Estado

- Lista para implementar: T001 en rojo primero.
- `gaps_release: 1` por G1 (US-TMC-019).
