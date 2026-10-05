---
id: DS-US-TMC-002
title: "Dev Spec — Deshacer la última operación del worktree (raptor undo)"
type: dev-spec
status: approved
feature: time-machine
domain: GRP
story: US-TMC-002
created: 2026-10-05
updated: 2026-10-05
related:
  stories: [US-TMC-002, US-TMC-001, US-TMC-003, US-TMC-004, US-TMC-012, US-TMC-013, US-TMC-021]
  enablers: [TS-TMC-002, TS-TMC-003, TS-TMC-004, TS-CKP-002]
  adrs: [ADR-TMC-002, ADR-TMC-003, ADR-TMC-004, ADR-TMC-005]
  rules: [BR-TMC-WF-001, BR-TMC-VAL-001, BR-TMC-CONS-001, BR-TMC-AUTH-001]
  nfrs: [NFR-01, NFR-TMC-14]
tags: [time-machine, undo, pila-por-worktree, aplicador, operacion-protegida, solicitante, permisos, i18n, m1]
---

# Dev Spec — US-TMC-002: `raptor undo` de la última operación

Plano compacto de [US-TMC-002](../user-stories/US-TMC-002-undo-ultima-operacion.md). Une piezas que ya están en `main`: la pila por worktree del oplog (`oplog/stack.rs`, TS-TMC-002), el aplicador (TS-TMC-003), la operación protegida con su solicitante (TS-TMC-004) y el almacén real de cada repo (US-TMC-001).

**Qué entrega**: `timemachine.undo` sin selectores deshace, como operación protegida de la Time Machine, la operación más reciente aún no deshecha del worktree pedido. Toma su propio snapshot previo, aplica el estado previo de la operación deshecha y registra el undo con su solicitante congelado, su canal y la operación sobre la que actuó. `raptor undo` lo pide desde el worktree del cwd, con textos en/es. Entrega el **mecanismo** del criterio de salida 3 de M1: un `reset --hard` real con trabajo sin commitear se recupera con el binario `raptor undo` (test e2e). El criterio en sí se cierra con uso real y con el Git crudo en la pila (US-TMC-004; ver § 6 y la decisión del PO en § 7).

**Qué no entrega** (interfaces preparadas, § 6): redo (US-TMC-003), Git crudo en la pila (US-TMC-004), `--since`, `--agent` y por id (US-TMC-010/011), solape (US-TMC-012), confirmación interactiva (US-TMC-013) y Guardrails (US-TMC-021).

## 1. Ubicación en el código

| Archivo | Responsabilidad |
|---|---|
| `crates/api/src/timemachine.rs` | `UndoResult` (+ proyección MCP `McpUndoResult`), `TmRejectedData` y `TmRejectReason` (códigos estables, NFR-TMC-14) |
| `crates/core/src/timemachine/undo.rs` (nuevo) | `UndoBackend` (repo de un worktree para la Time Machine), `TmRepoHandle`, la planificación del undo (pila, destino, regla base de permisos) y `UndoStep`, el `ProtectedStep` que corre el aplicador |
| `crates/core/src/timemachine/protected/mod.rs` | `ProtectedStep::self_annotated()`: el aplicador anota sus propios pasos y su cierre (ADR-TMC-002 § 3); la operación protegida no anota `applying{1}` y solo cierra si el paso dejó la operación abierta. `StepCtx::oplog()` solo para la crate |
| `crates/core/src/timemachine/protected/backend.rs` | `TimeMachineBackend` (implementa `UndoBackend`) sobre `TmRepos`: oplog, almacén, raíz principal, carpeta de la Time Machine y la clave del lock del repo |
| `crates/core/src/timemachine/apply/{mod,plan}.rs` | `ApplyPlan.refs: RefScope` (`None`, `All` u `Only(nombres)`) en lugar de `move_refs: bool`: un undo mueve solo las refs de su ámbito. `Applier::apply_holding` para quien ya tiene el lock del repo |
| `crates/core/src/repo_lock.rs` | `RepoGuard::key()`, para que el aplicador compruebe que el lock que recibe es el de su repo |
| `crates/core/src/channel/{mod,server,conn}.rs` | `TimeMachineWiring` en el canal; `timemachine.undo` sale de "no implementado" para el undo sin selectores |
| `crates/core/src/daemon/mod.rs` | El daemon cablea la Time Machine **siempre** (no depende del catálogo de operaciones): `TmRepos` + Git resuelto + `tm_prior_layer` (solo debug) |
| `apps/cli/src/{main,undo}.rs`, `apps/cli/i18n/{en,es}.txt` | `raptor undo [--json]`, raíz del worktree del cwd, textos y motivos en/es |

