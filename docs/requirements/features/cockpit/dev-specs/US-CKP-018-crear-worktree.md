---
id: DS-US-CKP-018
title: "Dev Spec — US-CKP-018: preparar desde la TUI un worktree para un agente nuevo"
type: dev-spec
status: approved
created: 2026-10-28
updated: 2026-10-28
story: US-CKP-018
feature: cockpit
domain: GRP
scope: fullstack
frontend_surface: true
stack: rust
profile: backend-service
tooling: [cargo, node]
related:
  context: ../context.md
  story: ../user-stories/US-CKP-018-crear-worktree.md
  adrs: [ADR-CKP-002, ADR-CKP-003, ADR-GRP-001, ADR-GRP-007, ADR-GRP-008, ADR-GRP-016, ADR-TMC-004, ADR-TMC-005, ADR-GRD-003]
  api_spec: null
  design_spec: null
  contracts: [../../mcp/dev-specs/US-MCP-019-dev-spec.md, ./TS-CKP-002-catalogo-ejecutor.md, ./TS-CKP-003-capa-cockpit-guardrails.md, ./US-CKP-014-integrar-rama-agente.md, ./US-CKP-017-descartar-worktree.md]
must_read:
  - ../user-stories/US-CKP-018-crear-worktree.md
  - ../business-rules.md
  - ../../mcp/dev-specs/US-MCP-019-dev-spec.md
  - ./US-CKP-014-integrar-rama-agente.md
  - ./US-CKP-017-descartar-worktree.md
  - ./TS-CKP-002-catalogo-ejecutor.md
  - ./TS-CKP-003-capa-cockpit-guardrails.md
  - ./TS-CKP-005-biblioteca-componentes-tui.md
  - ../../../../architecture/decisions/ADR-CKP-002-catalogo-operaciones-ejecutor.md
  - ../../../../architecture/decisions/ADR-CKP-003-arquitectura-tui.md
  - ../../../../architecture/extender-sin-archivos-compartidos.md
  - ../../../../design-system/README.md
  - ../../../../../crates/api/src/catalog.rs
  - ../../../../../crates/api/src/methods/operation.rs
  - ../../../../../crates/api/src/messages.rs
  - ../../../../../crates/core/src/executor/mod.rs
  - ../../../../../crates/core/src/executor/new_path.rs
  - ../../../../../crates/core/src/executor/ops/mod.rs
  - ../../../../../crates/core/src/executor/ops/test_arms.rs
  - ../../../../../crates/core/src/timemachine/undo.rs
  - ../../../../../crates/core/src/channel/conn.rs
  - ../../../../../apps/cli/src/guard/confirm.rs
  - ../../../../../apps/cli/src/tui/keymap.rs
  - ../../../../../apps/cli/src/tui/update.rs
  - ../../../../../apps/cli/src/tui/input.rs
  - ../../../../../apps/cli/src/tui/widgets/mod.rs
  - ../../../../../apps/cli/src/tui/widgets/confirm.rs
lineage:
  supersedes: []
  superseded_by: []
  migration_adr: null
  migration_guide: null
constitution_gates: []
validation:
  must_read_resolved: true
  gaps_blocking: 3
  blocked_tasks: [T001, T002, T003, T004, T005, T006]
  ready_to_implement: false
  gaps_release: 3
  ready_to_release: false
tags: [cockpit, tui, create-worktree, crear-worktree, operacion-protegida, deshacer, ejecutor, guardrails, h-02, nfr-01, nfr-02]
---

# DS-US-CKP-018 · Preparar desde la TUI un worktree para un agente nuevo

## Contexto rápido

El desarrollador pulsa `w` sobre un repo, escribe el nombre de una rama nueva y, si quiere, una ruta. GitRaptor crea la rama desde la base confirmada y su worktree en la ruta indicada, en la de la plantilla del perfil o en `<padre>/<repo>-<rama>`. Al terminar, la TUI muestra la ruta y la orden para lanzar Claude Code, sin lanzar nada. Durante 5 s, `u` deshace: borra el worktree y la rama creados. Después, el panel remite a `D` en su fila.

Es la misma operación del catálogo que usa el agente por MCP. [DS-US-MCP-019](../../mcp/dev-specs/US-MCP-019-dev-spec.md) construye la parte común: el brazo de producción de `create-worktree` con la plantilla, el sitio del daemon, los motivos `branch-exists`, `new-path-exists`, `base-unconfirmed` y `base-missing`, la capacidad `operation.create-worktree` y `OperationRunResult.created`. Esta DS no repite nada de eso. Añade la ruta explícita de una conexión completa (D1 de DS-MCP-019 se la reserva a esta historia), la acción de la TUI y su Deshacer.

Hoy:
- `create-worktree` está en el catálogo (`crates/api/src/catalog.rs:188-194`, no destructiva, gobernada como `CreateWorktree`, marcas Cockpit y MCP, `implemented_by = US-CKP-018`). `CreateWorktreeArgs { branch, path: Option<String> }` existe (`catalog.rs:246-250`); por MCP, `McpCreateWorktreeArgs` no tiene `path` (`catalog.rs:255-257`, `:319-325`).
- `OperationArgs::parse` ya valida la rama con `check_new_branch` (`catalog.rs:595`) y la ruta con `check_absolute` (`catalog.rs:530-545`): absoluta, sin `..`, sin prefijo `\\` ni `//` (UNC) y como mucho `MAX_PATH_BYTES` = 4096 (`catalog.rs:41`). Un fallo es `-32602 invalid-params` con el campo.
- `check_new_worktree_path` existe (`crates/core/src/executor/new_path.rs:50`), con `NewPathError` (`:12-26`). En Windows, `parent_is_private` devuelve siempre `true` (`:41-45`, G1 de DS-MCP-019).
- El catálogo de producción solo arma `commit` y `snapshot` (`executor/ops/mod.rs:27`, `ARMS`). `create-worktree` solo lo sirve el brazo de prueba (`ops/test_arms.rs:111-120`), que exige ruta y parte del `HEAD` del llamante.
- **Nada de DS-US-MCP-019 está en `main`** (comprobado el 2026-10-28): no existen `ops/create_worktree.rs`, `crates/api/src/worktree_template.rs`, `CAP_OPERATION_CREATE_WORKTREE` ni `OperationRunResult.created` (`crates/api/src/timemachine.rs:149-167`). G1.
- **DS-US-CKP-014 tampoco está en `main`**: no existen `tui/write_flow.rs`, `OpRequest`/`OpReply` ni `tui/undo.rs`. G2.
- **Deshacer genérico de un `create-worktree` no funciona**: el ámbito del oplog es el worktree nombrado más `refs/heads/<rama>`, y el worktree nuevo queda fuera. El plan del undo ve la rama sacada en un worktree fuera del ámbito y rechaza con `ref-in-use` (`crates/core/src/timemachine/undo.rs:556-583`). D5 lo resuelve sin tocar la Time Machine.
- El cliente pide todas las capacidades del registro (`crates/core/src/client.rs:77`, `capability::all()`): una capacidad nueva en el `GROUP` de `operation` llega a la TUI sin tocar el cliente.
- La base del repo viaja en `RepoView.base: BaseBranchView { name, status }` (`crates/api/src/messages.rs:143-151`, `:228-245`), con `BaseStatusView::{Confirmed, Unconfirmed, Invalid}`. La alimenta `repo_base` (`crates/core/src/daemon/mod.rs:1307-1313`) desde `confirmed_team_baseline`, la misma fuente que el sitio de DS-MCP-019 (su D3): la TUI y el daemon no pueden discrepar. La base se confirma con `raptor guard confirm [ruta]` (`apps/cli/src/guard/confirm.rs:20`, US-GRD-014).
- La TUI no tiene ningún campo de texto: los widgets son de solo presentación (`apps/cli/src/tui/widgets/mod.rs:1-18`). El pegado llega como `Msg::Paste` (`tui/input.rs:151`) y hoy no tiene destino (`tui/update.rs:45-46`).

Sin `architecture-constitution.md` en la cascada: rigen [AGENTS.md](../../../../../AGENTS.md) y los ADR citados (`unsafe_code = forbid` y escritura con la Git CLI del sistema, ADR-GRP-001; protocolo congelado y códigos en `-32016`, ADR-GRP-016; argv fijo y sin shell, NFR-02; el agente nunca elige la ruta, H-02). Ninguna tarea las viola.

