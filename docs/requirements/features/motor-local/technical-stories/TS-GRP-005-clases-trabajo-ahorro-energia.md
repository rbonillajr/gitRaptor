---
id: TS-GRP-005
title: "Clases de trabajo del daemon y mecanismo de ahorro de energía"
type: ts
status: partially-implemented
feature: motor-local
domain: GRP
priority: high
complexity: high
created: 2026-10-05
updated: 2026-11-08
related:
  adrs: [ADR-GRP-015, ADR-GRP-005, ADR-GRP-010, ADR-CKP-001, ADR-TMC-004, ADR-TMC-006]
  stories: [US-GRP-017, US-GRP-019, US-TMC-004, TS-GRP-003, INF-GRP-002, TS-TMC-001, TS-CKP-001]
  specs: [DS-TS-GRP-005]
ado:
  id: null
  url: null
tags: [motor-local, recursos, prioridad, qos, nice, ionice, ecoqos, bateria, daemon, res-06, res-07, res-08, m1]
---

## TS-GRP-005: Clases de trabajo del daemon y mecanismo de ahorro de energía

**Valor**: GitRaptor cede la máquina al usuario. Su trabajo de fondo corre con prioridad baja del SO, y lo que el usuario está esperando (snapshot previo, Guardrails, operaciones) no baja nunca.

### Descripción

**Como** Arquitecto
**Quiero** una API única de clases de trabajo en `crates/core`, que fije la prioridad del SO de cada hilo o pool y que hereden los `git` hijos, y una fuente de energía inyectable que active el modo de ahorro
**Para** cumplir RES-06, RES-07 y el mecanismo de RES-08 sin que cada módulo decida su prioridad por su cuenta (ADR-GRP-015)

> Dev Spec: [DS-TS-GRP-005](../dev-specs/TS-GRP-005-dev-spec.md) | Escrita (2026-10-27), en dos PR: el PR-A (T001 a T006) cierra G5 de TS-CKP-001
>
> **Depende de**: TS-GRP-003 (daemon y autoarranque), US-GRP-002 (observador y reconciliación implementados) e INF-GRP-002 (escenario de estrés de RES-07). **ADRs**: ADR-GRP-015 (§ 1 y § 2), ADR-GRP-005 § 3 (unidad de autoarranque), ADR-GRP-010 (reconciliación), ADR-CKP-001 (pool del predictor), ADR-TMC-004 y ADR-TMC-006 (separar el hashing de la escritura del almacén).
>
> **Origen**: decisión del orquestador (2026-10-05), validada por el Arquitecto (enabler: ninguna historia es dueña de la prioridad del SO y no hay resultado observable directo) y el PO (la prioridad entra en M1 como Should; el modo de ahorro observable es US-GRP-019).
>
> **Complejidad alta**: tres SO, riesgo de inversión de prioridad sobre el snapshot previo (ADR-TMC-006) y de romper NFR-04.

### Alcance Técnico

- **Crear** en `crates/core` el tipo `WorkClass` (`UserInitiated`, `Default`, `Utility`, `Background`) y una forma de lanzar hilos o pools con una clase, con implementación por SO tras un trait:
  - macOS: `pthread_set_qos_class_self_np` y `setiopolicy_np`.
  - Linux: `setpriority` por hilo e `ioprio_set`.
  - Windows: `SetThreadPriority`, `THREAD_MODE_BACKGROUND_BEGIN` y EcoQoS con `SetThreadInformation`.
- **Aplicar** las clases de la tabla de ADR-GRP-015 § 1 a los trabajos que ya existen: observador y canal en `Default`, reconciliación periódica en `Utility`, peticiones IPC en `UserInitiated`.
- **Exponer** la API para los trabajos que llegan después: captura continua y mantenimiento (TS-TMC-001, US-TMC-004), predictor (TS-CKP-001) y evaluación de Guardrails.
- **Lanzar** los `git` hijos con la clase del hilo que los pide (en macOS, la política de E/S en `pre_exec`). Medir el coste de perder `posix_spawn`.
- **Ajustar** la unidad de autoarranque: launchd con `ProcessType=Standard`; systemd sin `Nice=`.
- **Crear** el trait `PowerSource` (con corriente, con batería, bajo consumo, desconocido) con una implementación por SO (`IOPSCopyPowerSourcesInfo` y modo de bajo consumo; `/sys/class/power_supply`; `GetSystemPowerStatus`) y una implementación simulada para los tests.
- **Leer** la clave `engine.powerSaving` (`auto` | `on` | `off`, solo en el perfil; ADR-GRP-007) y publicar el modo como estado del motor en ≤ 60 s tras un cambio de fuente.
- **Aplicar** el modo en la reconciliación periódica (cada 15 min). Exponer el modo a los consumidores (el predictor se pausa, el mantenimiento se aplaza) para que lo apliquen sus historias.
- **Fuera de alcance**: el comportamiento observable del modo y su verificación de punta a punta (US-GRP-019); la vista del consumo (US-GRP-017); el gate de huella (INF-GRP-002).

