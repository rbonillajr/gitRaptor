---
id: DS-US-TMC-001
title: "Dev Spec — Snapshot previo antes de toda operación lanzada por GitRaptor"
type: dev-spec
status: implemented
feature: time-machine
domain: GRP
story: US-TMC-001
created: 2026-10-05
updated: 2026-10-08
related:
  stories: [US-TMC-001, US-GRP-001]
  enablers: [TS-TMC-001, TS-TMC-002, TS-TMC-004, TS-GRD-001, INF-GRP-001]
  adrs: [ADR-TMC-001, ADR-TMC-002, ADR-TMC-003, ADR-TMC-004, ADR-TMC-007, ADR-GRP-007, ADR-CKP-002]
  rules: [BR-TMC-CONS-001, BR-TMC-CONS-002]
  nfrs: [NFR-01, NFR-10, SEC-TMC-06]
tags: [time-machine, snapshot-previo, operacion-protegida, credenciales, perfil, catalogo, i18n, nfr-01]
---

# Dev Spec — US-TMC-001: snapshot previo antes de toda operación de GitRaptor

Plano compacto de [US-TMC-001](../user-stories/US-TMC-001-snapshot-previo-operaciones-gitraptor.md). Une lo que ya está en `main`: el almacén y la captura (TS-TMC-001), el oplog (TS-TMC-002), la operación protegida con su solicitante (TS-TMC-004), el cargador de configuración (TS-GRD-001) y el repo observado (US-GRP-001).

**Qué entrega**: el daemon resuelve de verdad el repo y el worktree de una operación, toma el snapshot previo garantizado en el **almacén real** del repo, con todos los worktrees del ámbito y el estado de refs, y aplica la opción del perfil sobre credenciales. Sin snapshot previo, la operación no se ejecuta y el solicitante recibe el motivo tipado, traducido en/es en la CLI.

**Qué no entrega**: el catálogo de operaciones ni el ejecutor (TS-CKP-002, en paralelo). Esta historia deja el punto de enganche (`OperationCatalog`) y lo prueba con un catálogo de test. Mientras no haya catálogo cableado, el daemon de producción responde `NOT_IMPLEMENTED` (F-001-02) a `operation.run`, igual que hoy.

## 1. Ubicación en el código

| Archivo | Responsabilidad |
|---|---|
| `crates/core/src/timemachine/protected/backend.rs` (nuevo) | `TmRepos` (registro por repo observado: oplog compartido y almacén abierto una vez), `OperationCatalog` (enganche del catálogo), `OperationsWiring` y `DaemonBackend`, el `ProtectedBackend` de producción |
| `crates/core/src/timemachine/protected/mod.rs` | `ProtectedStep::scope()` (ámbito declarado, obligatorio); `ProtectedRequest::for_step` (unión y validación del ámbito); claves estables de worktree; `StoreSnapshotter` lee la opción de credenciales del perfil en cada snapshot previo |
| `crates/core/src/timemachine/store/capture.rs` | `CaptureRequest.include_credentials`; un cambio de la opción fuerza una detección completa |
| `crates/core/src/profile/settings.rs` (nuevo en esta rama) | Lectura del `settings.json` del perfil con el parser de TS-GRD-001 (nivel perfil) |
| `crates/core/src/channel/conn.rs` | `operation.run` construye la petición con `ProtectedRequest::for_step`; un ámbito fuera del repo responde `SCOPE_REFUSED` |
| `crates/core/src/daemon/mod.rs` | El daemon guarda los oplogs en `TmRepos`, registra los repos que se añaden en caliente y cablea `DaemonBackend` cuando hay catálogo |
| `apps/cli/src/main.rs`, `apps/cli/i18n/{en,es}.txt` | Motivo de `PRIOR_SNAPSHOT_FAILED` traducido |

## 2. Diseño

### 2.1 Repo y worktree de la operación

`DaemonBackend::repo_of(carpeta)` abre la carpeta con `RepoReader` (solo lectura, sin buscar hacia arriba, igual que `repo.add`). La carpeta debe ser la **raíz** de un worktree. Su directorio común canónico debe ser el de un repo **observado** en `TmRepos`; si no, `NOT_OBSERVED`. El `RepoHandle` lleva el worktree canónico, el oplog compartido del repo y un `StoreSnapshotter` sobre el almacén del repo.