## 2. Diseño

### 2.1 Flujo en el daemon (orden de ADR-TMC-005 § 4)

1. **Validación** (BR-TMC-VAL-001): parámetros estrictos (ya en `main`). Con `since`, `agent` u `operation_id` el undo sigue por el camino que ya había (`tm_command`: solicitante, ámbito e id comprobados, sin oplog) y responde `NOT_IMPLEMENTED` con su historia (US-TMC-010, US-TMC-011; `operation_id` sin historia asignada todavía).
2. **Solicitante y canal**: como `operation.run` (TS-TMC-004). "Sin atribuir" por MCP: `SCOPE_REFUSED` antes de calcular nada (TQ-7 → a, ya en `main`).
3. **Ámbito**: CLI/TUI, la raíz del worktree que nombra el cliente; MCP, el cwd del llamante y la allowlist (deny-all en producción, DEP-MCP-4). Carpeta no observada o que no es raíz de worktree: `SCOPE_REFUSED`.
   - **Lock del repo**: el undo hace cola en `repo_lock::lock_queued` con la clave del ejecutor (ruta del almacén, ADR-CKP-002 § 5) y lo mantiene desde elegir el destino hasta el cierre. La "última operación" no puede cambiar debajo, un repo ocupado no gasta un previo y el aplicador recibe el lock ya tomado (`apply_holding`). Cola llena o daemon deteniéndose: `rechazada` con `repo-busy`.
4. **Conjunto**: `Oplog::undo_stack(StackScope::Worktree(<ruta canónica>), &[])` → `last_operation()`. Sin nada: `rechazada` con `nothing-to-undo`. Un `OpRef::GitEvent` (Git crudo) no puede aparecer todavía (la lista externa va vacía hasta US-TMC-004); si apareciera, `rechazada` con `raw-git-not-covered`.
5. **Destino**: el snapshot previo de esa operación (ADR-TMC-003 § 4), que debe seguir disponible (`complete`, `purge-announced` o `purge-cancelled`) y verificar en el almacén; si no, `rechazada` con `target-unavailable`.
6. **Regla base de permisos** (ADR-TMC-005 § 2), sobre el solicitante congelado de la operación deshecha (para una operación de GitRaptor el actor es su solicitante): agente X sobre trabajo de X (misma sesión; el nombre es texto del agente y solo se muestra) → permitido; agente X sobre otro → `other-actor`; "sin atribuir" (CLI/TUI) sobre "sin atribuir" → permitido; "sin atribuir" sobre trabajo de un agente → `confirmation-required` (la confirmación es US-TMC-013).
7. **Guardrails** (US-TMC-021) y **solape** (US-TMC-012): punto de enganche documentado en el código, sin implementar (§ 6).
8. **Precondiciones de Git** (`Applier::check_preconditions`) **antes** del snapshot previo, para no gastar un previo en un undo que no puede correr: operación de Git en curso, locks ajenos, repo sin confianza → `rechazada` con el código del aplicador.
9. **Operación protegida** (`ProtectedOperation::run`) de tipo `undo`: intención con `Target::Undo([op])`, el ámbito de la operación deshecha (sus worktrees y refs) y el solicitante congelado; snapshot previo de esos worktrees con el mismo snapshotter que US-TMC-001; después `UndoStep` corre el aplicador desde `ready`.

