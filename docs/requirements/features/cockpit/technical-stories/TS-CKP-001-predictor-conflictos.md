---
id: TS-CKP-001
title: "Predictor de conflictos en el daemon: solape y conflicto previsto publicados para todos los clientes"
type: ts
status: draft
feature: cockpit
domain: GRP
priority: critical
complexity: high
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-CKP-001, ADR-GRP-005, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRP-013]
  stories: [US-CKP-006, US-CKP-007, US-CKP-008, US-CKP-009, US-CKP-011, SPIKE-CKP-001, TS-GRP-002, TS-GRP-003, TS-GRP-004, INF-GRP-001, INF-GRP-002, TS-GRD-001]
  specs: []
ado:
  id: null
  url: null
tags: [cockpit, prediccion-conflictos, merge-en-seco, solape, daemon, nfr-01, sec-09, sec-12, br-06, dep-ckp-1, f-001-05]
---

## TS-CKP-001: Predictor de conflictos en el daemon

**Valor**: la TUI, `raptor conflicts` y la herramienta `check_conflicts` del MCP leen el mismo ⚡ y el mismo ⚠, calculados una sola vez en el motor y sin dejar rastro en el repo del usuario.

### Descripción

**Como** Arquitecto
**Quiero** un predictor dentro del daemon que calcule por par el solape y el conflicto previsto con un merge en seco, y que publique cada resultado con su estado y su hora de cálculo
**Para** que BR-06 y el Servidor MCP consuman una única predicción con frescura acotada, sin escribir en el repo (ADR-CKP-001, ADR-GRP-009, NFR-01)

> Dev Spec: `dev-specs/TS-CKP-001-predictor-conflictos.md` | Pendiente
>
> **Depende de**: SPIKE-CKP-001 (mecanismo y cifras; bloquea la Dev Spec), TS-GRP-002 (capa de lectura donde vive el merge en seco), TS-GRP-003 (daemon que aloja el pool) y TS-GRP-004 (instantánea y stream; la forma del estado y del evento de predicción es **pendiente, dueño: worker del canal (TS-GRP-004)**). Los pares contra la base usan la rama base confirmada (TS-GRD-001, US-GRD-014, US-GRP-016); sin ella se publican "pendiente". **ADRs**: ADR-CKP-001 § 1 a § 8 y § 10, que la Dev Spec sigue punto por punto; ADR-GRP-010 § 4 (rutas sin commitear en memoria); ADR-GRP-011 (presupuesto del motor que la predicción no consume). **Seguridad**: SEC-09, SEC-12, NFR-03. **Habilita**: BR-06 (BR-CKP-CALC-002, CALC-003, WF-005, WF-007, EDGE-001), `raptor conflicts` (Q-CKP-20) y `check_conflicts` de F-001-05.

### Alcance Técnico

- **Implementar** en la capa de lectura de Git el merge en seco de dos commits con el mecanismo que confirme SPIKE-CKP-001, devolviendo archivos en conflicto con su tipo y hunks como rangos de líneas.
- **Cerrar** en ese merge toda ejecución de programas del usuario: sin drivers de merge, sin filtros y con la pila de atributos vacía (ADR-CKP-001 § 1, L-04).
- **Leer** en ese merge los mismos objetos que verá el ejecutor, sin objetos de reemplazo ni grafts, y sin descargas en un *partial clone* (L-04, M-02).
- **Implementar** en la misma capa el cálculo de rutas cambiadas entre dos commits, que alimenta el solape y el prefiltro.
- **Crear** en el motor el módulo de predicción con los pares worktree–base y worktree–worktree con trabajo propio (ADR-CKP-001 § 2).
- **Calcular** los dos niveles: solape con lo sin commitear y conflicto previsto solo con lo commiteado (ADR-CKP-001 § 3).
- **Consumir** en memoria el conjunto de rutas sin commitear que el motor ya calcula por worktree, sin persistirlo.
- **Implementar** el recálculo incremental por par, con caché por ids de commit y el prefiltro si SPIKE-CKP-001 lo mantiene (ADR-CKP-001 § 4).
- **Implementar** la cola coalescente por par, con prioridad y un pool propio de prioridad baja separado del observador y del recomputo del motor (ADR-CKP-001 § 5).
- **Aplicar** los topes por par de tiempo, memoria, rutas, archivos y hunks, con el merge interrumpible, el tamaño de cada blob leído antes del merge y un límite de renombrados propio, y resultado "no calculable" o "truncado" al superarlos (M-06).
- **Mover** el merge en seco a un proceso trabajador con límites de CPU y memoria solo si SPIKE-CKP-001 no demuestra esas cotas dentro del daemon (ADR-CKP-001 § 1).
- **Publicar** cada par con su estado, su motivo, su hora de cálculo y los límites declarados como datos (ADR-CKP-001 § 6 a § 8).
- **Marcar** rutas y nombres de rama como texto no confiable y limitar toda respuesta a archivos y rangos, sin contenido.
- **Registrar** en la comprobación estática de CI que el módulo de predicción no importa ninguna capa de escritura.
- **Añadir** a INF-GRP-001 la suite "predicción" (10 worktrees, 55 pares, repo canario, *partial clone* sin red) como gate de esta historia.
- **Añadir** a INF-GRP-002 el escenario "commit hasta predicción publicada" y la medición del p95 del motor durante una ráfaga de predicciones.
- **Crear** el corpus fijo de fidelidad frente a un merge real de Git en clones temporales, que se repite al actualizar gitoxide.
- **Fuera de alcance**: el registro del KPI de detección (Q-CKP-21), que tiene resultado observable y va a una historia; la presentación del ⚡, ConflictAlert y el toast (historias de BR-06); la forma del contrato (TS-GRP-004); la publicación del estado en conflicto real (DEP-CKP-14, implementación en motor-local); las herramientas MCP (F-001-05).

