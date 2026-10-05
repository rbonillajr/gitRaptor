---
id: ADR-GRP-011
title: Reparto del presupuesto de frescura (NFR-04) entre el motor y el Cockpit
type: adr
status: accepted
accepted: 2026-10-04
date: 2026-10-03
created: 2026-10-03
updated: 2026-10-04
deciders: [Rene Bonilla]
domain: GRP
feature: motor-local
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, ADR-GRP-009, ADR-GRP-010, ADR-GRP-013, ADR-CKP-001, ADR-CKP-003, SPIKE-CKP-001, INF-GRP-002, SPIKE-GRP-002, TS-GRP-004, CTX-GRP-001, US-GRP-002]
tags: [rendimiento, latencia, presupuesto, p95, nfr-04, timestamps, instrumentacion, ci, dogfooding, cockpit]
---

# ADR-GRP-011 — Reparto del presupuesto de frescura (NFR-04)

> **Estado**: aceptado por Rene Bonilla el 2026-10-04.

## Contexto

NFR-04 exige que la TUI refleje un cambio en **menos de 500 ms**. Ese tiempo lo comparten dos features (context, Dependencias): el motor local, que detecta y publica el cambio, y el Cockpit (F-001-02), que lo recibe y lo pinta. Sin un reparto, cada feature puede creer que tiene los 500 ms enteros y, cuando el total falla, nadie sabe qué etapa se pasó.

El camino de un cambio tiene etapas de naturaleza muy distinta:

1. El SO notifica al motor (FSEvents, inotify o ReadDirectoryChangesW, ADR-GRP-010).
2. El motor agrupa eventos en una ventana de debounce.
3. El motor recomputa el estado con la capa de solo lectura (ADR-GRP-009).
4. El motor persiste el resultado en el perfil antes de publicarlo, para que ningún evento que vio un cliente se pierda al reiniciar (BR-CONS-005, ADR-GRP-006, ADR-GRP-013).
5. El motor publica por el canal local (ADR-GRP-005, contrato en `crates/api`).
6. El Cockpit recibe, aplica el delta y pinta.

NFR-04 no dice si "< 500 ms" es un máximo absoluto, una media o un percentil, ni en qué máquina. NFR-05 añade la condición de escala: 10 o más worktrees y repos de más de 100K commits sin degradarse.

## Decisión

Recomendación aceptada por Rene Bonilla el 2026-10-03 (índice de ADRs, opción 1): **presupuesto fijo por etapa con instrumentación en el evento**.

### 1. Interpretación de NFR-04

⚠️ **ASSUMPTION** (a confirmar con SPIKE-GRP-002 e INF-GRP-002): "< 500 ms" se mide como **p95** de extremo a extremo, desde que termina la escritura en el sistema de archivos hasta que el Cockpit pinta el frame que la refleja, en las máquinas de referencia (apartado 4), con 10 worktrees observados. Una ráfaga se mide dos veces: su primer cambio visible y su estado final, ambos desde la escritura correspondiente (primera y última).

**Confirmado en macOS por SPIKE-GRP-002 (Enmienda 2026-10-04)**: en 1.800 muestras, el máximo del motor fue de 175 ms y el p99 más alto, de 140,7 ms, así que p95, p99 y máximo dan el mismo veredicto. El gate sigue siendo el **p95**, por el ruido de los runners compartidos, y el banco reporta también el **p99 y el máximo**. En Linux y Windows la interpretación sigue como supuesto hasta su medición.

### 2. Reparto

⚠️ **ASSUMPTION**: las cifras son una hipótesis de diseño; SPIKE-GRP-002 da las primeras mediciones reales e INF-GRP-002 las convierte en gate. Si cambian, se actualiza esta tabla en este ADR.

| Etapa | Dueño | Presupuesto p95 | Marca de inicio → fin |
|---|---|---|---|
| Detección SO → motor | Motor (ADR-GRP-010) | ≤ 50 ms | `t0` (escritura, solo en el banco) → `t_recv` |
| Debounce | Motor | ventana fija **efectiva** de 75 ms, con la holgura del temporizador descontada (ADR-GRP-010 § 3) | `t_recv` → `t_flush` |
| Recomputo incremental | Motor | ≤ 150 ms (cómputo y persistencia juntos) | `t_flush` → `t_computed` |
| Persistencia antes de publicar | Motor (ADR-GRP-013) | (incluida arriba) | `t_computed` → `t_persisted` |
| Publicación por IPC | Motor (ADR-GRP-005) | ≤ 25 ms | `t_persisted` → `t_published` (escrito en el canal) → `t_client_recv` |
| **Total motor** | **Motor** | **≤ 300 ms** | `t0` → `t_client_recv` |
| Recepción, aplicación del delta y render | Cockpit (F-001-02) | ≤ 100 ms | `t_client_recv` → `t_render` |
| Margen | Nadie | 100 ms | Absorbe ruido de la máquina y la varianza del SO |
| **Total NFR-04** | | **< 500 ms** | `t0` → `t_render` |