### Plan de Verificación

#### Pruebas Automatizadas

- **RES-06**: dentro de un trabajo de cada clase, la prioridad leída del SO es la esperada (`qos_class_self()`, `/proc/<tid>/stat` e `ionice -p`, `GetThreadPriority`). Un `git` hijo de un trabajo `Background` sale con nice 10 (`ps -o ni`).
- **RES-07** (bloquea el merge): escenario de estrés en INF-GRP-002 con todos los núcleos saturados por un proceso ajeno. El snapshot previo sigue < 200 ms p95, la espera del previo al escritor es ≤ 10 ms y el motor ≤ 300 ms p95.
- **Modo de ahorro**: con `PowerSource` simulado y reloj simulado, en 60 min hay 4 reconciliaciones periódicas con batería y 12 con corriente; el estado del motor cambia en ≤ 60 s tras el cambio de fuente; `on` y `off` mandan sobre la fuente.
- **Repo intacto**: la suite de INF-GRP-001 sigue en verde con las clases aplicadas.
- Tests con repos y perfiles temporales, nunca este repo ni el perfil real (NFR-01).

#### Verificación Manual / Sandbox

- macOS: desenchufar el portátil y comprobar el modo en `raptor status --resources` (cuando exista US-GRP-017).
- Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

## Estado de la implementación (2026-11-08)

Implementado en: PR #298 (PR-A: T001 a T006) y PR #PRB (PR-B: T007 a T014 de DS-TS-GRP-005). Queda en `partially-implemented` por los pendientes de abajo.

- **Hecho en el PR-A (macOS):** `macsys::qos`, `work_class` (clases, tramos y registro de pools), `power` (modo resuelto de `engine.powerSaving` y la fuente), el hilo `raptor-power` y `Daemon::power()`, y las clases de los hilos que no tocan el camino del snapshot previo. Decisión D17: los hilos reciben solo la QoS.
- **Hecho en el PR-B (macOS):**
  - Fuente de energía real: batería por IOKit y bajo consumo por `NSProcessInfo` (`macsys::power`, `ffi_power`). Con `auto`, desenchufar o activar el bajo consumo pone el motor en ahorro en ≤ 30 s. Revisión de seguridad aceptada, `objc_msgSend` incluido; registrada en ADR-GRP-002 (cierra G1).
  - `engine.resources` publica `power_saving` en todos los SO y `pools` (los pools vivos) en macOS.
  - En ahorro, la reconciliación periódica pasa de 5 a 15 min, y el barrido y la reconciliación lenta de los dormidos pasan a 300 s y 180 min. Al volver a corriente, el próximo plazo se recalcula sin disparar una reconciliación.
  - Clases del camino del previo (D18): `raptor-conn`, `raptor-prior` y el hashing de un previo en `UserInitiated`, `raptor-tm-capture` en `Default` como pool `Capture`, y `raptor-tm-seed` en `Background`.
  - Un `git` lanzado por un hilo `Utility` o `Background` sale con nice 5 o 10, sin bajar nunca de un nice heredado mayor. El coste de perder `posix_spawn` es de 0,29 ms en el p95 (presupuesto 5 ms).
  - Test de la plist de launchd (`ProcessType=Standard`).
  - Escenario `--saturated` (RES-07) en los bancos `tm_snapshot` y `engine`.
- **Decisión nueva:** D18. La captura continua hashea con el escritor tomado, así que sigue en `Default` (Enmienda 2026-11-08 de ADR-GRP-015).
- **Pendiente:**
  - **RES-06 del hashing de la captura continua:** espera a [TD-TMC-002](../../time-machine/technical-stories/TD-TMC-002-captura-blobs-fuera-del-escritor.md), que saca los blobs del escritor.
  - **RES-07, la cifra en verde:** el escenario existe, pero en el Mac de desarrollo, con otros workers en marcha (carga 25–38 en 10 núcleos), la espera del previo al escritor ya supera los 10 ms sin carga añadida. La medición válida necesita una máquina tranquila. Las cifras están en el PR y en INF-GRP-002.
- **Linux y Windows:** degradación fail-closed. Las clases no se aplican, `pools` es `null` y la fuente es `Unknown`. Pendiente en la etapa de validación multiplataforma (XP-45).
