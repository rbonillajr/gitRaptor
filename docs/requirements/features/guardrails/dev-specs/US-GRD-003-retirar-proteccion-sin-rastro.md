---
id: DS-US-GRD-003
title: "Dev Spec — US-GRD-003: retirar la protección deja el repo exactamente como estaba"
type: dev-spec
status: partially-implemented
feature: guardrails
domain: GRP
story: US-GRD-003
created: 2026-10-08
updated: 2026-10-08
related:
  stories: [US-GRD-003, US-GRD-001, US-GRD-002, US-GRD-004, US-GRD-005, INF-GRD-001, SPIKE-GRD-001]
  adrs: [ADR-GRD-001, ADR-GRD-005, ADR-GRD-006, ADR-GRD-007, ADR-GRP-005, ADR-GRP-016]
  rules: [BR-CONS-005, BR-WF-002, BR-AUTH-001]
  nfrs: [NFR-01, NFR-12, NFR-GRD-01]
tags: [guardrails, hooks-git, desinstalacion, ventana-cancelable, d5, nfr-01, nfr-12, cortes]
---

# Dev Spec — US-GRD-003: retirar la protección deja el repo exactamente como estaba

Plano compacto (AADD ligero) de [US-GRD-003](../user-stories/US-GRD-003-retirar-proteccion-sin-rastro.md), construido junto a [US-GRD-002](./US-GRD-002-hooks-previos-respetados.md). El contrato lo fijan [ADR-GRD-001](../../../../architecture/decisions/ADR-GRD-001-capa-hooks-instalacion.md) § 4 (desinstalación y recuperación) y [ADR-GRD-007](../../../../architecture/decisions/ADR-GRD-007-acciones-reservadas-excepcion.md) § 1 y § 2 (D5), con la Enmienda del 2026-10-08 de ADR-GRD-001.

**Entrega parcial**: retirar la protección de una instalación de este perfil, con anuncio, ventana cancelable, auditoría, recuperación al arrancar y barrido de cortes. Quedan fuera la instalación huérfana (retirar o adoptar, E5) y lo demás del § 4.

## 1. Decisiones

Cada fila es una **Decisión del orquestador (2026-10-08), validada por el Arquitecto** (`nassa-architect:architect`), con sus ajustes ya incorporados.

