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
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, ADR-GRP-010, ADR-GRP-013, INF-GRP-002, SPIKE-GRP-002, TS-GRP-004, CTX-GRP-001, US-GRP-002]
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

### 2. Reparto

⚠️ **ASSUMPTION**: las cifras son una hipótesis de diseño; SPIKE-GRP-002 da las primeras mediciones reales e INF-GRP-002 las convierte en gate. Si cambian, se actualiza esta tabla en este ADR.

| Etapa | Dueño | Presupuesto p95 | Marca de inicio → fin |
|---|---|---|---|
| Detección SO → motor | Motor (ADR-GRP-010) | ≤ 50 ms | `t0` (escritura, solo en el banco) → `t_recv` |
| Debounce | Motor | ventana fija de 75 ms | `t_recv` → `t_flush` |
| Recomputo incremental | Motor | ≤ 150 ms (cómputo y persistencia juntos) | `t_flush` → `t_computed` |
| Persistencia antes de publicar | Motor (ADR-GRP-013) | (incluida arriba) | `t_computed` → `t_persisted` |
| Publicación por IPC | Motor (ADR-GRP-005) | ≤ 25 ms | `t_persisted` → `t_published` (escrito en el canal) → `t_client_recv` |
| **Total motor** | **Motor** | **≤ 300 ms** | `t0` → `t_client_recv` |
| Recepción, aplicación del delta y render | Cockpit (F-001-02) | ≤ 100 ms | `t_client_recv` → `t_render` |
| Margen | Nadie | 100 ms | Absorbe ruido de la máquina y la varianza del SO |
| **Total NFR-04** | | **< 500 ms** | `t0` → `t_render` |

**Nombres canónicos de las marcas**, en orden y con el mismo nombre en este ADR, en el contrato (§ 3) y en el informe de INF-GRP-002: `t0`, `t_recv`, `t_flush`, `t_computed`, `t_persisted`, `t_published`, `t_client_recv` y `t_render`.

- **El margen no se reparte**: ninguna feature puede reclamarlo para cumplir su parte.
- **Publicación en dos fases**: si un cambio grande no cabe en 150 ms de cómputo, el motor publica dentro del presupuesto lo barato (rama, `HEAD`, operación en curso) con una marca de "recomputando", y los recuentos en un segundo evento (ADR-GRP-010). El p95 se mide sobre el primer evento que refleja el cambio; el segundo se mide aparte y se reporta, sin gate en el MVP.
- **Arranque y reconciliación** (inicio, vuelta de suspensión, desbordamiento) no cuentan para NFR-04: son estados explícitos ("reconciliando") que el Cockpit presenta como tales.
- **Modo degradado** (sondeo, ADR-GRP-010) queda fuera de NFR-04 y se expone como tal.

### 3. Instrumentación en el contrato

- Cada evento de cambio del stream de `crates/api` lleva un bloque de **tiempos por etapa**: `t_recv`, `t_flush`, `t_computed`, `t_persisted` y `t_published`, más un identificador de lote que agrupa los eventos de una misma ventana de debounce. Va siempre en el evento (unos pocos enteros) para que dogfooding y CI usen el mismo dato.
- **Reloj común entre procesos**: los tiempos se toman de un reloj monótono del sistema compartido por todos los procesos de la máquina (`CLOCK_MONOTONIC` en Linux, `mach_continuous_time` en macOS, `QueryPerformanceCounter` en Windows), serializado como nanosegundos. No se usa el reloj de pared, que puede saltar por NTP. Un helper de `crates/api` lo lee igual en el motor y en los clientes. El evento lleva aparte la hora de pared para mostrarla a personas.
- El Cockpit añade localmente `t_client_recv` y `t_render` (frame en el que el cambio ya es visible) y no los devuelve al motor.
- Los tiempos son de diagnóstico: no cambian el comportamiento ni la atribución, y no salen de la máquina (NFR-03).

### 4. Medición en CI (INF-GRP-002)