`TmRepos` (`Arc` con un `Mutex` interno) sustituye a la lista de oplogs del daemon. Al arrancar recibe los oplogs recuperados (TS-TMC-002). `repo.add` abre y recupera el oplog de un repo nuevo o reactivado, y `repo.retire` lo quita. El almacén se abre (`open_or_create`) la primera vez que se necesita y queda en caché: una sola instancia por repo, porque el estado incremental de la captura vive en ella. Si no se puede abrir, el previo falla como `store-unavailable` y la operación no corre. Una operación en curso conserva su `RepoHandle` (oplog y almacén por `Arc`) hasta terminar, aunque su repo se retire o el daemon empiece a pararse. Al parar, el daemon vacía `TmRepos` después de cerrar el canal.

**Carpeta de la operación**: debe ser la raíz de un worktree; una subcarpeta responde `SCOPE_REFUSED`. Por MCP, el cwd del agente puede ser una subcarpeta. Resolver su raíz queda para DEP-MCP-4 / F-001-05; hoy no se nota, porque la allowlist MCP de producción no admite ningún repo.

### 2.2 Ámbito (ADR-TMC-001 § 1-2, ADR-TMC-002 § 5)

- `ProtectedStep::scope() -> StepScope { worktrees, refs }` es **obligatorio, sin valor por defecto**: toda operación dice qué toca (NFR-01). Si el ejecutor de TS-CKP-002 olvida declararlo, no compila. Ahí declara el ámbito de cada operación del catálogo (un merge con la base sacada en otro worktree, por ejemplo).
- `ProtectedRequest::for_step(repo, paso, …)` es el constructor común de la petición, para que la unión no se pierda si `operation.run` se divide en preparar/ejecutar (ADR-CKP-002). Captura el worktree nombrado más los que declara la operación, sin repetir. Anota en el oplog esos worktrees y esas refs. Si un worktree declarado no está registrado en el mismo repo, rechaza con `ScopeError::ForeignWorktree` (`SCOPE_REFUSED`) **antes** de anotar la intención.
- **Claves estables**: cada worktree va bajo `wt/main` o `wt/wt-<nombre en .git/worktrees>`, y no bajo su posición. Así conserva su estado incremental y el *fast path* de una operación a otra (ADR-TMC-006 § 5). Si el nombre no es una clave válida, se usa la posición.
- Las ramas, el stash y la lista de worktrees registrados del repo entran **siempre** en el snapshot (`meta.branches`, `meta.registered`). Por eso descartar un worktree y su rama queda cubierto con el worktree en el ámbito.

### 2.3 Credenciales (BR-TMC-CONS-002, SEC-TMC-06, ADR-GRP-007)

- `timeMachine.includeCredentialFiles` es una clave **solo de perfil** que vale `false` por defecto. Se lee del `settings.json` de la carpeta de configuración del perfil con `parse_document(…, Level::Profile, SourceKind::Profile)`. La lectura no sigue enlaces, solo acepta un archivo regular y corta a 64 KiB (L-03).
- Se lee **en cada snapshot previo**, sin caché. Si falta el archivo, no es válido o no se puede leer, vale `false` (fail-safe: se excluyen). El diagnóstico de un `settings.json` del perfil inválido lo muestra US-GRP-013, que es dueña de presentar los diagnósticos de configuración del perfil.
- Con `false`, la lista cerrada se excluye y se declara (`exclusions` del meta y del oplog con motivo `credential`), como ya hace la captura. Con `true`, se capturan y no se declaran.
- Un cambio de la opción entre dos capturas del mismo worktree fuerza una detección completa (`full:credential-option-changed`), para que la caché incremental no arrastre la decisión anterior.

### 2.4 Fallo del snapshot previo (ADR-TMC-003 § 3, ADR-TMC-004 § 1)

No cambia el pipeline de TS-TMC-004: un fallo deja la operación `aborted` con su motivo, el paso no corre y el cliente recibe `PRIOR_SNAPSHOT_FAILED` con `reason` (`no-space`, `store-unavailable`, `timeout`, `daemon-stopping`, `capture-failed`) y `operation_id`. Esta historia añade:

- **CLI**: el error se traduce por su `reason` con claves `prior.*` en en/es, con la infraestructura i18n de US-GRP-001. Lo usa el manejador común de errores de la CLI, así que lo hereda cualquier comando que lance operaciones.
- **Inyección de fallos en tests**: `OperationsWiring.prior_layer` envuelve al snapshotter de producción. Así un test devuelve `ENOSPC` real desde la conversión `CaptureError → PriorError` sin llenar un disco. Solo se aplica en builds con `debug_assertions`; un build de release la ignora (mismo patrón que `GITRAPTOR_PROFILE_DIR`).

### 2.5 Cableado

`DaemonConfig.operations: Option<OperationsWiring { catalog, prior_deadline, prior_layer }>`. Si `protected` (el doble completo de TS-TMC-004) es `Some`, ese gana. Si no y hay `operations`, el daemon cablea `DaemonBackend`. Si no hay ninguno, `operation.run` responde `NOT_IMPLEMENTED`. `for_current_user` deja `operations: None` hasta que TS-CKP-002 aporte el catálogo. La allowlist MCP de producción sigue siendo `NoMcpRepos` (deny-all, DEP-MCP-4).

## 3. Decisiones

Todas son **Decisión del orquestador (2026-10-05), validada por el Arquitecto** (ver § 6):

| # | Decisión | Motivo |
|---|---|---|
| D1 | El catálogo es un trait (`OperationCatalog`) y no se implementa aquí | El brief lo prohíbe: lo construye TS-CKP-002 en paralelo. El enganche es la operación protegida |
| D2 | El ámbito lo declara la operación (`ProtectedStep::scope`), es obligatorio y se valida contra los worktrees del repo | ADR-TMC-002 § 5 / ADR-TMC-007 § 2: cada operación declara su ámbito. Que sea obligatorio evita que un olvido deje un worktree fuera del previo sin aviso. Se valida porque un worktree declarado puede salir de argumentos del cliente |
| D3 | Lectura mínima del `settings.json` del perfil solo para esta clave | US-GRP-013 ampliará el cargador al nivel local. La clave es solo de perfil (ADR-GRP-007), así que no hace falta combinar niveles |
| D4 | `TmRepos` con una instancia de almacén por repo y oplogs compartidos `Arc<Mutex<_>>` | La operación protegida corre en el hilo de la conexión. El estado incremental de la captura y el oplog no se pueden duplicar |
| D5 | Sin espacio se prueba con `prior_layer` (fallo inyectado en el borde del snapshotter, solo en builds de debug) y no con un disco lleno | Un volumen lleno no es portable en CI. El resto de la cadena es real: daemon, canal, oplog y huella del repo |
| D7 | Claves estables de worktree en el snapshot (`main`, `wt-<nombre>`) | Con claves por posición, el mismo worktree cambiaba de clave entre operaciones y perdía el estado incremental y el *fast path* |
| D8 | Los repos se registran en el perfil antes de arrancar el daemon en los tests (no con `repo.add`) | `repo.add` es un comando reservado y un cliente del mismo proceso desciende del daemon, así que se rechaza. El alta en caliente (`add_repo` → `TmRepos`) queda cubierta por el mismo código de recuperación del arranque |
| D6 | Tests con daemon real **en proceso** y cliente real por el socket | Igual que `channel_protected.rs`. El solicitante por proceso ya lo cubre `apps/cli/tests/protected_process.rs` (TS-TMC-004) |

## 4. Plan de tests (`crates/core/tests/us_tmc_001.rs`)

Fixture del testkit (repo, worktrees y `HOME` temporales), perfil temporal aparte y daemon real con Git del sistema. El repo se observa desde el perfil (D8) y la operación entra por `operation.run`. El catálogo de test tiene `discard-changes` (`git checkout -- .` + `git clean -fd`), `discard-worktree` (`git worktree remove --force` + `git branch -D`) e `inspect` (`git status`, sin efectos), lanzados con `StepCtx::spawn`.