| # | Decisión | Ajuste de la validación |
|---|---|---|
| D6 | **`guard.uninstall` (reservado, `RepoWrite::Guardrails`) en dos llamadas, con la ventana impuesta por el daemon** (D5): la primera autoriza, audita y anuncia (la entrada `reserved.audit` aceptada llega a todos los clientes, con el solicitante) y guarda la acción pendiente (id aleatorio, `applies_at`); la segunda (`confirm: <id>`), reservada otra vez, solo aplica si la ventana se cerró, el id es el pendiente, no se canceló, no caducó (ventana + 60 s) y la pide **el mismo proceso** (pid y hora de inicio). `guard.cancel` no es reservado (solo mantiene la protección) y el canal no lo ofrece a `raptor-mcp`. Aplicar, cancelar, fallar y caducar van a la auditoría permanente con la aceptación del riesgo (`risk-accepted: ADR-GRD-007 § 2 …` y los vectores no cubiertos). Ventana de 10 s (⚠️ ASSUMPTION de ADR-GRD-007); en depuración, `GITRAPTOR_TEST_GUARD_WINDOW_MS` | La pendiente vive solo en memoria (un reinicio la descarta sin aplicar nada); una sola por repo; el anuncio sale antes de responder; la caducidad se audita; `guard.cancel` rechazado desde MCP por el canal |
| D7 | **Transacción inversa** (ADR-GRD-001 § 4): `config` regular comprobado **antes** de tocar la clave (sin exigir el mismo inodo: Git lo reescribe con cada `git config`) → diario `uninstalling` → clave: si el valor local es el nuestro, se restaura el previo si era **local** (operación tipada nueva `restore_hooks_path`, que admite un valor relativo y nunca uno que empiece por `-`) o se elimina; si otro gestor la cambió, no se toca → borrado de los archivos del diario y de las carpetas vacías (identidad comprobada; un archivo ajeno se queda) → registro, instantánea y diario fuera. **Recuperación al arrancar**: `uninstalling` con la clave ya restaurada (o cambiada por otro) → se borra el resto; con la clave nuestra → la protección sigue y el diario vuelve a `confirmed` (`guard_uninstall_incomplete`) | El diario `uninstalling` solo se escribe tras la ventana; el borrado es idempotente |
| D8 | **CLI**: `raptor guard uninstall [ruta] [--yes]` explica, pregunta (`--yes` responde la pregunta, nunca salta la ventana), anuncia, muestra la ventana ("pulsa Ctrl-C o ejecuta `raptor guard cancel`"), vigila `guard.status` cada 200 ms por si la cancelan y confirma al cerrarse. `raptor guard cancel [ruta]`. `guard.status` muestra la pendiente (`GuardStatus.pending`, capacidad `guard.pending-action`) | El Arquitecto pidió que Ctrl-C llame a `guard.cancel` (best effort): **no se hace**, porque exige un manejador de señales y una dependencia nueva. Ctrl-C mata el comando antes de confirmar, así que no se aplica nada; la pendiente caduca sola o se descarta con `raptor guard cancel` |
| D9 | **"Como estaba"** (Q-GRD-29): la huella de todos los ámbitos de la máquina temporal (repo, repo "otro", home con la configuración global, configuración de sistema) tras desinstalar es la de antes de instalar, con las únicas excepciones `guardrails_uninstalled` (tiempos del directorio común e identidad del `config`), y el `config` se compara además **byte a byte**. La diferencia de formato de una línea escrita a mano (SPIKE-GRD-001 § 5.1) sigue declarada | — |
| D10 | **Puntos de corte** (`test-cuts`, nunca en release): `uninstall-journal`, `uninstall-key`, `uninstall-folder` y `uninstall-clear`, antes y después; el protocolo de `gitraptor_testkit::cut`. El daemon muere en el punto y la recuperación corre en el siguiente arranque | Pedido por el coordinador (nota 3): feature de prueba, no `debug_assertions` |

## 2. Forma

| Pieza | Ubicación |
|---|---|
| Transacción inversa y recuperación | `crates/core/src/guardrails/uninstall.rs` |
| Acciones pendientes (ventana, solicitante, caducidad) | `crates/core/src/guardrails/pending.rs` (en `GuardRegistry`) |
| Puntos de corte | `crates/core/src/guardrails/cut.rs` (feature `test-cuts` de `gitraptor-core` y `gitraptor-cli`) |
| Escritura de la clave previa | `crates/git/src/guard_write/mod.rs` (`restore_hooks_path`), `crates/git/src/invoke.rs` (`GuardSubcommand::Restore`) |
| Peticiones al bucle y auditoría | `crates/core/src/daemon/guard.rs`, `crates/core/src/daemon/shutdown.rs` |
| Métodos, capacidad y código `guard-uninstall-refused` (bloque −33040) | `crates/api/src/guard.rs`, `crates/api/src/methods/guard.rs`, `crates/core/src/channel/conn.rs` |
| Variables de prueba que el cliente pasa al daemon | `crates/core/src/client.rs` (`debug_overrides`) |
| CLI y mensajes | `apps/cli/src/guard.rs` (`uninstall`, `cancel`), `apps/cli/src/commands/guard.rs`, `apps/cli/i18n/{en,es}/*.txt`, `apps/cli/src/present/i18n.rs` |

## 3. Pruebas

