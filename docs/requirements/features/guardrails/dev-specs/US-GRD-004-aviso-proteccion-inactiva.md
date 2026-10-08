---
id: DS-US-GRD-004
title: "Dev Spec — US-GRD-004: el desarrollador se entera de que la protección dejó de estar activa"
type: dev-spec
status: partially-implemented
feature: guardrails
domain: GRP
story: US-GRD-004
created: 2026-10-08
updated: 2026-10-08
related:
  stories: [US-GRD-004, US-GRD-001, US-GRD-003, US-GRD-005, US-GRP-006, INF-GRD-001]
  adrs: [ADR-GRD-001, ADR-GRD-005, ADR-GRD-006, ADR-GRP-010, ADR-GRP-015, ADR-GRP-016]
  rules: [BR-WF-002, BR-EDGE-003]
  nfrs: [NFR-01, NFR-GRD-10, NFR-GRD-13]
tags: [guardrails, estado-proteccion, deteccion-perdida, hooks-git, registro]
---

# Dev Spec — US-GRD-004: aviso de protección inactiva

Plano compacto (AADD ligero) de [US-GRD-004](../user-stories/US-GRD-004-aviso-proteccion-inactiva.md). Gobierno: [ADR-GRD-005](../../../../architecture/decisions/ADR-GRD-005-estado-proteccion.md) § 1, § 3, § 4 y § 5.

**Entrega parcial (M2a).** Entra la detección de las causas típicas de pérdida de la capa de hooks, su estado y causa en `guard.status`, el aviso en la CLI y la TUI (en/es), el registro `protection-state` (US-GRD-005) y el evento `protection-lost`. Quedan pendientes, con dueño, en el § 5.

## 1. Decisiones

Cada fila es una **Decisión del orquestador (2026-10-08), validada por el Arquitecto** (`nassa-architect:architect`) y por el PO (`nassa-aadd:product-owner`), con sus ajustes incorporados.