### Plan de Verificación

#### Pruebas Automatizadas

- **Repo intacto (suite de INF-GRP-001)**: con 10 worktrees y los 55 pares calculados, y tras recálculos por commit y por movimiento de la base, cero diferencias en la huella, incluido el mtime de packs, objetos sueltos y directorios de objetos. Gate de esta historia.
- **Cero ejecución (SEC-09)**: repo canario con drivers de merge, procesos de filtro, atributos con `merge=` y fsmonitor apuntando a un script marcador; el marcador nunca aparece. Con la opción (a), la auditoría de `exec` no ve procesos hijo del predictor.
- **Sin red (M-02)**: un *partial clone* con objetos ausentes y un remoto *promisor* da "no calculable (objeto ausente)" con 0 conexiones y 0 descargas.
- **Mismos objetos (L-04)**: con un `refs/replace` que cambia un commit del par, el resultado es el del objeto original; un atributo `merge=` del repo no ejecuta nada.
- **Fidelidad**: paridad de archivos en conflicto con el merge real por encima del umbral que fije SPIKE-CKP-001, y 0 falsos negativos en el escenario de la demo del BRD § 13 (Q-CKP-25). El prefiltro no introduce falsos negativos en el corpus.
- **Frescura (INF-GRP-002)**: del fin del commit a la predicción publicada, ≤ 5 s p95 con 10 worktrees y 100K commits. Aviso hasta que SPIKE-CKP-001 confirme la cifra (S-CKP-1) y gate después. El p95 del motor (≤ 300 ms, ADR-GRP-011) no empeora durante la ráfaga.
- **Estados**: pruebas de `calculando`, `recalculando`, `pendiente` (base no confirmada, operación en curso) y `no calculable` (base inexistente, límite excedido). Un resultado calculado con entradas viejas nunca se publica como `actual`. Con la base no confirmada, los pares entre worktrees siguen publicándose.
- **Incremental**: un commit en un worktree recalcula solo sus pares; sin cambios en las tres ids de commit, el par no se recalcula.
- **Topes (M-06)**: un repo hostil (archivo de 100 MB, 10K conflictos, árbol muy profundo, miles de renombrados) da "no calculable" o "truncado" dentro del tiempo máximo por par, y la memoria del daemon queda por debajo de la cota que fije SPIKE-CKP-001.
- **Salida (SEC-12)**: una ruta con secuencias de control sale marcada como no confiable; ninguna respuesta lleva contenido de archivos.
- **Frontera**: la comprobación estática falla si el módulo de predicción importa una capa de escritura.

#### Verificación Manual / Sandbox

- Reproducir la demo del BRD § 13 en un repo temporal con 4 sesiones, dos de ellas tocando el mismo archivo: aparece ⚡ con archivo y hunk antes del merge, y el merge real produce el conflicto previsto.
- Linux y Windows (prioridad baja del SO, auditoría de `exec`): **Pendiente: etapa de validación multiplataforma**.