Toda petición que llega al paso 4, aceptada o rechazada, queda en el oplog con su solicitante, su canal y su motivo (auditoría, ADR-TMC-005 § 4): los rechazos de los pasos 4–8 se anotan como una operación `undo` que pasa de `intent` a `rejected`. Una operación `rejected` no entra en la pila.

### 2.2 Aplicación (TS-TMC-003)

- `ApplyPlan { target_snapshot: previo de la operación deshecha, prior_snapshot: previo del undo, worktrees, refs }`. Cada worktree del ámbito se busca en el `meta` del destino **por su ruta canónica** (la que guardó el daemon), y de ahí sale su clave (`main` o `wt-<id>`, claves estables de US-TMC-001). La raíz sale del oplog, nunca del cliente, y se vuelve a validar contra los worktrees registrados del repo (el oplog es entrada no confiable al releerse, SEC-TMC-09): uno que ya no está registrado da `worktree-unavailable`.
- **Refs del ámbito** (`RefScope::Only`): las ramas a las que apunta `HEAD` en cada worktree del ámbito, en el destino y ahora, más las refs que declaró la operación deshecha. Así un undo en `feat-login` nunca mueve la rama de `feat-pagos` (escenario 4). Si una de esas ramas está sacada en un worktree **fuera** del ámbito, `rechazada` con `ref-in-use`: moverla cambiaría la historia de ese worktree bajo sus archivos. El undo guarda ese conjunto en su `Scope.refs`, para que deshacer un undo interrumpido y el redo usen el mismo ámbito. Los nombres los valida el aplicador al cargar (`RefUpdate::new`, SEC-TMC-14). Con `RefScope::All` el aplicador se comporta como hasta ahora (restauración, US-TMC-009).
- `UndoStep::self_annotated() = true`: el aplicador anota `applying{3..7}`, los locks y el cierre (`finished`, `interrupted` o `rejected` desde `ready`). La operación protegida solo cierra la operación si el paso la dejó sin estado terminal (red de seguridad).
- El informe del aplicador (rutas escritas y borradas, rutas en solape o no restaurables, avisos) vuelve al cliente como `UndoResult`; las rutas viajan como `Untrusted` y la proyección MCP no lleva rutas (SEC-12).
- Un fallo a mitad deja el undo `interrupted`: cuenta como hecho en la pila y el siguiente `raptor undo` lo deshace volviendo a su previo (US-TMC-019).

### 2.3 Contrato

| Respuesta | Cuándo |
|---|---|
| `UndoResult { operation_id, prior_snapshot_id, undone_operation_id, undone_subtype, target_snapshot_id, requester, written, removed, not_restored[], warnings[] }` | Undo terminado |
| `OPERATION_REJECTED` + `TmRejectedData { reason, operation_id }` | Pasos 4–8 o rechazo del aplicador bajo sus locks; repo sin cambios |
| `PRIOR_SNAPSHOT_FAILED` + `PriorFailedData` (ya en `main`) | El previo del undo falló; el undo no corre (BR-TMC-CONS-001) |
| `OPERATION_FAILED` + `operation_id` | Interrumpido a mitad; `raptor undo` vuelve a su previo |
| `SCOPE_REFUSED`, `INVALID_PARAMS`, `NOT_IMPLEMENTED` | Como hasta ahora |

`TmRejectReason`: `nothing-to-undo`, `raw-git-not-covered`, `target-unavailable`, `other-actor`, `confirmation-required`, `git-operation-in-progress`, `git-busy`, `repo-busy`, `repo-untrusted`, `worktree-unavailable`, `invalid-snapshot`, `hostile-tree`, `ref-moved`, `ref-in-use`, `unsupported`, `git-unavailable`. Redo y restauración reutilizarán el mismo tipo.

### 2.4 Cableado

