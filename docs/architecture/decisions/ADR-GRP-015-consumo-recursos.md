---
id: ADR-GRP-015
title: "Consumo de recursos: clases de trabajo, ahorro de energía y presupuesto de huella"
type: adr
status: proposed
date: 2026-10-05
created: 2026-10-05
updated: 2026-10-07
deciders: [Rene Bonilla]
domain: GRP
feature: motor-local
related: [BRD-GRP-001, ADR-GRP-005, ADR-GRP-006, ADR-GRP-010, ADR-GRP-011, ADR-GRP-016, ADR-CKP-001, ADR-TMC-004, ADR-TMC-006, ADR-TMC-007, TS-GRP-005, TS-GRP-006, INF-GRP-002, US-GRP-017, US-GRP-018, US-GRP-019, US-TMC-022]
tags: [recursos, huella, cpu, memoria, disco, prioridad, qos, nice, ionice, ecoqos, bateria, ahorro-energia, daemon, res-01, res-09, m1]
---

# ADR-GRP-015 — Consumo de recursos: clases de trabajo, ahorro de energía y presupuesto de huella

> **Estado**: propuesto (2026-10-05). **Decisión del orquestador (2026-10-05), validada por el Arquitecto y el PO**, a partir de la preocupación de Rene Bonilla (2026-10-05) de que GitRaptor no consuma los recursos del equipo del usuario. Pasa a `accepted` cuando Rene Bonilla confirme las cifras de RES-01 y RES-02 (hoy ⚠️ **ASSUMPTION**) y TS-GRP-005 supere RES-07 en macOS.

## Contexto

GitRaptor vive en la máquina del usuario: un daemon por usuario siempre encendido (ADR-GRP-005), un watcher por worktree (ADR-GRP-010), la Time Machine con su almacén en el perfil (ADR-TMC-001) y el predictor de conflictos (ADR-CKP-001). El usuario no lo eligió para que compita con sus agentes, su compilador o su batería.

Hoy el consumo solo se cubre en dos sitios:

- La fila **HUELLA** de [non-functional.md](../non-functional.md), con objetivos sin gate: CPU en reposo menor que 1 % y RSS menor que 150 MB con 10 worktrees. SPIKE-GRP-002 midió en macOS un 0,04 % de CPU en reposo y hasta 200 MiB de RSS como cota superior.
- ADR-CKP-001 pone el predictor en un pool de prioridad baja.

Faltan cuatro cosas:

1. Una regla común que diga **en qué prioridad del SO corre cada trabajo**.
2. Qué hace el motor **con batería**.
3. Un **gate** de la huella.
4. Una forma de que el usuario **vea** cuánto consume GitRaptor.

El tope de disco de la Time Machine no está en este ADR: es de la Time Machine y va como enmienda de ADR-TMC-007 § 5.

Hay dos restricciones que no se pueden romper:

- **NFR-04**: el motor tarda como mucho 300 ms en p95. Está confirmado en macOS con el observador en la prioridad por defecto.
- **ADR-TMC-006**: el snapshot previo tarda menos de 200 ms en p95 y bloquea la operación del usuario. **Nunca se degrada**.

## Decisión

### 1. Clases de trabajo

Todo trabajo del daemon declara una de cuatro **clases**. La clase fija la prioridad del SO de su hilo o de su pool, y los `git` hijos la heredan.

| Clase | Qué corre | macOS (QoS) | Linux | Windows |
|---|---|---|---|---|
| `user-initiated` | Snapshot previo; hilo escritor del almacén (ref y oplog); ejecutor de operaciones; peticiones IPC y MCP; evaluación de los hooks de Guardrails | `QOS_CLASS_USER_INITIATED` | nice 0 | Normal |
| `default` | Observador, debounce y recomputo, canal y stream, reconciliación disparada por eventos | `QOS_CLASS_DEFAULT` | nice 0 | Normal |
| `utility` | Predictor (ADR-CKP-001) y reconciliación periódica | `QOS_CLASS_UTILITY` | nice 5 | `THREAD_PRIORITY_BELOW_NORMAL` |
| `background` | Hashing de la captura continua (best-effort); consolidación (`repack`), `prune` y purga del almacén (idle) | `QOS_CLASS_BACKGROUND` | nice 10; `ionice` best-effort 7 para la captura e idle para el mantenimiento | EcoQoS para la captura; `THREAD_MODE_BACKGROUND_BEGIN` para el mantenimiento |