**Nombres canónicos de las marcas**, en orden y con el mismo nombre en este ADR, en el contrato (§ 3) y en el informe de INF-GRP-002: `t0`, `t_recv`, `t_flush`, `t_computed`, `t_persisted`, `t_published`, `t_client_recv` y `t_render`.

- **El margen no se reparte**: ninguna feature puede reclamarlo para cumplir su parte.
- **Publicación en dos fases**: si un cambio grande no cabe en 150 ms de cómputo, o si cambió la punta de la rama o la base y hay que recalcular ahead/behind (ADR-GRP-010 § 4, Enmienda 2026-10-04), el motor publica dentro del presupuesto lo barato (rama, `HEAD`, operación en curso) con una marca de "recomputando", y los recuentos en un segundo evento (ADR-GRP-010). El p95 se mide sobre el primer evento que refleja el cambio; el segundo se mide aparte y se reporta, sin gate en el MVP.
- **Arranque y reconciliación** (inicio, vuelta de suspensión, desbordamiento, recreación del stream del watcher, reconciliación periódica) no cuentan para NFR-04: son estados explícitos ("reconciliando") que el Cockpit presenta como tales. La publicación del alta o la baja del propio worktree sí cuenta: es el escenario "crear y borrar un worktree" del apartado 4.
- **Modo degradado** (sondeo, ADR-GRP-010) queda fuera de NFR-04 y se expone como tal.
- **Predicción de conflictos y consultas bajo demanda** (Enmienda 2026-10-04, Cockpit): fuera de NFR-04, con objetivo propio; ver la sección final.

### 3. Instrumentación en el contrato

- Cada evento de cambio del stream de `crates/api` lleva un bloque de **tiempos por etapa**: `t_recv`, `t_flush`, `t_computed`, `t_persisted` y `t_published`, más un identificador de lote que agrupa los eventos de una misma ventana de debounce. Va siempre en el evento (unos pocos enteros) para que dogfooding y CI usen el mismo dato.
- **Reloj común entre procesos**: los tiempos se toman de un reloj monótono del sistema compartido por todos los procesos de la máquina (`CLOCK_MONOTONIC` en Linux, `mach_continuous_time` en macOS, `QueryPerformanceCounter` en Windows), serializado como nanosegundos. No se usa el reloj de pared, que puede saltar por NTP. Un helper de `crates/api` lo lee igual en el motor y en los clientes. El evento lleva aparte la hora de pared para mostrarla a personas.
- El Cockpit añade localmente `t_client_recv` y `t_render` (frame en el que el cambio ya es visible) y no los devuelve al motor. (Enmienda 2026-10-04, Cockpit: definición exacta de las dos marcas en la sección final.)
- Los tiempos son de diagnóstico: no cambian el comportamiento ni la atribución, y no salen de la máquina (NFR-03).

### 4. Medición en CI (INF-GRP-002)

- **Banco**: repos temporales generados (uno de 100K commits o más, creado una vez y cacheado como artefacto de CI) con 10 worktrees. Escenarios: modificar un archivo, `git add`, commit, checkout de rama, crear y borrar un worktree, y una ráfaga de 1.000 archivos en un worktree mientras se mide en otro.
- **Cliente**: el banco escribe en `t0` y un suscriptor sin pantalla registra `t_client_recv`. Cuando exista el Cockpit (F-001-02), su TUI sin pantalla (backend de pruebas de `ratatui`) registra `t_render`. Hasta entonces se mide y se aplica el gate solo al motor.
- **`t0` en los escenarios de Git** (Enmienda 2026-10-04): `t0` es el fin del comando. Git escribe `index`, refs y `HEAD` antes de terminar, así que el lote suele abrirse antes de `t0` (SPIKE-GRP-002: en 200 de 200 commits y checkouts). La etapa de detección solo se aísla en "modificar un archivo"; en los demás escenarios se reporta el total desde el fin del comando.
- **Debounce medido como duración efectiva** (`t_recv` → `t_flush`), incluida la holgura del temporizador del SO, no como el valor configurado.
- **Calibración de la holgura** (Enmienda 2026-10-04): el banco mide la holgura del temporizador por SO y, con esos datos, la Dev Spec de INF-GRP-002 decide entre una constante por SO y la calibración en tiempo de ejecución (ADR-GRP-010 § 3).
- **Escenarios sin gate de latencia** (Enmienda 2026-10-04): la recreación del stream con escrituras concurrentes y la pérdida silenciosa recuperada por la reconciliación periódica tienen gate de **corrección** (100% recuperado y marcado como hueco), que bloquea el merge, pero no de latencia.
- **Muestras**: ⚠️ **ASSUMPTION**: al menos 200 por escenario y SO, descartando las 10 primeras de calentamiento.
- **Gates**:
  - p95 del motor (`t0` → `t_client_recv`) > 300 ms en cualquier escenario y SO: **el CI falla**.
  - p95 de extremo a extremo (`t0` → `t_render`) ≥ 500 ms, cuando exista el Cockpit: **el CI falla**.
  - p95 de una etapa por encima de su presupuesto con el total dentro: **aviso** con la etapa nombrada, sin fallar, para no volver inestable el CI por el ruido de los runners compartidos.
  - Gate del Cockpit y escenario de la predicción (Enmienda 2026-10-04, Cockpit): ver la sección final.