`TimeMachineWiring { backend: Arc<dyn UndoBackend>, git: Option<SystemGit>, invoker, prior_deadline }` en `ServeArgs`/`ServerCtx`. El daemon lo construye siempre en Unix sobre su `TmRepos`, aunque no haya catálogo: `raptor undo` funciona en el dogfooding en cuanto hay operaciones en el oplog. Sin Git resuelto, el undo se rechaza con `git-unavailable`. `DaemonConfig.tm_prior_layer` envuelve el snapshotter del undo para inyectar fallos en los tests y solo se aplica con `debug_assertions` (mismo patrón que `prior_layer` de US-TMC-001). El previo del undo usa el mismo snapshotter, la misma reserva de disco y el mismo tiempo máximo que US-TMC-001 (SEC-TMC-12). El lock del repo es el mismo del ejecutor (`repo_lock`, clave = ruta del almacén): un undo y una operación del catálogo no se pisan (ADR-CKP-002 § 5).

### 2.5 CLI

`raptor undo [--json]`: busca la raíz del worktree subiendo desde el cwd hasta una entrada `.git` (el daemon valida que es la raíz de un worktree observado) y llama a `timemachine.undo` con `surface: cli`. Salida: qué operación se deshizo, en qué worktree y el id del punto previo al undo ("se puede rehacer"); rutas no restauradas con su motivo; motivos de rechazo traducidos (`undo.reason.*`). Códigos de salida: 0 hecho, 1 rechazado o fallo. Sin daemon: el mensaje común de "no está corriendo".

## 3. Decisiones

Todas son **Decisión del orquestador (2026-10-05), validada por el Arquitecto** (ver § 7); D8 y el alcance del criterio 3, además, **por el PO**:

| # | Decisión | Motivo |
|---|---|---|
| D1 | El aplicador anota sus pasos y su cierre (`self_annotated`); la operación protegida no anota `applying{1}` para él | El aplicador necesita empezar en `ready` (permite `ready → rejected` bajo sus locks) y ya anota `applying{3..7}` y el cierre. Duplicarlo rompía las transiciones del diario |
| D2 | `RefScope::Only` con las ramas de `HEAD` de los worktrees del ámbito y las refs declaradas | El meta guarda **todas** las ramas; mover todas desharía trabajo de otros worktrees (escenario 4, BR-TMC-WF-001 "nunca actúa sobre otros worktrees") |
| D3 | Precondiciones de Git antes del previo, y de nuevo bajo los locks | Orden de ADR-TMC-005 § 4; un undo que no puede correr no deja un previo inútil. El aplicador las repite bajo sus locks (TS-TMC-003) |
| D4 | Los rechazos se registran como `undo` `intent → rejected` con motivo | Auditoría de ADR-TMC-005 § 4 y BR-TMC-VAL-001 ("rechazada con motivo") |
| D5 | Cableado de la Time Machine independiente del catálogo | En producción aún no hay catálogo (`operations: None`); el undo es de la Time Machine, no del ejecutor |
| D6 | Regla base de permisos implementada ya, con el agente comparado por sesión y el "sin atribuir" sobre trabajo de un agente rechazado como `confirmation-required` | ADR-TMC-005 § 2. Sin la confirmación de US-TMC-013 lo seguro es rechazar; nunca se toca trabajo ajeno sin control. El nombre del agente es texto que él aporta: no identifica |
| D7 | Selectores (`since`, `agent`, `operation_id`) siguen en `NOT_IMPLEMENTED` con su historia | Fuera del alcance; el contrato ya los valida |
| D8 | Tests con daemon real en proceso, catálogo de test y Git real; el e2e del criterio 3 lanza el binario `raptor` contra ese daemon | En producción no hay catálogo; un catálogo de test es la única forma de registrar una operación real (`git reset --hard`) en el oplog. El cliente hijo del proceso del daemon se resuelve como "sin atribuir" (TS-TMC-004 § 4), que es el solicitante de los escenarios. El e2e prueba el mecanismo, no el criterio (PO) |
| D9 | El undo hace cola en el lock del repo antes de leer la pila y lo mantiene hasta el cierre; el aplicador lo recibe tomado (`apply_holding`) | Ajuste bloqueante del Arquitecto: sin él, la última operación podía cambiar entre leer la pila y aplicar, y un repo ocupado gastaba un previo para acabar en `repo-busy` |
| D10 | `ref-in-use` cuando una rama a mover está sacada fuera del ámbito | Ajuste del Arquitecto: moverla cambiaría la historia de otro worktree sin tocar sus archivos |