| Escenario Gherkin | Test |
|---|---|
| Una operación guarda antes el trabajo sin commitear | `a_gitraptor_operation_saves_uncommitted_work_first`: el previo tiene `lib.rs` modificado y `nuevo.rs`; después de la operación el worktree queda limpio; en el oplog, la operación está `finished` con ese previo |
| Descartar un worktree y su rama queda cubierto | `discarding_a_worktree_and_its_branch_is_covered`: se lanza **desde el worktree principal** y `feat-pagos` entra solo por el ámbito declarado. El meta del previo tiene `feat-pagos` en `branches` con su commit, el worktree en `registered` y su trabajo en `wt/`; después ya no existen. En el oplog, el ámbito tiene los dos worktrees y la ref |
| Los ignorados no entran | `ignored_files_are_not_in_the_prior`: ni `.env` ni `node_modules/` en el previo, sin declarar, e intactos (huella) |
| Credenciales excluidas y declaradas | `untracked_credentials_are_excluded_and_declared`: `nuevo.rs` dentro; `deploy.pem` y `.env.local` fuera y declarados `credential`; intactos |
| El perfil incluye credenciales | `the_profile_can_include_credentials`: `settings.json` con `includeCredentialFiles: true`; `deploy.pem` dentro y sin declarar |
| Sin punto previo, no hay operación | `no_prior_snapshot_means_no_operation`: `ENOSPC` inyectado; `PRIOR_SNAPSHOT_FAILED` con `no-space`; el paso no corre; la huella del testkit no cambia (worktree, rama y trabajo intactos); oplog `aborted` |

Además: `a_scope_outside_the_repo_is_refused` (worktree de otro repo declarado, carpeta no observada y subcarpeta → `SCOPE_REFUSED`, sin nada en el oplog) y `without_a_catalog_operations_are_not_implemented`. Unitarios: lectura del perfil (ausente, inválido, enlace, demasiado grande → `false`); `changing_the_credentials_option_forces_a_full_detection` en `tm_store_capture.rs`; textos `prior.*` en/es (`every_prior_failure_has_a_message` y `catalogs_match`).

Los escenarios de credenciales e ignorados usan `inspect`, que no modifica nada. Con `discard-changes`, `git clean -fd` borraría `deploy.pem`, que no está en el previo y no se podría recuperar. Esto queda anotado para TS-CKP-002: descartar debe avisar de lo no recuperable (ADR-CKP-002 § 8).

El requisito "sin vías de escritura fuera de la operación protegida" lo verifica el test de contrato `api::methods::only_protected_paths_write` (TS-TMC-004), que esta historia no cambia.

## 5. Pendientes y fuera de alcance

- Catálogo, ejecutor, cerrojo con cola y regla de permisos: TS-CKP-002 / BR-07 / F-001-05. Lo que falta para producción es cablear `operations` en `for_current_user` con el catálogo.
- Verificación con un Claude Code real y `raptor undo`: US-TMC-002.
- Presupuesto y repo de referencia del previo: SPIKE-TMC-001 / ADR-TMC-006 (ya medido; no cambia aquí).
- Linux y Windows: los tests del canal son solo de macOS, igual que sus hermanos. **Pendiente: etapa de validación multiplataforma.** En Windows todo compila con `cfg` (sin canal).

## 6. Validación del Arquitecto (2026-10-05)

Veredicto: **aprobada con ajustes**, sin bloqueantes en D1–D6. Todos los ajustes están incorporados:

1. `scope()` obligatorio, con los worktrees declarados validados contra el repo (§ 2.2, D2).
2. La unión del ámbito, en un constructor común (`ProtectedRequest::for_step`) y no en `operation.run` (§ 2.2).
3. Concurrencia y vida de `TmRepos` especificadas: operación en curso al retirar o parar (§ 2.1).
4. Claves estables de worktree (D7).
5. La raíz del worktree por MCP, documentada como pendiente (§ 2.1).
6. `prior_layer` solo en builds de debug (§ 2.4, D5).
7. Test lanzado desde el worktree principal con el ámbito declarado. Escenarios de credenciales con `inspect`. Cita del test de contrato (§ 4).

## Estado de la implementación (2026-10-08)

Implementado en: PR #61.

- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