- **Banco**: repos temporales generados (uno de 100K commits o más, creado una vez y cacheado como artefacto de CI) con 10 worktrees. Escenarios: modificar un archivo, `git add`, commit, checkout de rama, crear y borrar un worktree, y una ráfaga de 1.000 archivos en un worktree mientras se mide en otro.
- **Cliente**: el banco escribe en `t0` y un suscriptor sin pantalla registra `t_client_recv`. Cuando exista el Cockpit (F-001-02), su TUI sin pantalla (backend de pruebas de `ratatui`) registra `t_render`. Hasta entonces se mide y se aplica el gate solo al motor.
- **Muestras**: ⚠️ **ASSUMPTION**: al menos 200 por escenario y SO, descartando las 10 primeras de calentamiento.
- **Gates**:
  - p95 del motor (`t0` → `t_client_recv`) > 300 ms en cualquier escenario y SO: **el CI falla**.
  - p95 de extremo a extremo (`t0` → `t_render`) ≥ 500 ms, cuando exista el Cockpit: **el CI falla**.
  - p95 de una etapa por encima de su presupuesto con el total dentro: **aviso** con la etapa nombrada, sin fallar, para no volver inestable el CI por el ruido de los runners compartidos.
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
- ⚠️ Persistir antes de publicar consume parte de los 150 ms; un `fsync` en macOS (`F_FULLFSYNC`) o en Windows puede costar decenas de milisegundos. **Mitigación**: ADR-GRP-006 § 4 agrupa las escrituras de un mismo lote de debounce en una sola transacción y SPIKE-GRP-002 mide el coste real.
- ⚠️ El p95 y las cifras de cada etapa son supuestos. **Mitigación**: SPIKE-GRP-002 los mide antes del desarrollo de US-GRP-002 e INF-GRP-002 los fija como gate; el cambio de interpretación (p95 frente a máximo) se confirma con Rene.
- ⚠️ Los runners compartidos del CI tienen ruido y pueden dar falsos fallos. **Mitigación**: gate sobre totales y aviso por etapa, calentamiento descartado y plan B de runner dedicado (apartado 4).
- ⚠️ La detección SO → motor no se puede medir fuera del banco, porque el SO no fecha los eventos en las tres plataformas. **Mitigación**: en dogfooding se mide desde `t_recv`; la detección solo se mide en el banco, donde se conoce `t0`.
- ⚠️ El bloque de tiempos amplía el contrato de `crates/api`. **Mitigación**: campos opcionales y versionados con el handshake de versión del canal (ADR-GRP-005); los clientes que no los usan los ignoran.

Nota de integración (Time Machine, ADR-TMC-004 y ADR-TMC-006, aceptados el 2026-10-03): la captura por observación de la Time Machine consume eventos ya publicados y no forma parte de este presupuesto. El gate del motor (p95 ≤ 300 ms) se ejecuta también con la Time Machine activa.

## Validación

- **SPIKE-GRP-002**: primera medición de las etapas del motor en los tres SO con 10 worktrees y un repo de 100K commits. Confirma o corrige las cifras del apartado 2 y la interpretación p95.
- **INF-GRP-002**: banco y gates del apartado 4. Pasa a bloquear el merge cuando US-GRP-002 está implementada.
- **Dogfooding**: percentiles reales del apartado 5 durante el uso diario de Rene; una desviación sostenida por encima del presupuesto abre la revisión de este ADR.

## Referencias

- Requerimiento: `docs/requirements/features/motor-local/context.md` (Dependencias con F-001-02, presupuesto compartido de NFR-04; Q1 y Q6).
- Reglas: BR-CONS-005.
- BRD: NFR-03, NFR-04, NFR-05.
- ADRs: ADR-GRP-001 (TUI con `ratatui`, motor con eventos incrementales), ADR-GRP-002 (`crates/api`), ADR-GRP-005 (canal y handshake), ADR-GRP-006 y ADR-GRP-013 (persistencia), ADR-GRP-009 (lectura), ADR-GRP-010 (detección, debounce y recomputo).
- Historias: US-GRP-002, TS-GRP-004 (canal y contrato), INF-GRP-002 (banco), SPIKE-GRP-002 (valida).
