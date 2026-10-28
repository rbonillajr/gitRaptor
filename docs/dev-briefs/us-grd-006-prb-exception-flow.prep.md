# Preparación de la corrida · US-GRD-006 PR-B (2026-10-28)

Sin diseño: [DS-US-GRD-006](../requirements/features/guardrails/dev-specs/US-GRD-006-excepcion-consciente.md) es el plan. Alcance: **solo PR-B** (D22) = T005–T011: daemon (`daemon/guard_exception.rs`), canal (`conn.rs`: tres métodos, `apply`, forma por capacidad, registro), entradas `exception*`, plantilla 4 y cliente del hook, `raptor guard exec`, estado y registro en la CLI, mensajes en/es, suites e2e y la documentación de T011 (enmiendas de § 9 a ADR-GRD-007, ADR-GRD-001 y ADR-GRD-006; comentario de `PatternPolicy`; estado de US-GRD-006 y US-GRD-017). Incluye lo diferido de PR-A (S1, S3, S4 de su [nota](us-grd-006-pra-exception-core.prep.md)) y las notas de su revisión (§ 10 de la spec). Contrato: [us-grd-006-prb-exception-flow.contract.json](us-grd-006-prb-exception-flow.contract.json).

## Deriva de citas

Todas las citas de la spec siguen vigentes; cambian las líneas. Lo que afecta al build:

| Cita de la spec | Hoy | Efecto |
|---|---|---|
| `conn.rs:888` `GUARD_EVALUATE`; `guard_served` antes de `hook_prior` | `conn.rs:987`; `guard_served` :994, `hook_prior` :1014 | `apply` entre :994 y :1014 |
| `conn.rs:1229` salida temprana de `log_decision` | `conn.rs:1363` | Deja de cubrir `exception = applied` |
| `conn.rs:2526` `reserved`; `:2564` anuncio | `conn.rs:2740`; `:2815` | Ninguno |
| `conn.rs:972` brazo `_` | `conn.rs:1053`; audita como reservado también `guard.exception-end` | Los tres métodos ganan brazo propio (nota de revisión de PR-A) |
| `authz.rs:279` `check_reserved` | vigente | Ninguno |
| `install.rs:975` criterio `outdated` | `fn outdated` privada en `install.rs:988` | S-B5 |
| `log.rs` clave de agregación con `action_id` (D16) | `LogEntry` (`log.rs:33-55`) sin `action_id`; `agg_key` :74 | S-B2 |
| `raptor-hook.rs:325` `chain` quita el token | hoy no hay ningún `env_remove` | Se añade |
| `guard.rs:910` (`--yes` de uninstall) | `apps/cli/src/guard.rs:972` | Ninguno |
| `GROUP.errors` | `methods/guard.rs:166`, sin `GUARD_EXCEPTION_REFUSED` (declarado en :120) | S3 de PR-A, entra ahora |
| `schema.rs:236-237` | `:276-277` | Ninguno |
| `guardrails::exception::facts` (§ 6.1) | no existe ningún lector de `RepoFacts` | S-B6 |
| `pending::Entry::view()` y `pending::Refused` | `view()` fija `exception: None`; `Refused` usa `UninstallRefusal` | Se generalizan para `PendingKind::Exception` y `ExceptionRefusal` |
| `HOOK_PRIOR_DEADLINE` de pruebas | `GITRAPTOR_TEST_TM_HOOK_PRIOR_DEADLINE_MS` | Ninguno |
| `policy.diff-size`, `policy.commit-format` (D5) | no existen en `Rule` (DS-US-GRD-009 sin merge) | S-B7 |
| `settings/model.rs:168` comentario de `PatternPolicy` | sigue diciendo «sin excepción en el MVP» | Se corrige (Q-GRD-50) |
| `context.md`, `business-rules.md` (Q-GRD-47, Q-GRD-50) | ya registradas, con la nota de Q-GRD-35 | T011 solo comprueba |
| Enmiendas de § 9 a ADR-GRD-007/001/006 | sin aplicar (`ADR-GRD-007:78` sigue con `reserved-action-pending`) | T011 las aplica (G2) |