- **Máquinas de referencia**: runners estándar de Windows, macOS y Linux del CI (ADR-GRP-001, base de CI) más la máquina de dogfooding de Rene (macOS). ⚠️ **ASSUMPTION**: si el ruido de los runners compartidos hace inestable el gate, se mueve a un runner dedicado o se pasa a gate sobre la mediana con el p95 como aviso, y se registra en este ADR.

### 5. Medición en dogfooding

- El motor acumula en memoria un histograma por etapa con los tiempos de cada evento y lo guarda en el perfil a intervalos, en local y sin telemetría (NFR-03).
- El canal expone esos percentiles como dato de diagnóstico; presentarlos (p. ej. un comando de la CLI o un panel del Cockpit) es de la superficie correspondiente.
- La revisión de dogfooding de SPIKE-GRP-002 compara los percentiles reales con la tabla del apartado 2.

## Alternativas consideradas

- **Solo medición de extremo a extremo, sin reparto**: es lo que dice NFR-04 literalmente, pero cuando falla no indica qué etapa ni qué feature se pasó, y cada feature optimiza a ciegas. Se descarta.
- **Reparto 50/50 sin medición por etapa**: da 250 ms a cada lado sin justificación (el render de una TUI necesita mucho menos que el motor) y sigue sin diagnosticar. Se descarta.
- **Presupuesto fijo por etapa con instrumentación en el evento (elegida)**: cada feature conoce su límite, el CI dice qué etapa se pasó y el mismo dato sirve en dogfooding.

## Consecuencias

- ✅ Cada feature tiene un límite propio y verificable: el motor 300 ms y el Cockpit 100 ms.
- ✅ Un fallo de NFR-04 señala la etapa concreta, con el mismo dato en CI y en dogfooding.
- ✅ El margen de 100 ms protege contra la varianza del SO y de máquinas más lentas que las de referencia.
- ⚠️ Persistir antes de publicar consume parte de los 150 ms. En macOS, SPIKE-GRP-002 midió una transacción SQLite por lote con `F_FULLFSYNC` en **≤ 11 ms p95** (de 1 a 1.000 filas), y de 9 a 15 ms p95 dentro del motor con 10 worktrees compartiendo la conexión. Las "decenas de milisegundos" supuestas no se observaron. En Windows sigue sin medir. **Mitigación**: ADR-GRP-006 § 4 agrupa las escrituras de un mismo lote de debounce en una sola transacción.
- ⚠️ El temporizador de macOS se despierta hasta 10 ms tarde (SPIKE-GRP-002: 85 ms p95 con una ventana programada de 75 ms). **Mitigación**: la ventana se programa con la holgura descontada para que su duración efectiva sea de 75 ms (ADR-GRP-010 § 3), y el banco mide la duración efectiva.
- ⚠️ El p95 y las cifras de cada etapa siguen siendo supuestos en Linux y Windows. En macOS, SPIKE-GRP-002 los confirmó con el debounce compensado. **Mitigación**: SPIKE-GRP-002 los mide antes del desarrollo de US-GRP-002 e INF-GRP-002 los fija como gate; el cambio de interpretación (p95 frente a máximo) se confirma con Rene.
- ⚠️ Los runners compartidos del CI tienen ruido y pueden dar falsos fallos. **Mitigación**: gate sobre totales y aviso por etapa, calentamiento descartado y plan B de runner dedicado (apartado 4).
- ⚠️ La detección SO → motor no se puede medir fuera del banco, porque el SO no fecha los eventos en las tres plataformas. **Mitigación**: en dogfooding se mide desde `t_recv`; la detección solo se mide en el banco, donde se conoce `t0`.
- ⚠️ El bloque de tiempos amplía el contrato de `crates/api`. **Mitigación**: campos opcionales y versionados con el handshake de versión del canal (ADR-GRP-005); los clientes que no los usan los ignoran.