| # | Decisión | Ajuste de la validación |
|---|---|---|
| D1 | **Detector de solo lectura** `guardrails/health.rs`, sin escribir en el repo (ADR-GRD-005 Validación 11). `check(snapshot) -> Health` evalúa H1 a H4 reducido. Causas (código en inglés; ADR § 1 entre paréntesis): `hookspath-changed` (`hookspath-cambiado`), `repo-moved` (`repo-movido`: la clave coincide con la ruta del diario pero no con la del `common-dir` actual), `folder-missing` (`carpeta-ausente`), `dispatcher-missing`, `dispatcher-altered` (`dispatcher-alterado`), `dispatcher-not-executable` (solo `cfg(unix)`; un permiso de ejecución quitado y un dispatcher borrado son la misma condición: hooks que no pueden correr), `binary-missing` (`binario-ausente`: existencia de la ruta estable; la firma o huella queda pendiente). **El conjunto de integridad es `Journal.files` menos `manifest.json`** (ADR-GRD-001 Validación 9: editar el manifiesto no cambia el estado). Un error al leer la configuración **no** es "clave ausente": da el diagnóstico `config-unreadable`, nunca `hookspath-changed`. La identidad `dev/inode` de la carpeta **no** es señal de pérdida (falsos positivos al copiar o restaurar el repo, y volúmenes remontados): se compara por existencia y hashes | El Arquitecto añadió `repo-moved`, excluyó el manifiesto, separó "plantilla antigua" y pidió declarar que `-c core.hooksPath=` o `GIT_CONFIG_*` por proceso no se detectan |
| D2 | **Plantilla antigua no es pérdida**: `journal.template < TEMPLATE_VERSION` deja los hooks `active` y añade el diagnóstico `template-outdated` con la acción `raptor guard install`. Sale **solo** de la versión del diario, no de `install::outdated()` (que también devuelve `true` con un dispatcher borrado y se evaluaría después de H3) | Hallazgo 2 del Arquitecto |
| D3 | **Instalación huérfana**: sin diario pero con la clave igual a `<common>/gitraptor/hooks` y el manifiesto presente, `hooks = orphaned`, nunca `not-installed`. Se detecta en `guard.status` y al volver a añadir el repo (no en el tick: un repo sin perfil no está registrado). Sin acción ofrecida hasta US-GRD-003 E5 | Aprobado por el Arquitecto |
| D4 | **`status()` delega en el detector** (no usa más `in_place`): `state` y `hooks` no pueden divergir. `hooks.inactive` ⇒ `state = unprotected` (escenario 1) | Hallazgo 4 |
| D5 | **API, una sola capacidad `guard.protection`** (`crates/api/src/methods/guard.rs`): `GuardStatus` gana `hooks {status: active\|inactive\|not-installed\|orphaned, cause?, worktree?}`, `diagnostics[]` (`template-outdated`, `base-unconfirmed`, `config-unreadable`) y `minimumSet {status: active}` (escenario 4, fijo en `active` en esta historia). Con `serde(default, skip_serializing_if)`; el daemon los quita para una conexión sin la capacidad. La misma capacidad cubre el nuevo `LogKind::ProtectionState` y el evento `guard.protection-lost`. `worktree` y todo valor ajeno de `core.hooksPath` van como `Untrusted`; **no se registra el valor ajeno, solo su clase** (`other-path`, `empty`, `relative`) | Una capacidad en vez de dos (combinaciones rotas); `minimumSet` ahora para no pagar otra capacidad |
| D6 | **Disparadores**: módulo del daemon `modules/guard_health.rs` (una línea en `MODULES`) = hilo de comprobación **cada 60 s con jitter** + su punto `git_event` (comprobación a lo sumo cada 5 s por repo). La comprobación completa corre en ese hilo sobre una **instantánea de integridad** que el bucle publica en `GuardRegistry` al instalar y al arrancar (ruta del `common-dir`, copia del diario, observado sí/no): no abre la tienda ni despierta repos dormidos (ADR-GRP-015). El hilo es barato (ajuste del coordinador): primero una **huella por `stat`** (mtime y tamaño de `config`, de la carpeta y de cada archivo del diario) y solo si cambia abre el repo y hashea; sin cambios no abre gix. Se añade al banco de reposo (`apps/cli/benches/idle.rs`) y el PR muestra la diferencia de CPU en reposo. El hilo envía `Control::GuardHealth{repo_id, health}` al bucle **solo si cambia** frente a lo seguido. Además se comprueba en cada `guard.status`, al arrancar y tras la instalación o desinstalación propias. Antes de pasar a `inactive` hay **una segunda lectura** (1 s; en las pruebas, el intervalo) (descarta una escritura no atómica de otra herramienta). Interruptor de pruebas `GITRAPTOR_TEST_GUARD_HEALTH_MS` solo con `debug_assertions` | **Desviación declarada de ADR-GRD-005 § 4** (≤ 5 s para repos observados con vigilante de `config` y `gitraptor/`; ADR-GRP-010 figura "aplicada" pero el código no lo hace): aquí 60 s o el siguiente evento de Git. El sondeo de dos `stat` cada 5 s queda pendiente (§ 5). Sin vigilante de archivos (PO) |
| D7 | **Seguimiento y aviso**. El estado seguido por repo vive en `GuardRegistry` (precedente: `pending`), no como campo de `Daemon`. Una transición **propia** (instalar o desinstalar por Guardrails) fija el estado seguido sin aviso y se registra con `expected = true` (cierra E6 de US-GRD-003). Una transición **externa** se registra y publica `guard.protection-lost` con rebote de **10 minutos por (repo, causa)** (⚠️ **ASSUMPTION** de ADR-GRD-005 § 5; se añade a `business-rules.md` como supuesto); la primera siempre avisa; la recuperación (reinstalar) se registra sin aviso. Sin cliente abierto, el aviso queda pendiente porque `guard.status` muestra `hooks.inactive` al abrir la CLI o la TUI (declarado). Sin reparación automática: la acción sugerida es `raptor guard install` (reservado) | "Esperada" en memoria desvía ADR § 5 (diario); aceptable porque todo corre en el bucle y el arranque recupera `Installing` y `Uninstalling`. Tras un reinicio el rebote se pierde: la primera comprobación puede avisar una vez más (declarado) |
| D8 | **Registro (US-GRD-005)**: `LogKind::ProtectionState` y una variante de `LoggedOperation` con `from`, `to`, `cause`, `expected` (sin `worktree`: H2 por worktree es pendiente); capa `hooks` (la de la capa de hooks, que es de la que se informa; el contrato solo conoce esa), origen `daemon`; la tabla `guardrails_decisions` ya lo admite, sin migración. Las repeticiones de una causa persistente o de una clave que alterna se **agregan** por la `agg_key` existente (contador), así no inundan el registro. `guard.log` filtra estas entradas para una conexión sin la capacidad | `guard_record` descarta entradas de repos no observados: escenario 3 pendiente |
| D9 | **Reinstalar corrige** (criterio "al reinstalar desaparece"): hoy `plan` sobre un diario confirmado con la clave cambiada da `OrphanFolder` y `install` lo rechaza; solo `upgrade()` repara un dispatcher borrado. Con `hooks.inactive` por una causa de H2 o H3, `raptor guard install` actúa como **reinstalación de la misma instalación**: la transacción de ADR-GRD-001 § 4 reconstruye la carpeta y escribe la clave, tomando el `core.hooksPath` vigente como `prior` si no es el nuestro (como sobre husky), con la permisividad de siempre (permiso, ventana, diario). Nunca borra un archivo ajeno. **Ajuste del coordinador (2026-10-08):** la pantalla de permiso dice que es una **reparación** y muestra exactamente qué cambia (el `core.hooksPath` vigente que pasa a ser el `prior` encadenado y los archivos que se reescriben); sigue siendo un comando reservado y **la comprobación de salud nunca lo ejecuta**; un corte a mitad de la reparación (feature `chaos`) deja la protección **tan inactiva como estaba** (misma causa, la clave del otro gestor y sus archivos intactos) **o reparada**, y reinstalar la completa; **no** restaura byte a byte los dispatchers propios que ya se reescribieron (eran de Guardrails; lo editado se reemplaza por lo propio, lo que la pantalla de permiso dice). Desviación declarada de la petición del coordinador ("restaura el estado exacto"). Es la parte de mayor riesgo NFR-01: se prueba con repos temporales | Hallazgo 3 del Arquitecto: la acción sugerida no funcionaba |
| D10 | **Superficies**: `raptor guard status` (línea de estado con causa y la acción), `raptor status` (una línea por repo protegido con pérdida o diagnóstico) y la TUI (el hueco `protection` de la barra de estado, `tui/view.rs`, con el catálogo tipado). Mensajes en `apps/cli/i18n/{en,es}/guard.txt` (grupo `guard.`) y `present/i18n.rs` | Honestidad del estado (PO): `guard status` añade una nota breve "no detectado: repo fuera de la observación, firma del binario" |
| D11 | **El bucle lee otra vez lo que el hilo vio**: `Control::GuardHealth` solo lleva el id del repo; el bucle, único punto donde nada más cambia la instalación, vuelve a comprobar contra la instalación publicada y descarta el informe si ya no hay ninguna. Sin esto, una desinstalación propia que el hilo vio a medias salía como pérdida externa (carrera hallada con la suite e2e) | Hallazgo propio, verificado con 5 ejecuciones seguidas de la suite |
| D12 | **Evento de recuperación** `guard.protection-restored` (misma capacidad y mismos datos que `guard.protection-lost`): sin él, la TUI abierta no podía quitar el aviso al reinstalar. La TUI además pregunta `guard.status` al abrir un repo (la pérdida que ocurrió con la TUI cerrada) | Decisión de implementación dentro de D5 y D10 |
| D13 | **Reinstalar no es negar el permiso**: si el desarrollador responde "no" a una reparación, no se llama a `guard.decline` (que marcaría el permiso como denegado y no volvería a ofrecerlo) | Hallazgo propio |
| D14 | **Prueba corregida tras la revisión** (autorizada por el coordinador, 2026-10-08): `loss::the_manifest_is_not_the_reference` afirmaba `transitions(&m).is_empty()` tras editar el manifiesto, pero la instalación propia queda registrada como transición esperada (E6, que exige esta spec). Antes: `assert!(transitions(&m).is_empty())`. Después: las transiciones son **exactamente** una, a `active` y `expected = true`; una transición de más sigue fallando. No cambia lo que prueba el criterio | Contradecía la Dev Spec (E6) |
| D15 | **Banco de reposo**: `cargo bench -p gitraptor-cli --bench idle -- --protected` instala la capa de hooks en el repo temporal del banco y mide la CPU en reposo con la comprobación periódica; el PR muestra la diferencia con y sin la bandera | Ajuste del coordinador |
| D16 | **Correcciones de la revisión** (Opus, 2026-10-08): (a) reparar con solo `gitraptor/hooks/` borrado ya no falla con `OrphanFolder`; (b) al reparar con la clave cambiada, la pantalla lista los archivos de la carpeta **como quedará** (con los hooks que encadena la clave nueva), los dispatchers de solo encadenado del prior anterior que siguen intactos se retiran, los editados quedan en el diario, y **nunca** se reemplaza un archivo que no es del diario (se rechaza con `OrphanFolder`); (c) una protección inactiva se puede retirar (`guard uninstall` ya no responde "no instalada"): con la clave de otro gestor, esta se deja intacta; (d) un cliente sin `guard.protection` no puede ejecutar una reparación que no vio (`guard.install` se rechaza como antes); (e) `unwatch` limpia el estado seguido y los rebotes; (f) el bucle descarta un informe mientras el diario no esté `confirmed` (una instalación o desinstalación propia a medias no es una pérdida externa); (g) la clave se compara como ruta; (h) `repo-moved` se detecta también sin diario (la clave nombra la carpeta de hooks de otro sitio) y no se ofrece reparación (pendiente de US-GRD-003 E5). Quedan declaradas: `guard.log` filtra después del `LIMIT` para un cliente sin la capacidad (páginas más cortas) | Revisión de código del PR |
| D17 | **Correcciones de la revisión de seguridad** (Opus, 2026-10-08): (a) los archivos de la carpeta se leen con `read_regular` (sin seguir enlaces, sin bloquear con una tubería y con tope de 32 MiB): una tubería puesta donde había un dispatcher es `dispatcher-altered` y no congela el bucle; (b) un informe de un repo dormido despierta el repo antes de comprobar el diario; (c) una reparación confirmada adopta como identidad la de la carpeta tal como está (una copia o una restauración idéntica), de modo que reparar y retirar siguen funcionando; (d) la huella incluye `ctime`, que un usuario normal no puede retrasar con `touch -r`; (e) la pantalla de reparación muestra lo que la misma detección va a encadenar, a cualquier nivel; (f) `guard.protection` no llega a `raptor-mcp` (BR-AUTH-004); (g) el banco rechaza un `raptor` que no sea de depuración. **Declarado, pendiente**: la pantalla muestra el valor visto pero `guard.install` no lo envía de vuelta para rechazar si cambió entre la pantalla y la aplicación (misma exposición que el flujo de hooks previos de US-GRD-002); retirar una protección cuya carpeta se reemplazó por otra distinta sigue pidiendo reparar antes | Revisión de seguridad del PR |