### Decisiones

Se reutiliza sin repetir lo que fija DS-US-MCP-019: plantilla y marcadores cerrados (D2), sitio del daemon (D3), "nunca reutilizar" una rama (D4), motivos del canal (D5), defensa TOCTOU de la ruta (D6), ruta como `Untrusted` acotada a 1024 bytes (D7), `created` con capacidad (D8), operación en curso en el worktree nombrado (D10) y `base-missing` (D11). De DS-US-CKP-014: flujo de escritura y vía del canal (I10), actor de la fila (I13). De DS-US-CKP-017: el brazo `discard-worktree` y su Deshacer (X3, X6, X9, X10).

Revisión del Arquitecto y del PO (2026-10-28): ratificadas C1 a C4, C6, C11, C12 y C15; cambiadas C5, C7, C8, C9, C10, C13 y C14. Cada fila lleva su marca.

| ID | Decisión | Por qué / descartado |
|----|----------|----------------------|
| C1 | **Una sola operación para la TUI y el MCP.** La TUI pide `create-worktree` por `operation.prepare` y `operation.run`, con el mismo brazo (`ops/create_worktree.rs`), el mismo sitio, los mismos motivos y la misma capacidad que DS-US-MCP-019. Si esta historia empieza antes que US-MCP-019, implementa T001 a T005 de DS-US-MCP-019 tal como están escritas, sin reinterpretarlas; T006 y T007 (la herramienta MCP) siguen siendo de US-MCP-019 | DS-MCP-019 D1 ("la construye la primera de las dos historias que entre"). Dos caminos para crear un worktree divergirían en las reglas de H-02 y de D6. **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO** |
| C2 | **Ruta explícita en producción**, solo en conexión completa y solo para la capa `cockpit`. `plan_op` con `args.path = Some` deja de responder `NotImplemented` cuando no se pidieron los brazos de prueba; con ellos pedidos, sigue delegando en `test_arms` (las pruebas de proceso de hoy no cambian). La ruta pasa por las mismas comprobaciones que una ruta de plantilla: forma final (componentes de 1 a 255 bytes, sin nombres de dispositivo de Windows, ≤ 1024 bytes), las tres de `check_new_worktree_path`, `create_dir` y la verificación posterior. No necesita `main_root`: un repo *bare* acepta una ruta explícita | Lo pide BR-CKP-VAL-001 ("acepta una ruta") y D1 de DS-MCP-019 lo deja aquí. Una sola función de forma final para las dos rutas evita que la explícita sea más débil que la de plantilla. **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO** |
| C3 | **Capa**: el ejecutor rechaza `create-worktree` con `path` para una capa distinta de `cockpit` con `not-available-for-layer` (`RejectReason::NotAvailableForLayer`, ya existe), en `Executor::plan` antes de `plan_op`, como la regla de `commit` de la capa `mcp` (`executor/mod.rs:535-546`) | Defensa en profundidad de H-02: hoy solo el esquema MCP quita `path` (`catalog.rs:319-325`), y un agente que hablara el protocolo completo podría elegir la ruta. Descartado: confiar en el esquema. **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO** |
| C4 | **UNC y ruta mal formada se rechazan antes del daemon.** La TUI valida los parámetros con `OperationArgs::parse(OperationId::CreateWorktree, &args, false)`, la misma función que usa el daemon. Un `Invalid` con campo `path` pinta "ruta no válida: usa una ruta local absoluta, sin UNC ni `..`" y no se llama al daemon. El daemon vuelve a validar (`-32602`) | Escenario 6. `check_absolute` ya rechaza `\\…` y `//…` (`catalog.rs:535`). Un solo validador: la TUI no puede aceptar lo que el daemon rechaza. El rechazo de UNC en Windows real (incluido `\\?\`) se valida en la etapa multiplataforma (XP-57). **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO** |
| C5 | **Deshacer = descartar el worktree creado**, y solo desde el panel de resultado (ventana de 5 s, C7): `u` lanza `discard-worktree` (DS-US-CKP-017) sobre la raíz de `created.path`, por el mismo `write_flow`. No usa `timemachine.undo`. Una rama recién creada, sin cambios, está integrada (su punta es la de la base o un antepasado), así que el descarte es `Affected::Nobody` y corre sin prompt (DS-017 X3). Si el desarrollador ya escribió algo, el descarte pide su confirmación y toma el previo garantizado del worktree nuevo; el descarte queda en el oplog y se puede deshacer (DS-017 X10). **Fuera del panel**: mientras el worktree creado exista, `raptor undo` o `u` sobre el principal responden `ref-in-use` (`undo.rs:563-581`); la CLI lo traduce a "esta operación creó un worktree; bórralo desde raptor (D en su fila)" sin cambiar el protocolo, y el historial (DS-021) muestra la entrada con `u` desactivado (Q-CKP-58, cerrada). Una rama nueva que casa con una regla de rama protegida no se deshace desde el panel (Q-CKP-57, cerrada; C7) | El undo genérico rechaza con `ref-in-use` (`undo.rs:556-583`) y la Time Machine no puede borrar un worktree sin porcelana (SEC-TMC-02). El descarte ya tiene previo, precondiciones (sesión presente, bloqueado) y Deshacer propio: NFR-01 sin código nuevo en la Time Machine. Descartados: un undo nativo de `create-worktree` en la Time Machine (otra recreación inversa, otra historia de la TM), `git worktree remove` desde la TUI (sin previo) y saltarse `protected-branch` en este descarte. Lo que hace `raptor undo` en el principal tras crear y tras el descarte es G6. **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO (revisión, con cambios)** |
| C6 | **Deshacer espera a la fila.** El descarte nombra un worktree observado. Si al pulsar `u` la instantánea aún no tiene la fila de `created.path`, la petición queda pendiente y sale en cuanto aparece, mientras siga la ventana de 5 s; si no aparece, "el worktree aún no aparece; descártalo con D desde su fila" | ⚠️ **ASSUMPTION**: el motor publica el worktree nuevo dentro del presupuesto de frescura de ADR-GRP-011, que es menor de 5 s. Descartado: nombrar una ruta no observada (el canal la rechaza, `conn.rs:3270-3280` y `repo_of`). **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO** |
| C7 | **Resultado en un panel que se queda**, no en un toast de 5 s: "Worktree listo", la rama, la ruta y "Para lanzar Claude Code: cd <ruta> && claude". `u Deshacer` vive 5 s dentro del panel, con cuenta atrás (BR-CKP-WF-002). Al vencer, el panel sigue abierto y dice "para borrarlo: D en su fila". Si la rama nueva casa con una regla de rama protegida (`WorktreeView.protected_branch` de la fila, DS-017 X16), `u` sale desactivado desde el principio: "rama protegida: bórrala con D desde su fila cuando la regla lo permita". `Esc` lo cierra; cualquier otra acción de escritura también. La TUI no lanza ningún proceso | El desarrollador tiene que poder copiar la orden con calma; un toast de 5 s no basta. BR-CKP-ELIG-005: "no lanza al agente". PO: el panel nunca ofrece un Deshacer que va a fallar. **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO (revisión, con cambios)** |
| C8 | **La orden de lanzamiento se cita para el shell.** Si la ruta solo tiene `[A-Za-z0-9/._+@%,:=-]`, va tal cual (el ejemplo de la historia). Si no (también con `~`, que el shell expandiría), entre comillas simples POSIX, con cada `'` como `'\''`. Si `SafeText` tuvo que neutralizar algo de la ruta (control, bidi, UTF-8 no válido), no se muestra orden: "la ruta tiene caracteres que no se pueden copiar con seguridad", y solo la ruta saneada. El agente es siempre `claude` (MVP, D2 de AGENTS.md) | Una ruta con `;` o `$(…)` copiada y pegada sería una orden distinta: la TUI no debe fabricar una inyección (SEC-12). La cita de `cmd` y PowerShell queda para la etapa multiplataforma (XP-58). **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO (revisión, con cambios)** |
| C9 | **Tecla `w`** ("nuevo worktree"), acción de repo: actúa sobre el repo de la fila seleccionada, o sobre el del encabezado si no hay filas. Ocupadas por otras specs: `a`, `c` (Cancelar, DS-016), `d` (DS-012), `D` (DS-017), `e` (DS-013), `m`, `o`/`u` (DS-014), `r`, `t` (DS-021), `x` (DS-019), `?`; y en el mapa de hoy `q`, `k`/`j`, `Enter`, `Ctrl-Z`, `y`/`s`, `n`, `Esc`, `f` (`keymap.rs:84-136`) | Decisión común de teclas de la revisión. `c` es Cancelar de DS-016; el test `keymap.rs:188`, que fija `c` sin acción, no cambia aquí. **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO (revisión, con cambios)** |
| C10 | **Formulario modal de dos campos**, `CreateFormModel` (a crear) según la convención de TS-CKP-005: modelo de vista puro; el texto y el foco viven en `Model.ui`. "Rama" (obligatoria) y "Ruta" (opcional). Bajo "Ruta", la ruta por defecto calculada con la rama escrita: "vacía: /code/shop-feat-pagos" (`worktree_path_from_template(None, …)` de DS-MCP-019 T001 sobre la raíz del principal). ⚠️ **ASSUMPTION**: la TUI no lee la plantilla del perfil (sería un segundo lector); el texto añade "o la plantilla del perfil, si la hay" y manda el daemon. `Tab`/`Shift-Tab` cambian de campo, `Backspace` borra, `Enter` envía, `Esc` cancela. `Ctrl-C` sale y `Ctrl-Z` suspende también con el formulario abierto; el resto de teclas, `q` incluida, se escriben en el campo y no van al mapa de acciones. Solo entran caracteres imprimibles; un pegado con cualquier carácter de control se ignora entero con un aviso. Topes al escribir: 200 bytes la rama (`MAX_BRANCH_BYTES`), 1024 la ruta (D7 de DS-MCP-019). Un `~/` al principio de la ruta se expande con el `HOME` de la TUI, igual que el marcador `~/` de la plantilla; ninguna otra expansión | El primer campo de texto de la TUI. Descartado: dos prompts seguidos (no se puede volver a la rama tras escribir la ruta). Descartado aceptar `~usuario` o variables: la misma lista cerrada de D2 de DS-MCP-019. **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO (revisión, con cambios)** |
| C11 | **Sin prompt de confirmación.** `create-worktree` no es destructiva, el plan es `Affected::Nobody` y sin avisos (DS-MCP-019 T004, paso 2.7), así que tras `Prepared` el flujo pasa a `Run` sin preguntar. El formulario enviado es la intención. Si algún día el plan trae avisos o reto, el `write_flow` genérico muestra su ConfirmPrompt | Mismo criterio que BR-CKP-ELIG-004 y DS-017 T006 (sin avisos ni reto, sin prompt). **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO** |
| C12 | **El worktree nombrado es el principal del repo** (`RepoView.worktrees[0]` si es el principal y está disponible); si no lo hay (repo *bare*) o no está disponible, el primer worktree disponible. Una operación en curso o un bloqueo en él rechaza la creación (`check_common`, DS-MCP-019 D10) | La plantilla parte del worktree principal sea cual sea el nombrado (DS-MCP-019 D3, `main_root`), así que elegir la fila seleccionada no aporta y haría que crear dependiera de en qué fila está el cursor. **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO** |
| C13 | **Elegibilidad de "Crear worktree"** como función pura, de repo. Orden: fuera de "En vivo" → `NotLive`; capa distinta de `cockpit` → `AgentLayer`; `base.status != Confirmed` (incluido `Invalid`) **o** `base.pending.is_some()` (`BaseBranchView.pending`, capacidad `engine.base-pending`, DS-US-GRP-016) → `BaseUnconfirmed`, con la acción `raptor guard confirm`; sin worktree que nombrar (C12) → `NoWorktree`; operación en curso en el worktree nombrado → `InProgress`. El daemon revalida todo al preparar | Escenario 5 y BR-CKP-WF-005. El texto "confírmala con `raptor guard confirm`" sigue el precedente de DS-022 D5 (Q-CKP-49). Sin la capacidad `engine.base-pending`, `pending` no viaja y la regla se queda en el estado. **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO (revisión, con cambios)** |
| C14 | **Base pendiente también en el daemon**, para todas las capas: con `pending` presente, el sitio de DS-MCP-019 responde `base-unconfirmed` (motivo existente). Enmienda condicional a DS-MCP-019 D3 y a DS-US-GRP-016; hasta que entre, solo la TUI desactiva (G5) | Arquitecto y PO coinciden: crear desde una base que el equipo ya cambió repartiría ramas nuevas desde la vieja, por MCP igual que por la TUI. Descartado: dejarlo como precondición solo de presentación. **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO (revisión, con cambios)** |
| C15 | **`OpRequest::Prepare` gana `args`** (`serde_json::Map`, vacío para las operaciones sin argumentos) | `write_flow` (DS-014 T005) no lleva argumentos y `create-worktree` los necesita. Cambio aditivo en un tipo interno de la TUI, no del protocolo. Enmienda a DS-014. **Decisión del orquestador (2026-10-28), validada por Arquitecto y PO** |

## ⚠️ Gaps y violaciones de la constitución

| ID | Qué falta | Severidad | Alcance | Acción | Owner |
|----|-----------|-----------|---------|--------|-------|
| G1 | **Parte común de DS-US-MCP-019** (T001 a T005): brazo `create-worktree`, sitio, plantilla, motivos, capacidad y `created`. Nada está en `main` (2026-10-28) | Bloqueante | T001, T002, T005, T006 | Mezclar T001 a T005 de DS-US-MCP-019 antes, o implementarlas aquí tal como están escritas (C1) | Orquestador |
| G2 | **Flujo de escritura de DS-US-CKP-014** (T005 `write_flow`, `OpRequest`/`OpReply`; T007 `tui/undo.rs`) y, para Deshacer, **el brazo `discard-worktree` de DS-US-CKP-017** (T002, T003). Ninguno está en `main` | Bloqueante | T003 a T006 (T006 solo por DS-017) | Mezclar DS-014 T005 y T007 antes de T003; DS-017 T002 y T003 antes de T006 | Orquestador |
| G3 | **Windows**: `parent_is_private` siempre `true` (G1 de DS-MCP-019), rechazo de UNC y `\\?\` en Windows real (XP-57) y cita de la orden de lanzamiento para `cmd`/PowerShell (XP-58) | Bloquea liberación en Windows | Solo Windows | Etapa de validación multiplataforma | Arquitecto |
| G4 | G1 de DS-US-CKP-014 (avisos de `timemachine.notices` tras un Deshacer, US-TMC-019) aplica igual al descarte que hace de Deshacer | Bloquea liberación | — | Cerrar con US-TMC-019 | Orquestador |
| G5 | **Rechazo de base pendiente en el daemon** (C14): el sitio de DS-MCP-019 responde `base-unconfirmed` con `pending` presente. Depende de DS-MCP-019 D3 y de DS-US-GRP-016 (`BaseBranchView.pending`). Hasta entonces solo la TUI desactiva | Bloquea liberación | — | Enmienda condicional a DS-MCP-019 D3 y DS-US-GRP-016 | Orquestador |
| G6 | **`raptor undo` en el principal tras crear y tras el descarte que hace de Deshacer**: hoy `ref-in-use` (`undo.rs:563-581`) mientras exista el worktree creado; tras el descarte, qué deshace `raptor undo` en el principal (la creación ya no tiene worktree que la bloquee) no está fijado | Informativo | — | Fijarlo en la Time Machine o en DS-MCP-019; esta DS solo traduce el texto de la CLI y lo prueba (`undo_in_main_after_create_reports_why`) | Time Machine / DS-MCP-019 |

Ninguna violación: sin constitución, ver Contexto rápido.

## 🔭 La forma

```mermaid
sequenceDiagram
    autonumber
    participant T as TUI (update + write_flow)
    participant F as tui/create_worktree.rs
    participant D as daemon (executor)
    participant A as ops/create_worktree.rs (DS-MCP-019)
    participant G as git (hijo directo)
    T->>F: w sobre el repo: create_eligibility (T003)
    F-->>T: formulario Rama / Ruta (T004)
    T->>T: OperationArgs::parse local (C4); inválido → texto, nada enviado
    T->>D: operation.prepare create-worktree {branch, path?} (worktree = principal, C12)
    D->>D: capa cockpit si hay path (C3, T002)
    D->>A: plan_op: base, rama libre, ruta (explícita o plantilla), check_new_worktree_path
    D-->>T: PrepareResult sin avisos ni reto
    T->>D: operation.run sin prompt (C11)
    D->>A: replan bajo el cerrojo, Guardrails, previo, create_dir, worktree add -b, verificación
    A->>G: run_git
    D-->>T: OperationRunResult {created: {path, branch}}
    T->>T: panel "Worktree listo" + orden citada (C7, C8)
    T->>D: u → operation.prepare discard-worktree {worktree: created.path} (C5, C6, T006)
    D-->>T: descarte sin prompt (integrado y limpio) → worktree y rama borrados
```

---

## 🚀 Plan de implementación

### El trabajo de un vistazo

| # | Tarea | Depende | Aterriza en |
|---|---|---|---|
| T001 | Escribir en rojo los tests de contrato del daemon y de la TUI | G1, G2 | `crates/core/tests/us_ckp_018_create.rs`, `apps/cli/tests/tui_create_worktree.rs`, stubs |
| T002 | Servir la ruta explícita y fijarla a la capa `cockpit` | T001 | `crates/core/src/executor/ops/create_worktree.rs`, `crates/core/src/executor/mod.rs`, `crates/api/src/worktree_template.rs` |
| T003 | Crear la elegibilidad de "Crear worktree" en la TUI | T001 | `apps/cli/src/tui/create_worktree.rs` |
| T004 | Crear el formulario y su edición | T003 | `apps/cli/src/tui/widgets/create_form.rs`, `apps/cli/src/tui/create_worktree.rs` |
| T005 | Pintar la acción, enviar y mostrar el resultado, con textos en/es | T002, T004 | `tui/{update,view,help,keymap,mod}.rs`, `present/create_worktree_text.rs`, `present/write_text.rs`, `tui/write_flow.rs` |
| T006 | Deshacer desde el panel con un descarte | T005, G2 (DS-017) | `apps/cli/src/tui/undo.rs`, `apps/cli/src/tui/create_worktree.rs` |
| T007 | Cerrar la historia | T001–T006 | `docs` |

T002 y T003 van en paralelo. T006 es lo único que espera a DS-017.

### T001 — Escribir en rojo los tests de contrato

**Objetivo.** Los seis escenarios de la historia, en rojo, contra el daemon real con repos y perfil temporales, y contra la `App` real sobre `TestBackend`.

**Ubicación.**
- `crates/core/tests/us_ckp_018_create.rs` (**CREATE**): daemon en proceso con `OperationsWiring::production()`, sin brazos de prueba, Git real; la base se confirma como en `crates/core/tests/profile_team_baseline.rs`.
- `apps/cli/tests/tui_create_worktree.rs` (**CREATE**): arnés de `apps/cli/tests/live_fleet.rs` (`#![cfg(all(target_os = "macos", debug_assertions))]`, `gitraptor_testkit::Fixture`).
- Stubs que compilan (`todo!()` o `PlanError::NotImplemented`) en los archivos de T002 a T006.

**Reglas**
- Los tests rojos van en su archivo y no se editan durante la implementación (R4 de `/implement`).
- Nunca este repo ni el perfil real (NFR-01). El padre de los repos temporales con modo `0o755` (lo exige `parent_is_private`, `new_path.rs:35-39`).
- La capa `cockpit` sale de la resolución real del solicitante, nunca de `test_layer_override` en los tests de punta a punta.

- **Depende:** G1, G2
- **Refs:** US-CKP-018 escenarios 1 a 6; § 9.4
- **Aceptación:** `cargo test -p gitraptor-core --test us_ckp_018_create` y `cargo test -p gitraptor-cli --test tui_create_worktree` compilan y fallan por "no implementado"

### T002 — Servir la ruta explícita y fijarla a la capa `cockpit`

**Objetivo.** Una conexión completa de la capa `cockpit` crea el worktree en la ruta que indica, con las mismas comprobaciones que la ruta de plantilla.

**Ubicación.**
- `crates/core/src/executor/ops/create_worktree.rs` (**MODIFY**, lo crea DS-MCP-019 T004): rama `args.path = Some` de `plan_op`.
- `crates/api/src/worktree_template.rs` (**MODIFY**, lo crea DS-MCP-019 T001): `pub fn check_final_path(&Path) -> Result<(), TemplateError>`, extraída de `PathTemplate::render` (la usan las dos rutas).
- `crates/core/src/executor/mod.rs` (**MODIFY**): la regla de capa de C3 en `Executor::plan` (`:518-573`).

**Reglas** — `plan_op` con `args.path = Some(p)` (sustituye el paso 2.1 de DS-MCP-019 T004 solo cuando no se pidieron los brazos de prueba)
1. Brazos de prueba pedidos → `test_arms::plan_op`, como hoy.
2. Los pasos 2.2 a 2.4 de DS-MCP-019 T004 sin cambios: `BaseUnconfirmed`, `BaseMissing`, `BranchExists`.
3. La ruta es `p` (ya absoluta y sin `..`: `check_absolute` corrió al parsear). `check_final_path(p)` y longitud ≤ 1024 bytes; si falla → `NewPathRefused`. No se renderiza ninguna plantilla y no se mira `main_root`.
4. `check_new_worktree_path(p, &site.protected)`: `Exists` → `NewPathExists`; el resto → `NewPathRefused` (DS-MCP-019 ⛔1.1).
5. El `OpPlan` y el paso, idénticos a los de la ruta de plantilla: la ruta canónica entra en la huella, `create_dir` no recursivo, `worktree add -b`, verificación posterior y nunca `remove_dir_all` (DS-MCP-019 D6, ⛔4.2).

**Reglas** — capa (C3)
- En `Executor::plan`, antes de `backend.plan_op`: `op == CreateWorktree`, `args` con `path` y `layer != Layer::Cockpit` → `RejectReason::NotAvailableForLayer`. Se evalúa al preparar y otra vez bajo el cerrojo, porque `run` rehace el plan.

- **Depende:** T001, G1
- **Refs:** C2, C3; DS-MCP-019 D2, D6, D7, T004; ADR-CKP-002 § 1 ("Ruta nueva") y § 5
- **Aceptación:** `cargo test -p gitraptor-core --lib executor::ops::create_worktree` y `cargo test -p gitraptor-core --test us_ckp_018_create` en verde, y siguen en verde `--test channel_protected`, `--test ts_ckp_003_deadlock` y `cargo test -p gitraptor-cli --test guard_ts_ckp_003` (usan el brazo de prueba con ruta), con:
- [ ] Ruta explícita que existe → `new-path-exists`; padre enlace simbólico → `new-path-refused`; dentro de un `.git` o de otro worktree → `new-path-refused`.
- [ ] Ruta explícita con un componente `CON` o de 256 bytes → `new-path-refused`, sin llamar a `git`.
- [ ] Capa `mcp` por conexión completa con `path` → `not-available-for-layer`, nada creado.
- [ ] Repo *bare* con ruta explícita → creado; sin ruta → `new-path-refused` (DS-MCP-019 T003).

### T003 — Crear la elegibilidad de "Crear worktree" en la TUI

**Objetivo.** La columna "Crear worktree" de BR-CKP-ELIG-001 como función pura, con el motivo y la acción que desbloquea.

**Ubicación.** `apps/cli/src/tui/create_worktree.rs` (**CREATE**), una línea en `tui/mod.rs`.

**Reglas**
- `create_eligibility(repo: &RepoView, layer: Option<Layer>, live: bool) -> CreateEligibility`. Orden (el primero que falla manda), C13:
  1. Fuera de "En vivo" → `NotLive`.
  2. Capa distinta de `cockpit` → `AgentLayer`.
  3. `repo.base.status != Confirmed` o `repo.base.pending.is_some()` → `BaseUnconfirmed`, con `unblock = Some(Unblock::ConfirmBase)`.
  4. Ningún worktree disponible que nombrar (C12) → `NoWorktree`.
  5. Operación en curso en el worktree nombrado → `InProgress`.
- `named_worktree(repo: &RepoView) -> Option<&WorktreeView>`: la regla de C12.
- Sin texto: devuelve códigos. Reutiliza `Eligibility`/`Unblock` de DS-014 T004 si su forma lo admite; si no, `CreateEligibility::{Offered { worktree }, Disabled { reason: CreateBlock, unblock: Option<Unblock> }}`.

- **Depende:** T001, G2
- **Refs:** C12, C13; BR-CKP-ELIG-001, BR-CKP-ELIG-005, BR-CKP-WF-005
- **Aceptación:** `cargo test -p gitraptor-cli --lib tui::create_worktree`: un test por regla y uno del orden

### T004 — Crear el formulario y su edición

**Objetivo.** El primer campo de texto de la TUI: rama y ruta, con validación local antes de enviar.

**Ubicación.**
- `apps/cli/src/tui/widgets/create_form.rs` (**CREATE**), una línea en `widgets/mod.rs`: `CreateFormModel`, componente puro (TS-CKP-005).
- `apps/cli/src/tui/create_worktree.rs` (**MODIFY**): `CreateForm` (estado en `Model.ui`) y `on_form_key`, `on_form_paste`, `submit`.

**Reglas**
- Estado: `CreateForm { branch: String, path: String, focus: Field, note: Option<FormNote> }`. Vive en `Model.ui.create`; abierto solo con `create_eligibility` = `Offered`.
- Teclas con el formulario abierto (no pasan por `keymap::action`): carácter imprimible → se añade si cabe en el tope (C10), `q` incluida; `Backspace`; `Tab`/`Shift-Tab`; `Enter` → `submit`; `Esc` → cerrar sin efectos. Excepciones: `Ctrl-C` sale de la TUI y `Ctrl-Z` suspende, como sin formulario.
- Bajo el campo "Ruta", con la rama escrita y válida: "vacía: <ruta por defecto>", calculada con `worktree_path_from_template(None, …)` sobre la raíz del principal y `sanitized_branch`; "o la plantilla del perfil, si la hay" detrás (C10, ⚠️ ASSUMPTION). Sin rama válida, solo "vacía: junto al repo o la plantilla del perfil".
- Pegado (`Msg::Paste`): si el texto tiene algún carácter de control (`char::is_control`, incluidos `\n` y `\t`) o bidi, se ignora entero y `note = PasteRefused`; si no, se añade hasta el tope.
- `submit`: `args = {"branch": rama}` y, si la ruta no está vacía, `"path"` con `~/` inicial expandido con el `HOME` de la TUI. `OperationArgs::parse(OperationId::CreateWorktree, &args, false)`: `Invalid` con campo `branch` → `note = InvalidBranch`; con campo `path` → `note = InvalidPath`; el formulario sigue abierto y no se envía nada (C4). Válido → `OpRequest::Prepare { operation: CreateWorktree, worktree: named_worktree, args, session_env }` (C15) y el formulario se cierra.
- El modelo de vista lleva todos sus textos como `SafeText`; el foco tiene forma (borde más grueso y `›`), no solo color; plegado ASCII y sin color (DS-US-CKP-005).

- **Depende:** T003
- **Refs:** C4, C10, C15; TS-CKP-005; ADR-CKP-003 § 3 (feedback < 100 ms) y § 8 (SEC-12)
- **Aceptación:** `cargo test -p gitraptor-cli --lib tui::create_worktree` y `tui::widgets::create_form` con:
- [ ] `feat..pagos` → `InvalidBranch`, ningún `OpRequest`.
- [ ] `\\servidor\compartido\shop-x` → `InvalidPath`, ningún `OpRequest`.
- [ ] Pegado con `\n` o `\x1b` → ignorado, `PasteRefused`.
- [ ] `~/x` → `"path": "<HOME>/x"`; `~otro/x` → `InvalidPath`.
- [ ] `q` con el formulario abierto se escribe en el campo; `Ctrl-C` sale.
- [ ] Rama `feat/pagos` en el repo `/code/shop` → "vacía: /code/shop-feat-pagos".
- [ ] Snapshots en/es del formulario vacío, con foco en cada campo y con cada nota.

### T005 — Pintar la acción, enviar y mostrar el resultado

**Objetivo.** `w` abre el formulario, el envío prepara y ejecuta sin prompt, y el panel muestra la ruta y cómo lanzar Claude Code.

**Ubicación.**
- `apps/cli/src/tui/keymap.rs` (**MODIFY**): `Action::CreateWorktree` en `w` (C9). El test de `:188` (`c` sin acción) no cambia aquí.
- `apps/cli/src/tui/update.rs` (**MODIFY**): `CreateWorktree` abre el formulario o pinta el motivo; con el formulario abierto, `Msg::Key` y `Msg::Paste` van a `create_worktree::on_form_*`; `EngineMsg::Operation` a `write_flow`.
- `apps/cli/src/tui/write_flow.rs` (**MODIFY**, lo crea DS-014): `OpRequest::Prepare` con `args` (C15).
- `apps/cli/src/tui/{view,help,mod}.rs` (**MODIFY**): la acción en `KeyHints`, desactivada con su motivo; el formulario y el panel como modales (`modal_area`).
- `apps/cli/src/present/create_worktree_text.rs` (**CREATE**): textos en/es y `launch_line`.
- `apps/cli/src/present/write_text.rs` (**MODIFY**, lo crea DS-014): `branch-exists`, `new-path-exists`, `new-path-refused`, `base-unconfirmed`, `base-missing` en su `match` exhaustivo.

**Reglas**
- Tras `Prepared` sin `warnings` ni `challenge` → `Run` directo (C11).
- Tras `Ran` con `outcome = done` y `created` → panel "Worktree listo" (C7): "Rama <rama> desde <base>", "En <ruta>", la línea de `launch_line` y "u Deshacer (<n> s)" durante 5 s; al vencer, "para borrarlo: D en su fila" y el panel sigue abierto. Rama protegida → `u` desactivado desde el principio con "rama protegida: bórrala con D desde su fila cuando la regla lo permita". Sin `created` (daemon sin la capacidad) → "Worktree creado; revisa la flota" sin orden.
- `launch_line(path: &SafeText, intact: bool) -> LaunchLine` (C8): `intact == false` → `Unsafe`; ruta solo con `[A-Za-z0-9/._+@%,:=-]` → `cd <ruta> && claude`; si no (también con `~`), `cd '<ruta con ' → '\''>' && claude`. Texto: "Para lanzar Claude Code: <orden>".
- Cada texto fijo tiene es y en. Los de la historia:

  | es | en |
  |---|---|
  | Worktree listo | Worktree ready |
  | Para lanzar Claude Code: cd <ruta> && claude | To launch Claude Code: cd <path> && claude |
  | nombre de rama no válido | branch name is not valid |
  | la rama ya existe | the branch already exists |
  | la ruta ya existe; elige otra | the path already exists; choose another |
  | confirma la rama base | confirm the base branch |

- Textos de rechazo (C13 y escenarios):
  - `InvalidBranch` → "nombre de rama no válido".
  - `branch-exists` → empieza siempre por la frase de la historia, "la rama ya existe" / "the branch already exists". Solo si la instantánea muestra una rama que difiere de la pedida en mayúsculas o por prefijo de carpeta, se añade " (choca con otra por mayúsculas o por carpeta)" / " (it clashes with another by case or folder)". El daemon no distingue el caso: la TUI lo deduce de lo que ve y, si no ve nada, no añade el paréntesis.
  - `new-path-exists` → "la ruta ya existe; elige otra".
  - `new-path-refused` → "ubicación no válida: la carpeta padre debe existir, ser tuya y no ser un enlace, y quedar fuera de todo .git, del perfil y de otros worktrees".
  - `InvalidPath` (local o `-32602` con campo `path`) → "ruta no válida: usa una ruta local absoluta, sin UNC ni '..'".
  - `base-unconfirmed` (carrera con la instantánea) y "Crear worktree" desactivado por base → "confirma la rama base" y "confírmala con raptor guard confirm".
  - `base-missing` → "la rama base <base> no existe en local; créala o revísala" (el nombre sale de `RepoView.base.name`, no del daemon).
  - `operation-in-progress` y `worktree-locked` del worktree nombrado → los textos únicos de DS-015 T005, DS-016 T007 y DS-017 T006.
  - `guardrails-denied` → PolicyBanner (DS-019).
- Un rechazo de `branch-exists`, `new-path-exists` o `InvalidPath` reabre el formulario con lo escrito y el foco en el campo culpable.
- Nombres y rutas por `SafeText`. Plegado ASCII y sin color (DS-US-CKP-005).

- **Depende:** T002, T004
- **Refs:** C7, C8, C9, C11, C13; BR-CKP-VAL-001, BR-CKP-ELIG-005, BR-CKP-WF-002, BR-CKP-VAL-002
- **Aceptación:** `cargo test -p gitraptor-cli --lib tui` con snapshots en/es del panel (ruta simple, ruta con espacio, ruta con `'`, ruta no segura), de "Crear worktree" desactivado por cada motivo y de cada rechazo; `cargo test -p gitraptor-cli --lib present::create_worktree_text` con la tabla de `launch_line`

### T006 — Deshacer desde el panel con un descarte

**Objetivo.** `u` en el panel borra el worktree y la rama creados, con previo y Deshacer propio (C5).

**Ubicación.**
- `apps/cli/src/tui/undo.rs` (**MODIFY**, lo crea DS-014 T007): para un resultado de `create-worktree`, `u` envía `OpRequest::Prepare { operation: DiscardWorktree, worktree: created.path, args: {} }` y no `OpRequest::Undo`.
- `apps/cli/src/tui/create_worktree.rs` (**MODIFY**): `PendingUndo { root, until }` (C6).
- `apps/cli/src/commands/undo.rs` (**MODIFY**): el texto de `ref-in-use` cuando la operación deshecha es una `create-worktree` (Q-CKP-58).

**Reglas**
- La ventana es la del panel (5 s desde `Ran`). Con la fila de `created.path` en la instantánea → se envía; sin ella → `PendingUndo` y se envía en cuanto aparezca, si sigue la ventana; vencida → "el worktree aún no aparece; descártalo con D desde su fila". Pasada la ventana, `u` no envía nada.
- Una escritura a la vez (DS-021 H10): `u` está desactivado mientras `Ui.write` no esté en `Idle`.
- Rama protegida (C7): `u` desactivado desde el principio; no se prepara ningún descarte.
- El descarte sigue el flujo de DS-017 T006 entero: sin avisos ni reto → `Run` sin prompt; con ellos → su ConfirmPrompt único con default No.
- Resultado `done` → "Deshecho: borrados el worktree <carpeta> y la rama <rama>" y el panel se cierra. `other-session-present` → "hay una sesión del agente en <carpeta>: ciérrala antes de deshacer". `protected-branch` → el texto de DS-017 ("<rama> es rama protegida (<nivel>)"). Los demás, con los textos de DS-017 T006.
- El descarte queda en el oplog: `raptor undo` sobre su worktree principal lo deshace (DS-017 X9, X10).
- Sin descarte, mientras el worktree creado exista, `raptor undo` en el principal recibe `ref-in-use`; la CLI (`apps/cli/src/commands/undo.rs`, **MODIFY**) lo traduce, solo cuando la operación deshecha es una `create-worktree`, a "esta operación creó un worktree; bórralo desde raptor (D en su fila)" / "this operation created a worktree; remove it from raptor (D on its row)", sin cambiar el protocolo (Q-CKP-58, G6).

- **Depende:** T005, G2 (DS-017 T002 y T003)
- **Refs:** C5, C6; DS-017 X3, X9, X10, T006
- **Aceptación:** `cargo test -p gitraptor-cli --test tui_create_worktree default_path_creates_shows_the_command_and_undo_removes_both` en verde

### T007 — Cerrar la historia

**Objetivo.** La historia queda implementada, verificada y enlazada.

**Ubicación.** `docs/requirements/features/cockpit/user-stories/US-CKP-018-crear-worktree.md` (**MODIFY**) y esta DS (**MODIFY**).

**Reglas**
- La historia pasa a `implemented` y enlaza el PR; esta DS añade `## Estado de la implementación`.
- Las [Enmiendas que implica](#enmiendas-que-implica) se aplican en el mismo PR o quedan anotadas en él.

- **Depende:** T001 a T006
- **Refs:** AGENTS.md (reglas de calidad)
- **Aceptación:** `cargo clippy --workspace` y `node tools/test/nextest-junit.mjs` en verde

---

## Estructura de ficheros

| Archivo | Acción | Tarea |
|---|---|---|
| `crates/core/src/executor/ops/create_worktree.rs` | MODIFY (rama `path = Some`; lo crea DS-MCP-019) | T002 |
| `crates/api/src/worktree_template.rs` | MODIFY (`check_final_path`; lo crea DS-MCP-019) | T002 |
| `crates/core/src/executor/mod.rs` | MODIFY (regla de capa) | T002 |
| `crates/core/tests/us_ckp_018_create.rs` | CREATE | T001 |
| `apps/cli/src/tui/create_worktree.rs` | CREATE | T003, T004, T006 |
| `apps/cli/src/tui/widgets/create_form.rs`, `widgets/mod.rs` | CREATE, MODIFY (1 línea) | T004 |
| `apps/cli/src/tui/{update,view,help,keymap,mod}.rs` | MODIFY | T005 |
| `apps/cli/src/tui/write_flow.rs` | MODIFY (`args` en `Prepare`; lo crea DS-014) | T005 |
| `apps/cli/src/tui/undo.rs` | MODIFY (lo crea DS-014) | T006 |
| `apps/cli/src/commands/undo.rs` | MODIFY (texto de `ref-in-use` sobre una `create-worktree`) | T006 |
| `apps/cli/src/present/create_worktree_text.rs` | CREATE | T005 |
| `apps/cli/src/present/write_text.rs` | MODIFY (5 motivos; lo crea DS-014) | T005 |
| `apps/cli/tests/tui_create_worktree.rs` | CREATE | T001 |

Sin cambios: `crates/api/src/catalog.rs` (los motivos los añade DS-MCP-019 T001; `check_absolute` ya rechaza UNC), `crates/api/src/methods/operation.rs` (la capacidad la añade DS-MCP-019), `crates/core/src/client.rs` (pide todas las capacidades), `crates/core/src/timemachine/` (C5), `crates/policy/` (`PlanShape::CreateWorktree` ya existe) y `apps/mcp/`.

**Puntos calientes compartidos**: `tui/keymap.rs` (DS-012, 014, 015, 017, 019, 022), `present/write_text.rs` (su `match` exhaustivo), `tui/update.rs` y `ops/create_worktree.rs` (con DS-MCP-019). Cada cambio es aditivo; el segundo en entrar rebasa.

## Contratos compartidos

### Tipos y datos compartidos

```rust
// crates/api/src/worktree_template.rs (T002) — a crear; render la usa también
/// Final shape of a new worktree path, from a template or given by the developer: every
/// component 1 to 255 bytes, no Windows device name in any component's stem, on every platform.
pub fn check_final_path(path: &Path) -> Result<(), TemplateError>;

// apps/cli/src/tui/create_worktree.rs (T003, T004, T006) — a crear
pub enum CreateBlock { NotLive, AgentLayer, BaseUnconfirmed, NoWorktree, InProgress }
pub enum CreateEligibility {
    Offered { worktree: PathBuf },
    Disabled { reason: CreateBlock, unblock: Option<Unblock> },
}
pub struct CreateForm { pub branch: String, pub path: String, pub focus: Field, pub note: Option<FormNote> }
pub enum Field { Branch, Path }
pub enum FormNote { InvalidBranch, InvalidPath, PasteRefused, TooLong }
pub struct PendingUndo { pub root: PathBuf, pub until: Instant }

// apps/cli/src/tui/write_flow.rs (T005) — cambio aditivo sobre DS-014 T005
OpRequest::Prepare { operation, worktree, args: serde_json::Map<String, Value>, session_env }

// apps/cli/src/present/create_worktree_text.rs (T005) — a crear
pub enum LaunchLine { Plain(String), Quoted(String), Unsafe }
```

Lo que ya existe y se usa tal cual: `OperationId::{CreateWorktree, DiscardWorktree}`, `CreateWorktreeArgs`, `OperationArgs::parse`, `check_new_branch`, `sanitized_branch`, `RejectReason::{NewPathRefused, NotAvailableForLayer, OperationInProgress, WorktreeLocked}`, `GovernedAs::CreateWorktree`, `PlanShape::CreateWorktree`, `check_new_worktree_path`, `NewPathError`, `UserOp::WorktreeAdd`, `BaseBranchView`, `BaseStatusView`. Lo que crea DS-MCP-019 y se usa tal cual: `RejectReason::{BranchExists, NewPathExists, BaseUnconfirmed, BaseMissing}`, `CAP_OPERATION_CREATE_WORKTREE`, `OperationRunResult.created`, `CreatedWorktree`, `PathTemplate`, `WorktreeSite`.

### Ciclos de vida (DI)

- El sitio del brazo: el de DS-MCP-019 (montado una vez en `serve_channel`, leído fresco en cada llamada).
- `CreateForm` y `PendingUndo`: en `Model.ui`; se borran al cerrar el formulario o el panel, o al cambiar de repo.

### Firmas del stack

```rust
// apps/cli/src/tui/create_worktree.rs — a crear
pub fn create_eligibility(repo: &RepoView, layer: Option<Layer>, live: bool) -> CreateEligibility;
pub fn named_worktree(repo: &RepoView) -> Option<&WorktreeView>;
pub fn on_form_key(form: &mut CreateForm, key: KeyEvent) -> FormStep;   // Editing | Submit | Cancel
pub fn on_form_paste(form: &mut CreateForm, text: &str);
pub fn submit(form: &mut CreateForm, home: Option<&Path>) -> Result<Map<String, Value>, FormNote>;

// apps/cli/src/present/create_worktree_text.rs — a crear
pub fn launch_line(path: &SafeText, intact: bool) -> LaunchLine;
```

## Contrato de API

| Método | Cambio | Con la capacidad | Sin ella |
|---|---|---|---|
| `operation.prepare {operation: "create-worktree", worktree: <raíz del principal>, args: {branch, path?}, session_env}` | `path` servido en producción (C2); `not-available-for-layer` con `path` fuera de la capa `cockpit` (C3) | `PrepareResult` sin avisos ni reto | El de DS-MCP-019 |
| `operation.run {plan_id, accepted_warnings: [], confirmation: null}` | Ninguno sobre DS-MCP-019 | `OperationRunResult` con `created` (`operation.create-worktree`) | Sin `created` |
| `operation.prepare {operation: "discard-worktree", worktree: <created.path>}` | Ninguno (DS-017) | El de DS-017 | El de DS-017 |

### Forma del error y del cuerpo de respuesta

Un rechazo es `-32014 OPERATION_REJECTED` con `data.reason`; una ruta o rama mal formada, `-32602` con el campo. No hay códigos nuevos ni motivos nuevos: `not-available-for-layer` ya existe.

### Forma de la configuración

_No aplica — la plantilla `cockpit.worktreePathTemplate` la define y la lee DS-MCP-019 (T002, D2)._

### Valores numéricos

| Valor | Cifra | Fuente |
|---|---|---|
| Tope de la rama al escribir | 200 bytes | `MAX_BRANCH_BYTES`, `catalog.rs:52` |
| Tope de la ruta al escribir | 1024 bytes | DS-MCP-019 D7 |
| Deshacer en el panel | 5 s | BR-CKP-WF-002 |
| Caducidad del plan | 60 s | `PLAN_TTL_MS`, DS-TS-CKP-002 D8 |

## Modelo de datos

_No aplica — nada nuevo se persiste. La creación queda en el oplog con su subtipo `create-worktree` (DS-MCP-019, Modelo de datos); su Deshacer, como un `discard-worktree` con su previo (DS-017)._

## Estrategia de pruebas y cobertura

### 9.1 Pirámide de pruebas

| Nivel | Dónde | Qué |
|---|---|---|
| Unit | `ops/create_worktree.rs` (ruta explícita), `worktree_template.rs`, `tui/create_worktree.rs`, `widgets/create_form.rs`, `present/create_worktree_text.rs` | Ruta explícita, capa, elegibilidad, formulario, pegado, cita de la orden, idiomas |
| Integración (daemon + Git real) | `crates/core/tests/us_ckp_018_create.rs` | Escenarios del daemon, capa |
| E2E (TUI + daemon) | `apps/cli/tests/tui_create_worktree.rs` (macOS, debug) | Escenarios de la historia y Deshacer |
| Snapshots | `apps/cli/src/tui/snapshots/` | Formulario, panel, acción desactivada, rechazos, en/es |

El contrato de `/nassa-core:implement` declara `"suite": { "command": "node tools/test/nextest-junit.mjs" }` y `"layers": { "runtime": { "commands": [{ "id": "suite", "cmd": "node tools/test/nextest-junit.mjs" }], "report": "target/nextest/ci/junit-paths.xml" } }` (AGENTS.md).

### 9.2 Umbrales de cobertura

Toda regla de la rama `path = Some` de `plan_op`, de `create_eligibility`, de `submit` y de `launch_line` con su test. Sin cifra de cobertura de líneas en el repo.

### 9.3 Datos de prueba

`gitraptor_testkit::Fixture`: repo `shop` bajo un padre temporal `0o755`, `main` confirmada; una rama `feat-login`; un directorio `shop-feat-pagos` ya creado para el escenario 3; un perfil temporal con `cockpit.worktreePathTemplate = "<tmp>/orca/workspaces/shop/<rama>"` para el escenario 4 (el `~/` se prueba en unitarios, con un `HOME` inyectado); un repo con la base sin confirmar para el escenario 5. Nunca este repo ni el perfil real.

### 9.4 Comportamientos críticos verificados

| Escenario de la historia | Test |
|---|---|
| 1. Crear con la ruta por defecto, orden de lanzamiento y Deshacer | `tui_create_worktree::default_path_creates_shows_the_command_and_undo_removes_both`: `refs/heads/feat/pagos` en el oid de `main`, worktree en `<padre>/shop-feat-pagos`, panel con "Para lanzar Claude Code: cd <padre>/shop-feat-pagos && claude", ningún proceso hijo de la TUI; tras `u`, ni el worktree ni la rama existen y el oplog tiene el descarte. `us_ckp_018_create::default_path_is_the_sibling_of_the_main_worktree` |
| 2. Nombre de rama no válido o existente | `tui_create_worktree::an_invalid_branch_never_reaches_the_daemon` (`feat..pagos`, "nombre de rama no válido", ningún `operation.prepare`); `us_ckp_018_create::an_existing_branch_is_branch_exists` (`feat-login`); `tui_create_worktree::an_existing_branch_says_the_story_text` ("la rama ya existe", sin paréntesis); en todos, ni rama ni carpeta nuevas y `last_seq` del oplog igual |
| 3. La ruta ya existe | `us_ckp_018_create::an_existing_default_path_is_new_path_exists` y `an_existing_explicit_path_is_new_path_exists`: `(dev, inode)` y contenido del directorio iguales; `tui_create_worktree::path_exists_reopens_the_form_on_the_path_field` |
| 4. Plantilla de ruta del perfil | `us_ckp_018_create::the_profile_template_places_the_worktree` (`<tmp>/orca/workspaces/shop/feat-pagos`) |
| 5. Sin base confirmada, crear desactivado | `tui::create_worktree::tests::unconfirmed_base_disables_with_raptor_guard_confirm` y `tui_create_worktree::create_is_disabled_until_the_base_is_confirmed` (sin `operation.prepare`) |
| 6. Ruta UNC rechazada | `tui::create_worktree::tests::unc_path_is_refused_locally` (`\\servidor\compartido\shop-x` y `//servidor/compartido/shop-x`); `us_ckp_018_create::unc_path_is_invalid_params` (`-32602`, campo `path`, nada creado) |

Además:
- `us_ckp_018_create::an_agent_layer_cannot_choose_the_path` (C3).
- `us_ckp_018_create::an_explicit_path_gets_the_same_toctou_checks` (enlace en el padre, padre no privado, dentro de un `.git` y de otro worktree).
- `us_ckp_018_create::a_windows_device_name_is_refused_everywhere` (`.../CON`, `.../nul.txt`).
- `us_ckp_018_create::with_test_arms_an_explicit_path_still_goes_to_the_test_arm` (C2, regla 1).
- `tui::create_worktree::tests::paste_with_control_characters_is_ignored`.
- `present::create_worktree_text::tests::launch_line_quotes_or_refuses` (espacio, `'`, `;`, `$(`, ruta neutralizada).
- `tui_create_worktree::undo_waits_for_the_row_then_discards` (C6).
- `tui_create_worktree::undo_with_a_session_present_is_refused_and_keeps_everything` (C5).
- `tui_create_worktree::undo_after_the_window_sends_nothing` (pasados 5 s, `u` no envía nada y el panel dice "para borrarlo: D en su fila").
- `tui_create_worktree::ctrl_c_quits_with_the_form_open` (y `q` se escribe en el campo).
- `tui_create_worktree::a_protected_new_branch_has_undo_disabled_from_the_start` (C7).
- `tui::create_worktree::tests::undo_is_disabled_while_another_write_runs` (DS-021 H10).
- `us_ckp_018_create::undo_in_main_after_create_reports_why` (`raptor undo` en el principal → `ref-in-use` y el texto de la CLI; G6).
- `us_ckp_018_create::a_pending_base_is_base_unconfirmed` (C14; `#[ignore]` hasta que entre G5).

### 9.5 Plataformas

Se ejecuta en macOS. Linux y Windows: **Pendiente: etapa de validación multiplataforma** ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)), con G1 de DS-MCP-019 (ACL del padre), XP-57 (UNC y `\\?\` en Windows real) y XP-58 (cita de la orden para `cmd` y PowerShell). En Windows, mientras XP-58 siga abierto, el panel muestra la ruta y "Para lanzar Claude Code: abre una terminal en esa carpeta y ejecuta claude", sin orden compuesta. El Deshacer es un descarte `Affected::Nobody`, que Windows sí ofrece (DS-017 9.5).

## Gate de seguridad

- [ ] **H-02**: con `path`, solo la capa `cockpit` (C3), además del esquema MCP sin `path`. Test `an_agent_layer_cannot_choose_the_path`.
- [ ] **NFR-02, sin shell**: solo `run_git` con `UserOp::WorktreeAdd` (DS-MCP-019); la TUI no lanza ningún proceso. La orden de lanzamiento se muestra, nunca se ejecuta.
- [ ] **Inyección por copiar y pegar**: `launch_line` cita la ruta o no muestra orden (C8).
- [ ] **TOCTOU de la ruta explícita**: las mismas tres comprobaciones, `create_dir` y verificación posterior que la ruta de plantilla (DS-MCP-019 D6); un fallo nunca borra contenido.
- [ ] **NFR-01**: la creación tiene previo garantizado (DS-MCP-019 G6); Deshacer es un descarte con su propio previo y su propio Deshacer (C5).
- [ ] **Texto no confiable**: entrada del formulario solo imprimible; rutas y ramas del daemon por `SafeText` (BR-CKP-VAL-002).
- [ ] Toca la elección de ruta del sistema de archivos: **revisión de `security-expert`** junto a la de DS-MCP-019, con el foco en C2, C3 y C8.

## Cómo extender

- **Otro agente en la orden de lanzamiento** (Codex, Cursor, tras el MVP): una variante en `present/create_worktree_text.rs` elegida por el perfil; `launch_line` no cambia la cita.
- **Otro campo del formulario**: un `Field` y su regla en `submit`, con su validación en `OperationArgs::parse` (nunca solo en la TUI).
- El resto, como en [Cómo extender de DS-014](./US-CKP-014-integrar-rama-agente.md#cómo-extender).

## Enmiendas que implica

No aplicadas por esta DS; las aplica el PR de la historia o quedan anotadas en él.

- **DS-US-MCP-019 T004 paso 2.1**: con `path = Some` y sin brazos de prueba, el brazo sirve la ruta explícita (C2) en lugar de `NotImplemented`; la prueba `an_explicit_path_is_us_ckp_018s` pasa a `with_test_arms_an_explicit_path_still_goes_to_the_test_arm` y a los tests de T002. En § Fuera de alcance, "Deshacer la creación" lo cubre US-CKP-018 con un descarte (C5).
- **DS-US-MCP-019 T001**: `worktree_template.rs` expone `check_final_path`, que usa `render` (C2).
- **DS-US-CKP-014 T005**: `OpRequest::Prepare` gana `args` (C15).
- **DS-US-CKP-014 I12**: el Deshacer de una creación no es `timemachine.undo`, sino un descarte (C5).
- **ADR-CKP-002 § 1 ("Ruta nueva")**: la ruta explícita solo la acepta la capa `cockpit`; el ejecutor lo comprueba además del esquema MCP (C3).
- **BR-CKP-ELIG-005**: "Deshacer borra el worktree y la rama creados" se implementa como el descarte de BR-CKP-ELIG-004 sobre el worktree creado; si hay sesión presente, no se deshace (C5).
- **DS-US-CKP-021 (historial)**: una entrada `protected` con subtipo `create-worktree` sale con `u` desactivado y "se deshace descartando el worktree: D en su fila" / "undo it by discarding the worktree: D on its row"; test `a_create_worktree_entry_is_not_undone_from_the_history` (Q-CKP-58).
- **DS-US-MCP-019 D3 y DS-US-GRP-016 (condicional, C14)**: con `BaseBranchView.pending` presente (capacidad `engine.base-pending`), el sitio de `create-worktree` responde `base-unconfirmed` para todas las capas. Se aplica cuando las dos estén en `main` (G5).

## Fuera de alcance

| Qué | Dueño |
|---|---|
| La herramienta MCP `create_worktree` | US-MCP-019 |
| Lanzar o registrar el agente en el worktree nuevo | Nunca (BR-CKP-ELIG-005) |
| Copiar la orden al portapapeles | Fuera del MVP |
| Elegir la base de la rama nueva | Siempre la base confirmada (BR-CKP-ELIG-005) |
| Deshacer una creación con `raptor undo` o desde el historial | Q-CKP-58, US-CKP-021 |
| Rama base del equipo | US-GRD-014 (implementada; la lee el sitio de DS-MCP-019) |

## Notas del autor

- Riesgo: si el desarrollador lanza Claude Code en el worktree nuevo antes de los 5 s, Deshacer se rechaza por sesión presente; es lo correcto (DS-017 X15) y el texto lo explica.
- Una rama nueva que casa con una regla de rama protegida se crea (Guardrails decide `Crear worktree`), pero no se deshace desde el panel: `u` sale desactivado y remite a `D` cuando la regla lo permita (C7, Q-CKP-57).
- `new-path-refused` no dice cuál de sus condiciones falló: el contrato no lo lleva y añadirlo sería otra capacidad. El texto enumera las condiciones.

### Preguntas abiertas

| ID | Pregunta | Para |
|----|----------|------|
| ~~Q-CKP-57~~ | ~~Deshacer de una creación = descarte del worktree creado (C5). ¿Se acepta que una rama protegida creada no se deshaga desde el panel?~~ **Cerrada.** Decisión del orquestador (2026-10-28), validada por Arquitecto y PO: aceptado en el MVP, sin saltarse `protected-branch`; `u` sale desactivado desde el principio (C7) | Arquitecto / PO |
| ~~Q-CKP-58~~ | ~~`raptor undo` y el historial (US-CKP-021) ante una `create-worktree`: ¿se mapean al descarte?~~ **Cerrada.** Decisión del orquestador (2026-10-28), versión del Arquitecto: el Deshacer de la creación vale solo en el panel (5 s) y lanza `discard-worktree`. En el historial (DS-021), la entrada sale con `u` desactivado y "se deshace descartando el worktree: D en su fila". `raptor undo` sigue rechazando; la CLI traduce `ref-in-use` a "esta operación creó un worktree; bórralo desde raptor (D en su fila)", sin cambiar el protocolo | Arquitecto / PO |

## Estado

- Bloqueada por G1 (DS-MCP-019 T001 a T005) y G2 (DS-014 T005 y T007; DS-017 T002 y T003 para T006). G5 y G6 bloquean la liberación o son informativos, no la construcción.
- Validada por Arquitecto y PO (2026-10-28); nada pendiente de validar.