- **El observador se queda en `default`**: NFR-04 se confirmó con esa clase y bajarla ahorra muy poco (0,04 % de CPU en reposo). Solo pasaría a `utility` si INF-GRP-002 demuestra que cumple NFR-04.
- **La evaluación de Guardrails va en `user-initiated`** porque su hook bloquea el `git` del usuario.
- **Para evitar la inversión de prioridad**, la captura separa el hashing (`background`) de la escritura de la ref y del oplog, que va en el hilo escritor en `user-initiated`. Así un snapshot previo nunca espera a una captura en segundo plano (RES-07).
- **Sin `SCHED_IDLE` ni `ionice` idle para la captura**: dejarían la captura sin turno y abrirían huecos (BR-CONS-005). Por eso la captura va en best-effort.
- **Autoarranque** (ADR-GRP-005 § 3):
  - launchd: `ProcessType=Standard`. `Background` limitaría el proceso entero (también el snapshot previo) y `Adaptive` solo se eleva por XPC, que el canal no usa.
  - systemd: sin `Nice=` en la unidad. Un proceso sin privilegios no puede volver a subir su prioridad, así que el snapshot previo quedaría penalizado. Solo se pone `CPUWeight=` si el controlador de CPU está delegado (⚠️ **ASSUMPTION**).
  - Windows (HKCU Run): prioridad normal del proceso; las clases se aplican por hilo.
- **Herencia en los hijos**: en Linux, nice e `ionice` se heredan en el `fork`. En macOS, la política de E/S (`setiopolicy_np`) hay que aplicarla en `pre_exec`, que desactiva `posix_spawn`; el coste lo mide TS-GRP-005. En Windows, el hijo se crea con la clase de prioridad del hilo que lo lanza.

### 2. Ahorro de energía

Nueva clave `engine.powerSaving` (`auto` | `on` | `off`; solo en el nivel de perfil; por defecto `auto`; ADR-GRP-007). Con `auto`, el modo se activa cuando la máquina va con batería o con el modo de bajo consumo del SO. Con `on` está siempre activo y con `off` nunca.

| En modo ahorro | Se hace | No cambia nunca |
|---|---|---|
| Reconciliación periódica | Cada 15 min en lugar de 5 (⚠️ **ASSUMPTION**). BR-CONS-005 pasa a "como mucho, el intervalo de reconciliación vigente" | Snapshot previo (ADR-TMC-006) |
| Predictor | En pausa: los pares quedan "sin calcular", con su motivo visible (BR-CKP-CALC-001). Decisión de Rene Bonilla (2026-10-05) | Frescura de los eventos (NFR-04) |
| Mantenimiento del almacén | Se aplaza hasta volver a corriente, salvo que el tope de disco de la Time Machine esté superado (ADR-TMC-007, Enmienda 2026-10-05) | Guardrails y ejecutor de operaciones |

- **Detección por SO**: macOS con `IOPSCopyPowerSourcesInfo` y `NSProcessInfo.isLowPowerModeEnabled`; Linux con `/sys/class/power_supply/*/online` y `status` (sin D-Bus); Windows con `GetSystemPowerStatus` (`ACLineStatus` y `SystemStatusFlag`, ahorro de batería).
- **Si no se puede detectar** (escritorio sin batería, `sysfs` ilegible), el modo `auto` se comporta como "con corriente" y lo dice en el diagnóstico.
- **El cambio de fuente se aplica en ≤ 60 s** y se publica como estado del motor, para que la CLI y la TUI lo muestren.
- **El Arquitecto propuso** que el predictor siguiera con batería calculando solo los pares de prioridad 1, uno a uno, y que se pausara solo en el modo de bajo consumo. Se descarta por la decisión de Rene ("pausar el predictor"). Queda como alternativa si el dogfooding muestra que se echa de menos.

### 3. Presupuesto de huella como gate

La fila HUELLA se divide en los NFRs RES-01 a RES-05 de [non-functional.md](../non-functional.md) § "Consumo de recursos":