## 2. Forma

| Pieza | Ubicación |
|---|---|
| Detector (lectura) y causas | `crates/core/src/guardrails/health.rs` (nuevo) |
| `status()` delega en el detector; reinstalación de D9 | `crates/core/src/guardrails/install.rs` |
| Instantánea y seguimiento | `crates/core/src/guardrails/registry.rs` (`GuardRegistry`) |
| Hilo de comprobación | `crates/core/src/daemon/modules/guard_health.rs` + una línea en `MODULES` |
| Transición, registro y aviso | `crates/core/src/daemon/guard.rs`; un `Control::GuardHealth` en `daemon/shutdown.rs` |
| Registro `protection-state` | `crates/core/src/guardrails/log.rs`, `crates/core/src/profile/guard_log.rs` |
| Contrato: `hooks`, `diagnostics`, `minimumSet`, `LogKind`, evento, capacidad | `crates/api/src/guard.rs`, `crates/api/src/methods/guard.rs`, `crates/api/src/event.rs` |
| Recorte por capacidad | `crates/core/src/channel/conn.rs` |
| CLI y mensajes | `apps/cli/src/guard.rs`, `apps/cli/src/status.rs`, `apps/cli/i18n/{en,es}/guard.txt` |
| TUI | `apps/cli/src/tui/view.rs`, `apps/cli/src/tui/widgets/layout.rs`, `apps/cli/src/present/i18n.rs` |

