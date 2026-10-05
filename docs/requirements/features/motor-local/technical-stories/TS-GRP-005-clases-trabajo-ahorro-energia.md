---
id: TS-GRP-005
title: "Clases de trabajo del daemon y mecanismo de ahorro de energía"
type: ts
status: ready
feature: motor-local
domain: GRP
priority: high
complexity: high
created: 2026-10-05
updated: 2026-10-05
related:
  adrs: [ADR-GRP-015, ADR-GRP-005, ADR-GRP-010, ADR-CKP-001, ADR-TMC-004, ADR-TMC-006]
  stories: [US-GRP-017, US-GRP-019, US-TMC-004, TS-GRP-003, INF-GRP-002, TS-TMC-001, TS-CKP-001]
  specs: []
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

> Dev Spec: `dev-specs/TS-GRP-005-dev-spec.md` | Pendiente
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