| Escenario o criterio | Test |
|---|---|
| E1 · Desinstalar restaura el estado exacto (sin hooks previos, con un hook propio que sigue funcionando y con un `core.hooksPath` local previo que vuelve igual); `config` byte a byte | `guard_us_grd_003::repo_intact::e1_*` |
| E2 · Tras retirar, un force-push con Git directo no se evalúa | `…::e2_after_uninstall_a_force_push_is_not_evaluated` |
| E4 · Nada cambia fuera del repo en ningún momento (tras proteger y tras retirar) | `…::e4_nothing_changes_outside_the_repo_at_any_moment` |
| D5 · Cancelar dentro de la ventana mantiene la protección | `…::the_window_can_be_cancelled_and_the_protection_stays` |
| BR-AUTH-001 · Un agente no puede desinstalar (rechazo del comando reservado) | `…::an_agent_cannot_uninstall` |
| E3 / NFR-12 · Desinstalación interrumpida: con la clave restaurada se completa al arrancar (huella idéntica); sin restaurar, la protección sigue | `…::recovery::*` |
| NFR-01 · Una instalación deshecha al arrancar con la clave nuestra devuelve el `core.hooksPath` local previo (husky), huella idéntica (hallazgo High de la revisión) | `…::recovery::repo_intact_an_install_undone_at_startup_restores_the_prior_local_key` |
| NFR-12 · Barrido: corte en cada punto de la transacción inversa → completo o idéntico byte a byte | `…::cuts::repo_intact_an_uninstall_cut_at_any_point_is_complete_or_undone` (`--features test-cuts`) |
| Ventana: solo el solicitante, solo tras cerrarse, una por repo, caducidad | `crates/core` `guardrails::pending::tests::*` |
| Valores de la clave que acepta la capa de escritura | `crates/git` `invoke::tests::the_guard_key_takes_only_the_values_it_may` |
| Contrato: métodos de protocolo 9, escritores declarados | `crates/api` `methods::tests::only_protected_paths_write`, `tests/legacy_protocols.rs` |

**Línea base (B1)**: `cargo test --workspace --no-fail-fast` sobre `58f9ff8a` más los dos archivos de test nuevos: solo fallaban los tests del contrato (`guard_us_grd_002`, 7; `guard_us_grd_003`, 9), escritos en rojo antes de implementar; todo lo demás en verde.

## 4. Pendientes y fuera de alcance

| Pendiente | Dueño |
|---|---|
| **E5 · Instalación huérfana: retirarla o adoptarla** (manifiesto mostrado antes, constantes regeneradas al adoptar, sin confirmar la rama base) | Segunda entrega de US-GRD-003, tras la detección `instalacion-huerfana` de US-GRD-004 |
| **E6 · Registro `protection-state`** de instalar y desinstalar en el registro de decisiones (ADR-GRD-006). Hoy queda en la auditoría permanente (quién, cuándo, en qué repo, aceptación del riesgo) | US-GRD-005 / US-GRD-004 |
| Mostrar la acción pendiente (anuncio con la ventana) en la TUI; hoy el anuncio es la entrada `reserved.audit` de todos los clientes, y la CLI la muestra en el `status` | Cockpit (US-CKP) |
| Exponer en `audit.list` los resultados `applied`, `cancelled`, `failed` y `expired` (hoy solo en la tabla permanente; `AuditOutcome` cambiaría de forma) | Historia del contrato de auditoría |
| Ctrl-C que llama a `guard.cancel` (necesita un manejador de señales) | Declarado (D8) |
| Daemon que se cierra si cambia la identidad de su propio ejecutable (riesgo D10 de DS-US-GRD-001) y ADR-GRD-001 Validación 9 (`binario-no-valido`) | Segunda entrega de US-GRD-003 |
| Linux en máquina real y Windows (sin canal) | **Pendiente: etapa de validación multiplataforma** |

## Estado de la implementación (2026-10-08)

Implementado en: PR #191 (parcial): E1 a E4 y la ventana; faltan E5 y E6.
