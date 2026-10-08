---
id: DS-US-GRD-004
title: "Dev Spec — US-GRD-004: el desarrollador se entera de que la protección dejó de estar activa"
type: dev-spec
status: draft
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
| D6 | **Disparadores**: módulo del daemon `modules/guard_health.rs` (una línea en `MODULES`) = hilo de comprobación **cada 60 s con jitter** + su punto `git_event` (comprobación a lo sumo cada 5 s por repo). La comprobación completa corre en ese hilo sobre una **instantánea de integridad** que el bucle publica en `GuardRegistry` al instalar y al arrancar (ruta del `common-dir`, copia del diario, observado sí/no): no abre la tienda ni despierta repos dormidos (ADR-GRP-015). El hilo es barato (ajuste del coordinador): primero una **huella por `stat`** (mtime y tamaño de `config`, de la carpeta y de cada archivo del diario) y solo si cambia abre el repo y hashea; sin cambios no abre gix. Se añade al banco de reposo (`apps/cli/benches/idle.rs`) y el PR muestra la diferencia de CPU en reposo. El hilo envía `Control::GuardHealth{repo_id, health}` al bucle **solo si cambia** frente a lo seguido. Además se comprueba en cada `guard.status`, al arrancar y tras la instalación o desinstalación propias. Antes de pasar a `inactive` hay **una segunda lectura a ~1 s** (descarta una escritura no atómica de otra herramienta). Interruptor de pruebas `GITRAPTOR_TEST_GUARD_HEALTH_MS` solo con `debug_assertions` | **Desviación declarada de ADR-GRD-005 § 4** (≤ 5 s para repos observados con vigilante de `config` y `gitraptor/`; ADR-GRP-010 figura "aplicada" pero el código no lo hace): aquí 60 s o el siguiente evento de Git. El sondeo de dos `stat` cada 5 s queda pendiente (§ 5). Sin vigilante de archivos (PO) |
| D7 | **Seguimiento y aviso**. El estado seguido por repo vive en `GuardRegistry` (precedente: `pending`), no como campo de `Daemon`. Una transición **propia** (instalar o desinstalar por Guardrails) fija el estado seguido sin aviso y se registra con `expected = true` (cierra E6 de US-GRD-003). Una transición **externa** se registra y publica `guard.protection-lost` con rebote de **10 minutos por (repo, causa)** (⚠️ **ASSUMPTION** de ADR-GRD-005 § 5; se añade a `business-rules.md` como supuesto); la primera siempre avisa; la recuperación (reinstalar) se registra sin aviso. Sin cliente abierto, el aviso queda pendiente porque `guard.status` muestra `hooks.inactive` al abrir la CLI o la TUI (declarado). Sin reparación automática: la acción sugerida es `raptor guard install` (reservado) | "Esperada" en memoria desvía ADR § 5 (diario); aceptable porque todo corre en el bucle y el arranque recupera `Installing` y `Uninstalling`. Tras un reinicio el rebote se pierde: la primera comprobación puede avisar una vez más (declarado) |
| D8 | **Registro (US-GRD-005)**: `LogKind::ProtectionState` y una variante de `LoggedOperation` con `from`, `to`, `cause`, `worktree`, `expected`; capa `guardrails`, origen `daemon`; la tabla `guardrails_decisions` ya lo admite, sin migración. Las repeticiones de una causa persistente o de una clave que alterna se **agregan** por la `agg_key` existente (contador), así no inundan el registro. `guard.log` filtra estas entradas para una conexión sin la capacidad | `guard_record` descarta entradas de repos no observados: escenario 3 pendiente |
| D9 | **Reinstalar corrige** (criterio "al reinstalar desaparece"): hoy `plan` sobre un diario confirmado con la clave cambiada da `OrphanFolder` y `install` lo rechaza; solo `upgrade()` repara un dispatcher borrado. Con `hooks.inactive` por una causa de H2 o H3, `raptor guard install` actúa como **reinstalación de la misma instalación**: la transacción de ADR-GRD-001 § 4 reconstruye la carpeta y escribe la clave, tomando el `core.hooksPath` vigente como `prior` si no es el nuestro (como sobre husky), con la permisividad de siempre (permiso, ventana, diario). Nunca borra un archivo ajeno. **Ajuste del coordinador (2026-10-08):** la pantalla de permiso dice que es una **reparación** y muestra exactamente qué cambia (el `core.hooksPath` vigente que pasa a ser el `prior` encadenado y los archivos que se reescriben); sigue siendo un comando reservado y **la comprobación de salud nunca lo ejecuta**; un corte a mitad de la reparación (feature `chaos`) devuelve el estado exacto de antes de repararla. Es la parte de mayor riesgo NFR-01: se prueba con repos temporales | Hallazgo 3 del Arquitecto: la acción sugerida no funcionaba |
| D10 | **Superficies**: `raptor guard status` (línea de estado con causa y la acción), `raptor status` (una línea por repo protegido con pérdida o diagnóstico) y la TUI (el hueco `protection` de la barra de estado, `tui/view.rs`, con el catálogo tipado). Mensajes en `apps/cli/i18n/{en,es}/guard.txt` (grupo `guard.`) y `present/i18n.rs` | Honestidad del estado (PO): `guard status` añade una nota breve "no detectado: repo fuera de la observación, firma del binario" |

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