## 3. Pruebas

Repos y directorios temporales con `guard_machine::Machine`; nunca este repo ni el perfil real. Esperas por señal (el registro, el estado o un evento), sin `sleep` fijo.

| Escenario o criterio | Test |
|---|---|
| E1 · Cada forma típica de romper la protección (clave cambiada, carpeta borrada, dispatcher borrado, editado o sin permiso de ejecución) da `hooks.inactive` con su causa, `state = unprotected`, entrada `protection-state` y aviso (CLI) en menos de 1 minuto; el daemon de prueba comprueba cada 100 ms | `apps/cli/tests/guard_us_grd_004.rs`: `loss::*`, `surfaces::*` |
| E1 · Editar solo el manifiesto no cambia el estado; editar a la vez un dispatcher y el manifiesto da `dispatcher-altered` | `…::loss::the_manifest_is_not_the_reference` |
| E1 · Binario ausente, error de lectura de la configuración, repo movido, huérfana por clave y manifiesto, plantilla antigua (aviso, no pérdida), huella por `stat` | `crates/core/src/guardrails/health_tests.rs` |
| E2 · Instalar y desinstalar desde Guardrails no avisan y quedan como transición esperada | `…::expected::guardrails_own_changes_do_not_alert`; `guard_us_grd_004_events.rs::guardrails_own_removal_publishes_no_loss` |
| D9 · Reinstalar tras cada causa deja `hooks.active`, borra el aviso y no toca archivos ajenos; sin reparar a solas | `…::repair::*`; `guard_us_grd_004_review.rs` (carpeta de hooks sola, retirar una protección inactiva, cadena que cambia, archivo ajeno) |
| D9 · La pantalla de permiso dice que es una reparación y qué cambia (archivos y `core.hooksPath` encadenado), en en/es; responder "no" no niega el permiso | `guard_us_grd_004_repair.rs` |
| D9 · Cortes de la reparación (`repair-files`, `repair-key`, antes y después): completa o como estaba, y reinstalar la completa | `guard_us_grd_004_repair.rs::cuts::*` |
| Rebote · Una causa persistente es una entrada agregada; primera pérdida avisa, repetida dentro de 10 minutos no; recuperación sin aviso | `…::debounce::a_persistent_cause_is_one_entry`; `crates/core` `guardrails::protection::tests::*` |
| D3 · Huérfana: perfil borrado con clave y manifiesto → `orphaned` | `…::debounce::an_orphan_install_is_shown_as_orphaned` |
| Eventos · `guard.protection-lost` y `guard.protection-restored` llegan a un cliente suscrito | `guard_us_grd_004_events.rs::a_loss_and_its_repair_are_published` |
| Sin escrituras · la comprobación no modifica el repo (huella idéntica) | `…::loss::the_check_writes_nothing` |
| E4 · `minimum_set` activo | `…::surfaces::the_minimum_set_is_visible` |
| Contrato · pruebas existentes de capacidades, métodos y mensajes en/es | `crates/api` y `apps/cli` (i18n) |