## 4. Plan de tests

`crates/core/tests/us_tmc_002.rs` (macOS, como sus hermanos del canal): fixture del testkit, perfil temporal, daemon real en proceso con catálogo de test (pasos de Git reales por `StepCtx::spawn`), cliente real por el socket y huella del testkit (`Snapshot::take`/`diff`) para "no cambia". Sin esperas por tiempo: cada paso espera la respuesta del daemon.

| Escenario Gherkin | Test |
|---|---|
| Deshacer la última operación del worktree actual | `undo_recovers_the_last_operation_of_the_worktree`: `reset --hard` descarta `a.rs` sin commitear en `feat-login`; el undo lo recupera; el oplog tiene el undo `finished`, solicitante "sin atribuir", canal `cli` y `Target::Undo([op])` |
| El undo queda protegido por su propio punto previo | `the_undo_has_its_own_prior_point`: el previo del undo es un snapshot disponible y verificado con el estado de `feat-login` justo antes del undo |
| Undos seguidos retroceden una operación más cada vez | `consecutive_undos_walk_back`: commit "A" y checkout "B"; el primer undo deshace B (HEAD), el segundo A (rama y archivos); la huella del worktree y de la rama es la de antes de A |
| El undo no actúa sobre otros worktrees | `undo_never_touches_other_worktrees`: operación en `feat-login` y después un commit en `feat-pagos`; el undo desde `feat-login` deshace la suya y la huella de `feat-pagos` (worktree y rama) no cambia |
| No hay nada que deshacer | `nothing_to_undo_changes_nothing`: `OPERATION_REJECTED` `nothing-to-undo`; huella igual; el rechazo queda en el oplog |
| Si el punto previo al undo falla, el undo no se ejecuta | `a_failed_prior_means_no_undo`: `ENOSPC` inyectado; `PRIOR_SNAPSHOT_FAILED` `no-space`; huella igual; undo `aborted` |

Además: `an_undo_with_a_git_operation_in_progress_is_rejected` (precondición antes del previo, sin gastar un punto), `a_branch_checked_out_elsewhere_is_never_moved` (`ref-in-use`), unitarios de la regla de permisos y de los motivos (`undo::tests`) y de los textos en/es (`catalogs_match`, `every_rejection_has_a_message`, raíz del worktree desde una subcarpeta). `RefScope::All` mantiene verde `tm_apply.rs`; `RefScope::Only` lo prueba el escenario 4.

**E2E del mecanismo del criterio 3 de M1** (`apps/cli/tests/undo_process.rs`): daemon real en proceso con el catálogo de test; una operación del catálogo hace `git reset --hard` sobre trabajo sin commitear; `raptor undo` (binario real, cwd en una subcarpeta del worktree) lo recupera, con la salida en inglés; un segundo `raptor undo` no cambia nada y lo dice en español (undos seguidos retroceden; deshacer el undo es redo, US-TMC-003); `--json`; y, por ajuste del PO, el límite aceptado para M1: un "sin atribuir" no deshace trabajo de un agente (`confirmation-required`, repo intacto).

## 5. Verificación

macOS (Apple Silicon): `cargo clippy --all-targets -- -D warnings` y `cargo test --workspace`. Linux y Windows: los tests del canal son solo macOS, igual que sus hermanos; el aplicador devuelve `Unsupported` en Windows. **Pendiente: etapa de validación multiplataforma.**

## 6. Pendientes e interfaces preparadas