Nota de integración (Time Machine, ADR-TMC-004 y ADR-TMC-006, aceptados el 2026-10-03): la captura por observación de la Time Machine consume eventos ya publicados y no forma parte de este presupuesto. El gate del motor (p95 ≤ 300 ms) se ejecuta también con la Time Machine activa.

## Validación

- **SPIKE-GRP-002**: primera medición de las etapas del motor con 10 worktrees y un repo de 100K commits. **Medido solo en macOS** ([resultados](../../requirements/features/motor-local/research/SPIKE-GRP-002-resultados.md), § 3.1 a § 3.3): confirma las cifras del apartado 2, con el debounce compensado, y la interpretación p95. **Linux y Windows siguen pendientes**: se miden con el procedimiento del [README del prototipo](../../../spikes/watcher-viability/README.md#reproducir-en-linux-y-windows), que confirma o corrige las cifras en esos SO.
- **INF-GRP-002**: banco y gates del apartado 4. Pasa a bloquear el merge cuando US-GRP-002 está implementada.
- **Dogfooding**: percentiles reales del apartado 5 durante el uso diario de Rene; una desviación sostenida por encima del presupuesto abre la revisión de este ADR.

## Referencias

- Requerimiento: `docs/requirements/features/motor-local/context.md` (Dependencias con F-001-02, presupuesto compartido de NFR-04; Q1 y Q6).
- Reglas: BR-CONS-005.
- BRD: NFR-03, NFR-04, NFR-05.
- ADRs: ADR-GRP-001 (TUI con `ratatui`, motor con eventos incrementales), ADR-GRP-002 (`crates/api`), ADR-GRP-005 (canal y handshake), ADR-GRP-006 y ADR-GRP-013 (persistencia), ADR-GRP-009 (lectura), ADR-GRP-010 (detección, debounce y recomputo).
- Historias: US-GRP-002, TS-GRP-004 (canal y contrato), INF-GRP-002 (banco), SPIKE-GRP-002 (valida).

## Enmienda (2026-10-04, SPIKE-GRP-002)

Aplicada desde las recomendaciones de [SPIKE-GRP-002-resultados.md](../../requirements/features/motor-local/research/SPIKE-GRP-002-resultados.md) (§ 6), que se midieron **solo en macOS**. Las cifras del reparto (§ 2) no cambian. El `status` sigue en `accepted`: la enmienda no cambia la decisión aceptada por Rene Bonilla.

| Cambio | Dónde | Fuente |
|---|---|---|
| Interpretación p95 confirmada en macOS; el banco reporta además p99 y máximo; Linux y Windows siguen como supuesto | § 1 | Resultados § 3.3 |
| La fila de debounce pasa a ventana **efectiva** de 75 ms, con la holgura del temporizador descontada (medida: 85 ms p95 sin compensar). Se elige compensar en lugar de presupuestar 85 ms, para que las etapas sigan sumando 300 ms | § 2 (tabla), Consecuencias | Resultados § 3.2 |
| El banco toma `t0` al fin del comando en los escenarios de Git, aísla la detección solo en "modificar un archivo" y mide el debounce como duración efectiva | § 4 | Resultados § 2, § 3.1 |
| Coste medido de la persistencia con `F_FULLFSYNC` en macOS (≤ 11 ms p95); Windows sigue sin medir | Consecuencias | Resultados § 3.11 |
| Revisión de coherencia con la Enmienda de ADR-GRP-010: ahead/behind recalculado activa la segunda fase; la reconciliación tras recrear el stream no cuenta para NFR-04, pero el alta y la baja del propio worktree sí; las cifras siguen como supuesto solo en Linux y Windows | § 2, Consecuencias | ADR-GRP-010 § 4 y § 6; revisión del Arquitecto (2026-10-04) |
| Validación alineada con el alcance parcial: SPIKE-GRP-002 medido solo en macOS; Linux y Windows pendientes con el procedimiento del README del prototipo | Validación | Resultados § 5 y § 7; Artifact Judge (reservas) |
| La reconciliación periódica de ADR-GRP-010 § 5 tampoco cuenta para NFR-04; el escenario de recreación del stream del banco tiene gate de corrección, no de latencia | § 2 | ADR-GRP-010, Enmienda (2026-10-04, SPIKE-GRP-002); decisión del orquestador, validada por el Arquitecto |
| El banco calibra la holgura del temporizador por SO y decide con datos entre constante y calibración en ejecución; los escenarios de recreación del stream y de reconciliación periódica llevan gate de corrección | § 4 | Resultados § 3.2 y § 3.6; revisión del Arquitecto (2026-10-04) |

## Enmienda (2026-10-04, Cockpit)

Aplicada desde la enmienda E2 de [ADR-CKP-003](./ADR-CKP-003-arquitectura-tui.md) (§ 6, Validación V3) y desde [ADR-CKP-001](./ADR-CKP-001-prediccion-conflictos-merge-en-seco.md) § 5 y Validación 5, los dos `accepted` el 2026-10-04, con Q-CKP-6 y S-CKP-1 de [CTX-CKP-001](../../requirements/features/cockpit/context.md). **Decisión del orquestador (2026-10-04), validada por Arquitecto**; el PO valida el alcance después. **Las cifras del reparto (§ 2) no cambian**: el Cockpit sigue con ≤ 100 ms p95. El `status` sigue en `accepted`.

| Cambio | Dónde | Fuente |
|---|---|---|
| Definición exacta de `t_client_recv` y `t_render` | § 3 | ADR-CKP-003 § 6 (E2) |
| Gate nuevo: p95 del Cockpit (`t_client_recv` → `t_render`) > 100 ms, **el CI falla**, simétrico al del motor; aviso de feedback por tecla | § 4 | ADR-CKP-003 § 6, V3 (E2) |
| El cliente sin pantalla del banco es la `App` de `apps/cli` sobre el backend de pruebas de `ratatui` | § 4 | ADR-CKP-003 § 6 y § 12 |
| La predicción de conflictos queda fuera de NFR-04, con objetivo propio de ≤ 5 s p95 (supuesto); escenario nuevo en INF-GRP-002 | § 2, § 4 | ADR-CKP-001 § 5; Q-CKP-6; S-CKP-1 |
| Las consultas bajo demanda (grafo, diff) quedan fuera de NFR-04 | § 2 | DEP-CKP-2, DEP-CKP-3; ADR-GRP-005 (Enmienda, Cockpit) |

**Marcas del Cockpit** (§ 3):

- **`t_client_recv`**: lo toma el hilo del canal de la TUI al terminar de leer del transporte el mensaje completo, **antes** de deserializarlo. Decodificar cuenta en el presupuesto del Cockpit.
- **`t_render`**: lo toma el hilo principal al volver la llamada de dibujo del **primer frame dibujado después de aplicar** el mensaje, con el buffer ya volcado al backend.
- Ambas con el helper de reloj monótono de `crates/api`. Nunca vuelven al motor ni salen de la máquina (NFR-03).

**Gates** (§ 4), que se suman a los existentes:

- p95 del Cockpit (`t_client_recv` → `t_render`) > 100 ms en cualquier escenario y SO: **el CI falla**.
- Feedback por tecla (tecla leída → frame) p95 ≥ 100 ms: **aviso** (patrón de ADR-GRP-004 § 3).
- El gate de extremo a extremo (`t0` → `t_render` ≥ 500 ms) entra en vigor con la `App` del Cockpit en el banco.
- El banco no mide el volcado a una terminal real; el histograma local de la TUI lo mide en dogfooding (ADR-CKP-003 § 6). Una variante del banco sobre una pseudo-terminal queda para la Dev Spec de INF-GRP-002.

**Predicción de conflictos** (ADR-CKP-001 § 5):

- **Fuera de NFR-04**: corre en un pool propio del daemon y nunca consume el presupuesto del motor (≤ 300 ms p95). La TUI muestra su antigüedad y su estado (`calculando`, `recalculando`).
- **Objetivo propio**: ⚠️ **ASSUMPTION** (S-CKP-1): del fin del commit a la predicción publicada, ≤ 5 s p95 con 10 worktrees y un repo de 100K commits. Lo mide SPIKE-CKP-001.
- **Escenario nuevo en INF-GRP-002**: el objetivo es **aviso** hasta que SPIKE-CKP-001 confirme la cifra, y gate después. Además, el p95 del motor no puede empeorar durante una ráfaga de predicciones; si empeora, falla el gate del motor que ya existe.

**Consultas bajo demanda** (grafo y diff): son respuestas a una petición, no eventos del stream, así que quedan fuera de NFR-04. La TUI muestra su estado pendiente en < 100 ms (feedback por tecla) y su latencia se reporta sin gate en el MVP.

Linux y Windows: **Pendiente: etapa de validación multiplataforma**.