**Línea base (B1)**: `cargo test --workspace --no-fail-fast` sobre el árbol con las pruebas en rojo: fallaban solo las pruebas del contrato (`guard_us_grd_004`, 17; `health_tests`, 1 compilando en rojo) y `a_burst_of_1000_events_is_painted_within_the_cockpit_budget` (`tui_loop`, de rendimiento en depuración, ajena a esta historia). La baseline de la corrida se aceptó en rojo por eso (`--accept-red-baseline`), con la admisión en el PR.

## 4. Ejecución

Línea base (B1) y verificación final: `cargo test --workspace`; durante el desarrollo, `cargo test -p <crate> --test guard_us_grd_004 <filtro>`.

## 5. Pendientes y fuera de alcance

| Pendiente | Dueño |
|---|---|
| **Escenario 3 · repo retirado de la observación (`protected-but-unobserved`)**: incumplimiento visible; el criterio 1 de M2 solo se cierra con decisión escrita de Rene | US-GRP-006 (retirar el repo) y esta historia (aviso) |
| Vigilancia de `config` / `config.worktree` / `gitraptor/` (sondeo de `stat` cada 5 s) para cumplir el ≤ 5 s de ADR-GRD-005 § 4 en repos observados; enmienda de ADR-GRP-010 | Segunda entrega de US-GRD-004 |
| H2 por worktree (`config.worktree`): el lector actual lee solo la configuración común. **Hueco de detección en el caso de uso del producto** (un agente por worktree). Va al PR en "Para Rene" con recomendación | Decisión de Rene; segunda entrega |
| H4 con firma o huella del binario (ADR-GRD-001 § 8) y `binario-no-valido` | ADR-GRD-001 § 8 |
| Diagnósticos `floor-relax-pending`, `base-change-pending`, `hook-previo-no-encadenado` | US-GRD-007, US-GRD-014, US-GRD-002 |
| Adoptar o retirar una huérfana | US-GRD-003 E5 |
| `dispatcher-not-executable` en Windows (DACL) y `FileId` | Etapa de validación multiplataforma |
| Límite declarado: `-c core.hooksPath=` y `GIT_CONFIG_*` por proceso no se detectan | — |
| `guard.log` para un cliente sin `guard.protection`: el filtro se aplica después del límite de la consulta (páginas más cortas); filtrar por clase en la consulta | Cuando exista un cliente sin la capacidad (hoy todos la piden) |
| Linux y Windows en máquina real | **Pendiente: etapa de validación multiplataforma** |

## Estado de la implementación (2026-10-08)

Implementado en: PR #201 (parcial). Cumple D1 a D15. Pendiente lo del § 5, con su dueño. Desviación declarada de ADR-GRD-005 § 4: en un repo observado la pérdida se detecta en 60 s o con el siguiente evento de Git (limitado a uno cada 5 s), no en ≤ 5 s; el sondeo de `stat` o el vigilante de `config` y `gitraptor/` es la segunda entrega.