| Pendiente | Dónde | Interfaz que queda |
|---|---|---|
| Git crudo en la pila (el undo que más usará el dogfooding) | US-TMC-004 | `undo_stack(.., external)` ya recibe los eventos; el destino de un `GitEvent` es la última captura válida anterior (ADR-TMC-003 § 4); hoy esa rama responde `raw-git-not-covered`. **Las escrituras del aplicador deben llegar marcadas como causadas por el undo** (`ExternalEvent.caused_by`): si no, la captura continua vería el efecto del undo como Git crudo y el siguiente undo haría otra cosa |
| Redo | US-TMC-003 | `UndoStack::next_redo`; `TmRejectReason` y `UndoStep` reutilizables con otro destino |
| Confirmación interactiva | US-TMC-013 | `confirmation-required` y `ChallengeBook` (TS-TMC-004) |
| Solape y Guardrails | US-TMC-012, US-TMC-021 | Punto de enganche en `undo::plan` entre la regla base y las precondiciones |
| `--since`, `--agent`, por id | US-TMC-010/011 | Parámetros ya validados en el contrato |
| Recrear un worktree borrado por la operación deshecha | US-TMC-009 | El aplicador ya recrea (`recreate_id`); hoy un worktree ausente se rechaza con `worktree-unavailable` |
| Undo por MCP de un agente | DEP-MCP-4 / US-GRP-009 | Allowlist deny-all y cwd ilegible en macOS: hoy `SCOPE_REFUSED` |
| Worktree movido (`git worktree move`) | Límite conocido | La pila se indexa por la ruta canónica: tras moverlo, el undo responde `nothing-to-undo` |

**Pregunta abierta 1 del backlog (§ Hito M1)**: sí, la pila de `raptor undo` alcanzará el Git crudo que capture US-TMC-004: `Oplog::undo_stack` ya intercala los eventos externos del motor por su marca, y su destino es la última captura válida anterior al evento (ADR-TMC-003 § 4). US-TMC-004 debe aportar esos eventos, ese destino y la marca `caused_by`. No hace falta US-TMC-009 para el criterio 3.

## 7. Validación

**Arquitecto (2026-10-05)**: aprobada con ajustes, con dos bloqueantes. Todo está incorporado:

1. **Concurrencia (bloqueante)**: lock del repo en cola desde la pila hasta el cierre y `apply_holding` (D9, § 2.1 paso 3).
2. **Criterio 3 (bloqueante)**: "es el criterio 3" sobreprometía. Ahora la Dev Spec dice que entrega el mecanismo y el PO decidió el alcance (abajo).
3. D1: el resultado tipado del aplicador lo guarda el paso y lo lee el undo; el undo construye su propia `ProtectedRequest` de tipo `undo` con `Target::Undo`, y las raíces del oplog se validan contra los worktrees registrados (§ 2.2).
4. D2: `Scope.refs` del undo con el conjunto calculado; nombres validados (SEC-TMC-14); `ref-in-use` (D10).
5. D6: comparación por sesión.
6. `caused_by` para US-TMC-004 y el límite del worktree movido (§ 6). Orden de `tm_command` con selectores, alineado con el código (§ 2.1 paso 1). SEC-TMC-12 (§ 2.4).

**PO (2026-10-05)**: aprueba con ajustes, incorporados:

1. US-TMC-002 entrega el mecanismo; el e2e es su evidencia, no la del criterio 3, que pide uso real. El e2e prueba también el rechazo `confirmation-required` (§ 4).
2. El criterio 3 se cierra con US-TMC-004, por la vía que dé la atribución real del `reset --hard` capturado: atribuido a la sesión de Claude, lo deshace el propio agente desde su shell; "sin atribuir", lo deshace Rene sin confirmación. Condición para US-TMC-004: el Git crudo capturado debe llegar a la pila de `raptor undo` (si solo se recuperara con una restauración, avisar antes de cerrar M1).
3. US-TMC-013 queda fuera de M1. Riesgo a anotar en el backlog por quien cierre M1: "en M1, Rene no puede deshacer trabajo atribuido a un agente; lo pide el propio agente". El texto de `confirmation-required` en la CLI ya lo dice.