- **RES-01 y RES-02 son gate en INF-GRP-002**, con los objetivos actuales (CPU en reposo < 1 % y RSS < 150 MB con 10 worktrees y la Time Machine activa).
- **La cifra exacta del gate la fija la Dev Spec de INF-GRP-002** con su medición. Esa cifra **nunca supera el objetivo sin una enmienda de este ADR**.
- RES-03 (despertares), RES-04 (descriptores y vigilancias) y RES-05 (disco del perfil) se reportan hasta tener una línea base y después pasan a gate.

### 4. Visibilidad

- **El daemon expone el consumo** con un método de solo lectura `engine.resources`: CPU media y pico, RSS, descriptores y vigilancias, disco del perfil y de la Time Machine por repo, clase activa por pool y modo de ahorro de energía.
  - Es aditivo en el contrato de `crates/api` (versión menor).
  - No está en el perfil `mcp` (SEC-MCP-01).
  - Su dueña es **US-GRP-017** (`raptor status --resources`), que lo detalla en sus Requisitos Técnicos.
- **`raptor doctor`** (US-GRP-018) lee el disco del perfil y de la Time Machine **aunque el daemon no esté corriendo** y avisa si un valor supera su objetivo.

## Alternativas consideradas

| Alternativa | Por qué se descarta |
|---|---|
| Todo el daemon en segundo plano (launchd `ProcessType=Background`, `Nice=10` en systemd) | Penaliza el snapshot previo y el hook de Guardrails, que bloquean al usuario, y un proceso sin privilegios no puede recuperar la prioridad |
| Observador en `utility` | Ahorra un 0,04 % de CPU a cambio de arriesgar NFR-04, confirmado en `default`; también limita la E/S |
| Repartir la regla en cinco enmiendas (ADR-GRP-005, 010, CKP-001, TMC-004 y TMC-006) | La clase de trabajo es un concepto transversal: repartida, nadie es dueño de ella. Esos ADRs remiten a este |
| Detener el motor con batería | Rompe BR-CONS-005 (sin huecos mientras la máquina está encendida) y deja la Time Machine sin captura |

## Consecuencias

- ✅ El trabajo que el usuario espera (snapshot previo, Guardrails, operaciones) nunca compite con trabajo de fondo de GitRaptor.
- ✅ El consumo tiene un gate en CI y una vista para el usuario (US-GRP-017).
- ⚠️ **Con batería**, una pérdida silenciosa de eventos tarda hasta 15 min en reconciliarse, en lugar de 5. Se acepta por la decisión de Rene; el PO actualiza BR-CONS-005 en US-GRP-019.
- ⚠️ **`ionice` no tiene efecto con el planificador `none`** (NVMe). Se acepta: la clase de CPU sigue aplicando.
- ⚠️ **Borrar refs no libera disco hasta el `repack` y el `prune`**, con 1 h de gracia. La contabilidad del tope mide después del mantenimiento (ADR-TMC-007, Enmienda 2026-10-05).

## Validación

1. **RES-06**: dentro de cada trabajo, la clase es la esperada (`qos_class_self()`, `/proc/<tid>/stat` e `ionice -p`, `GetThreadPriority`), y un `git` hijo de un trabajo `background` sale con nice 10 (`ps -o ni`).
2. **RES-07** (bloquea el merge de TS-GRP-005): con todos los núcleos saturados por un proceso ajeno, el snapshot previo sigue por debajo de 200 ms p95, la espera del previo al escritor es ≤ 10 ms y el motor ≤ 300 ms p95.
3. **RES-08**: con una fuente de energía simulada (un trait inyectable, sin batería real), en 60 min de tiempo simulado hay 4 reconciliaciones periódicas en vez de 12, el predictor no calcula ningún par y NFR-04 sigue en verde.
4. **RES-01 y RES-02**: gate en INF-GRP-002.

Linux y Windows: **Pendiente: etapa de validación multiplataforma**. Este ADR se verifica en macOS.

## Referencias

- **NFRs**: RES-01 a RES-09 y HUELLA en [non-functional.md](../non-functional.md); NFR-04, BR-CONS-005; NFR-TMC-11 y SEC-TMC-12 en [time-machine/non-functional.md](../time-machine/non-functional.md).
- **ADRs**: ADR-GRP-005 § 3 (autoarranque), ADR-GRP-007 (clave `engine.powerSaving`), ADR-GRP-010 (reconciliación), ADR-GRP-011, ADR-CKP-001 (pool del predictor), ADR-TMC-004, ADR-TMC-006, ADR-TMC-007 (Enmienda 2026-10-05: tope de disco).
- **Historias**: TS-GRP-005 (clases de trabajo y mecanismo de ahorro), US-GRP-017 (`raptor status --resources`), US-GRP-018 (`raptor doctor`), US-GRP-019 (modo de ahorro de energía), US-TMC-022 (tope de disco), INF-GRP-002 (gate).