Repos y directorios temporales con `guard_machine::Machine`; nunca este repo ni el perfil real. Esperas por señal (evento o consulta), sin `sleep` fijo.

| Escenario o criterio | Test |
|---|---|
| E1 · Cada forma típica de romper la protección (clave cambiada, carpeta borrada, dispatcher borrado, dispatcher editado, permiso de ejecución quitado, binario ausente) da `hooks.inactive` con su causa, `state = unprotected`, entrada `protection-state` y evento `protection-lost` en menos de 1 minuto o con el siguiente evento de Git | `guard_us_grd_004::loss::*` |
| E1 · Editar solo el manifiesto no cambia el estado; editar a la vez un dispatcher y el manifiesto da `dispatcher-altered` (ADR-GRD-005 Validación 3) | `…::loss::manifest_*` |
| E2 · Desinstalar o instalar desde Guardrails no avisa y queda registrado como esperado | `…::expected::*` |
| D9 · Reinstalar tras cada causa deja `hooks.active`, borra el aviso, sin tocar archivos ajenos; `config` de otro gestor conservado como `prior` | `…::repair::*` |
| Rebote · Una causa persistente da un aviso y una entrada agregada; recuperación sin aviso | `…::debounce::*` |
| D2 · Plantilla antigua: `active` con `template-outdated`, sin evento | `…::template_outdated_is_a_warning_not_a_loss` |
| D3 · Huérfana: perfil borrado con clave y manifiesto → `orphaned` | `…::orphan_is_shown_as_orphaned` |
| D1 · Error de lectura de config ⇒ `config-unreadable`, no pérdida | `crates/core` `guardrails::health::tests::*` |
| Sin escrituras · la comprobación no modifica el repo (huella idéntica) | `…::check_writes_nothing` |
| E4 · `minimumSet` activo y `notPreventable` por backend de refs (ya existe, se verifica) | `…::minimum_set_and_not_preventable` |
| Contrato · capacidad `guard.protection`, recorte para un cliente sin ella | `crates/api` tests de métodos; `crates/core` `channel` |
| CLI/TUI · textos en/es con las mismas claves y marcadores | `apps/cli` tests de i18n existentes; test de la línea de estado |
| Cortes · reinstalación interrumpida (`chaos`): completa o idéntica | `…::repair::cuts_*` |

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
| Linux y Windows en máquina real | **Pendiente: etapa de validación multiplataforma** |