Ninguna deriva contradice la spec: **sin BLOQUEO**.

## Supuestos

Decisiones del orquestador (2026-10-28), validadas por Arquitecto (`architecture-advisor`, opus). La revisión del Arquitecto ajustó S-B1, S-B4 y S-B6.

- **S-B1 · Auditoría `used`** (D11 d). Se añade `AuditOutcome::Used`. Está en `needs_capability` y además se filtra por `CAP_GUARD_EXCEPTION`, con el mismo mecanismo que `Changed` (`bus.rs:259` `without_changed_outcome`, `conn.rs:659/668`, `audit.list` `conn.rs:2595-2600`). Un cliente con `audit.outcomes` pero sin `guard.exception` no recibe `used`, porque el enum no tiene `#[serde(other)]`. La emisión del token audita `applied` y el uso audita `used`, con la cadena completa desde el hilo de la conexión. Si falla la escritura de `used`, se deniega.
- **S-B2 · `action_id` en la clave** (D16). `LogEntry.action_id: Option<String>`. `agg_key` construye otra tupla solo cuando hay `action_id`, así que las claves actuales salen idénticas byte a byte. No hay columna nueva. Las entradas `exception*` no las descarta el tope en vuelo: su número lo acota el «un pendiente por repo».
- **S-B3 · Reloj del TTL** (nota de PR-A). `Tokens` mantiene `now_ms`. El daemon le pasa milisegundos desde un único `Instant`, el del arranque del proceso, en `issue`, `present` y `sweep`. `pending::applies_at_ms` sigue en reloj de pared, así que los dos relojes no se comparan nunca (queda documentado en el código). Las variables de prueba son `GITRAPTOR_TEST_GUARD_EXCEPTION_TTL_MS` y `GITRAPTOR_TEST_GUARD_EXCEPTION_CHILD_WAIT_MS`, y solo se leen con `cfg!(debug_assertions)`, como `pending::window()`.
- **S-B4 · Ctrl-C** (D8). `exec` registra SIGINT en un `AtomicBool`, solo en Unix, con un helper nuevo de `gitraptor-core` (`pub fn interrupt_flag() -> io::Result<Arc<AtomicBool>>`, en un archivo propio). Por dentro usa `signal_hook::flag::register`, que es seguro: el workspace prohíbe `unsafe`. `signal-hook` ya es dependencia Unix de `crates/core` (`Cargo.toml:45`, `shutdown.rs:720`). Corrige PC-1: `apps/cli` no depende de `libc`. Durante la ventana, si lo ve, llama a `guard.cancel` y sale con 130. Tras lanzar `git` el manejador sigue registrado y se ignora: `git` recibe el SIGINT por el grupo de procesos y `exec` devuelve su código de salida. No se restaura el manejo por defecto. Sin dependencias nuevas y sin cambios en `Cargo.lock`. En Windows, Ctrl-C mata `exec` y la acción caduca (`expired`): **pendiente de la etapa multiplataforma**.
- **S-B5 · `outdated-install`** (D12). `install::outdated` pasa a `pub(crate)` y la llamada (1) la consulta **después** de autorizar (D3). `exec` comprueba antes `Diagnostic::TemplateOutdated` de `guard.status`, solo por UX: la autoridad es el daemon.
- **S-B6 · Hechos del repo** (D3). Archivo nuevo `crates/core/src/guardrails/exception_facts.rs`. La configuración efectiva (`push.default`, `remote.pushDefault`, `branch.<x>.remote/merge/pushRemote`, drivers de `textconv`) se lee con el envoltorio de Git CLI de ADR-GRP-009 (ruta resuelta, entorno limpio, `GIT_OPTIONAL_LOCKS=0`), no con un `Command` nuevo. HEAD, las copias de seguimiento y los oids se leen con `gix`. Todo se lee en el directorio efectivo (cwd del solicitante más los `-C`), en el hilo de la conexión y solo después de autorizar. Cierra el Medium de PR-A: un push triangular con `simple` se acepta cuando Git lo acepta, y lo cubre GRD006-TOKEN-ARGV.
- **S-B7 · Fuera del contrato** (Arquitecto, P7). `the_exception_skips_everyone_commit_policies_in_a_rebase` necesita las reglas de DS-US-GRD-009, que aún no existen. Queda pendiente para cuando 009 se mergee y se anota en § 10 de la spec y en el PR. `exceptable` ya obliga a clasificarlas: su `match` no tiene comodín.
- **S-B8 · Archivos de prueba.** Los tests de integración de PR-B van en `crates/core/tests/us_grd_006_channel.rs` y los del contrato en `crates/api/tests/us_grd_006_prb_contract.rs`, ambos archivos nuevos. Así no se tocan los archivos de PR-A, cuyas pruebas siguen congeladas. La spec nombra `crates/core/tests/us_grd_006.rs`.
- **S-B9 · Arnés de actor.** Las e2e usan `apps/cli/tests/guard_machine/mod.rs` (`Machine::with_window`) con `GITRAPTOR_AGENT_EXECUTABLES=raptor-fake-agent`: el daemon hijo solo reconoce el agente falso, así que el Claude Code real que lanza la suite no cuenta. El agente registrado del escenario 1 sigue el patrón de `agent_registration.rs` (`FAKE_CODEX`). Las suites llevan `#![cfg(unix)]` y se niegan a correr sin *debug assertions*.
- **S-B10 · Notas de la revisión de PR-A que entran.** El `Debug` de `GuardExceptionParams` redacta el argv. Un `exception_token` mal formado no tumba `guard.evaluate`: la decisión es la de siempre. `guard.exception-end` deja de auditarse como reservado. El entorno del hijo queda sin `GIT_*` salvo la lista de D10, y sin `EDITOR`, `VISUAL` ni `PAGER`. Los `-C` (`Request.dirs`) se validan: existen y quedan dentro del worktree registrado. Un destino corto de push y una rama de rebase se normalizan con `full_ref` o se rechazan. `ExceptionToken: PartialEq` se queda, porque el almacén compara con `ct_eq`.
- **S-B12 · Fallo de la auditoría `used`** (PC-2). Para probar que sin `used` no hay excepción, el daemon lee una inyección de fallo solo en depuración: `GITRAPTOR_TEST_GUARD_FAIL_USED_AUDIT=1`, con `cfg!(debug_assertions)`, igual que S-B3. Con ella, la escritura de `used` devuelve error y la presentación se deniega con `exception = rejected` y sin entrada `exception`. Lo cubre GRD006-USED-AUDIT-FAILS.
- **S-B11 · Plataformas.** La identidad del hijo (padre directo por pid y hora de inicio) reutiliza el patrón de `guardrails/link.rs:53-69`. macOS se verifica aquí y Linux en el CI de ubuntu. Windows (handle y hora de creación, Validación 12) queda **pendiente de la etapa multiplataforma**.

## Convenciones observadas

Solo lo que la spec no cubre:

- Las e2e se apoyan en `apps/cli/tests/guard_machine/mod.rs` (`Machine`, `Running::wait_for`, `fake_agent_entry`). Las esperas sin `sleep` siguen el patrón de `guard_us_grd_003.rs`, y el stream de eventos el de `claude_sessions.rs:525-570`.
- La conexión MCP se abre con `Client::connect(.., ClientKind::Mcp, ..)` (`mcp_allowlist.rs:500-510`) y el catálogo se lee como en `apps/mcp/tests/handshake.rs`.
- El daemon en hilo para core se monta como en `crates/core/tests/guard_evaluate.rs:50-90` y `us_grd_017.rs:29-60`, con `AgentMatcher::only(["raptor-fake-agent"])`.
- La plantilla 3 para `outdated-install` se fabrica al estilo de `guard_us_grd_018.rs:444`. `downgrade_to` solo admite las plantillas 1 y 2.
- Los textos se recogen solos: `apps/cli/build.rs` embebe `i18n/<lang>/*.txt`, y `codes.rs::every_contract_code_has_both_messages` lee `GROUP.errors`.