## Errata (2026-10-05, US-GRP-017)

**Decisión del orquestador (2026-10-05), validada por el Arquitecto**, al implementar la vista de § 4 ([Dev Spec de US-GRP-017](../../requirements/features/motor-local/dev-specs/US-GRP-017-dev-spec.md)):

- **Unidad de RES-02**: "150 MB" en § 3, en HUELLA y en RES-02 significa **150 MiB** (157 286 400 bytes), la cifra que ya aplica el gate de INF-GRP-002 (`FOOTPRINT_LIMITS`) y la vista (`gitraptor_api::resources::TARGETS`). Lo mismo para los 250 MiB de RES-05. La fila de [non-functional.md](../non-functional.md) se alinea cuando INF-GRP-002 (PR #76), que edita esas filas, esté en `main`.
- **Mismas definiciones, distinto instante**: la vista mide la CPU con la misma fórmula que el gate (tiempo de CPU del proceso entre tiempo de reloj, en % de un núcleo), pero sobre la ventana de 10 min de RES-01 y no sobre 30 s; el RSS y los descriptores son **instantáneos**, y el gate toma el pico en reposo. En macOS el gate cuenta descriptores con `lsof`, que incluye `cwd`, `txt` y las bibliotecas mapeadas; la vista cuenta solo descriptores numéricos (`/dev/fd`). Pendiente en INF-GRP-002: contar solo las filas con descriptor numérico para que gate y vista coincidan.
- **Windows**: los descriptores son handles del proceso y no tienen objetivo hasta tener línea base (se muestran sin evaluar).

## Enmienda (2026-10-07, observación por niveles)

Referencia cruzada de la [Enmienda (2026-10-07) de ADR-GRP-010](./ADR-GRP-010-observacion-cambios-worktrees.md#enmienda-2026-10-07-observación-por-niveles), que está propuesta y pendiente de aceptar por Rene Bonilla. **Decisión del orquestador (2026-10-07), validada por el Arquitecto.** No cambia las clases, el modo de ahorro ni los objetivos de RES-01 a RES-10: los extiende a muchos repos observados. El `status` sigue en `proposed`.

| Cambio | Dónde | Fuente |
|---|---|---|
| **Clases**: el barrido de dormidos, su reconciliación lenta y el descubrimiento en raíces corren en `utility`, como la reconciliación periódica. Despertar un repo (abrir el almacén y reconciliarlo) va en `default`, porque lo espera una sesión, un cliente o una edición que la Time Machine tiene que capturar | § 1 | ADR-GRP-010, N1 a N6 |
| **Por qué hacen falta los niveles**: con 100 repos activos, solo los almacenes abiertos (3 descriptores y hasta 2 MiB de caché cada uno) superarían RES-02 y RES-04, y el sondeo y la reconciliación periódica costarían alrededor del 0,6 % de un núcleo en reposo. Un repo dormido conserva sus vigilancias como centinela (Q49 del PO), pero no ese coste | § 3 | ADR-GRP-010, Discrepancia con la propuesta B |
| **Modo de ahorro**: barrido de dormidos cada 300 s en lugar de 120, y reconciliación lenta con un intervalo mínimo de 180 min en lugar de 60 (⚠️ **ASSUMPTION**) | § 2 | ADR-GRP-010, N2 y N3 |
| **Gate**: RES-11 (100 repos observados, 5 activos) con los mismos objetivos que RES-01, RES-02 y RES-04, y RES-12 (retraso de un dormido y tiempo de despertar), en el escenario `tiered-scale` de INF-GRP-002 | § 3 | [non-functional.md](../non-functional.md) § Consumo de recursos |
| **Visibilidad**: `engine.resources` añade el bloque opcional `observation` (repos y worktrees por nivel, vigilancias por nivel y CPU de las tareas de los dormidos y del descubrimiento), detrás de la capacidad `observation.tiers` (ADR-GRP-016). La dueña de la vista sigue siendo US-GRP-017, y el PO la enmienda | § 4 | ADR-GRP-010, N8 |
