---
id: TD-GRP-003
title: "Presupuesto de NFR-04 sin gate automático: medirlo en una máquina de referencia"
type: td
status: ready
feature: motor-local
domain: GRP
priority: high
complexity: medium
created: 2026-10-05
updated: 2026-10-05
related:
  adrs: [ADR-GRP-011, ADR-GRP-014]
  stories: [INF-GRP-002, INF-GRP-003, TD-GRP-002]
  specs: [DS-INF-GRP-002]
ado:
  id: null
  url: null
tags: [motor-local, deuda-tecnica, rendimiento, nfr-04, ci, banco, runner, gate, latencia]
---

## TD-GRP-003: Presupuesto de NFR-04 sin gate automático: medirlo en una máquina de referencia

**Valor**: el presupuesto real de frescura (p95 del motor ≤ 300 ms) vuelve a bloquear de forma automática, sin falsos positivos que enseñen a relanzar el CI sin mirar.

### Descripción

**Como** responsable del motor
**Quiero** que el p95 de 300 ms de NFR-04 se mida y bloquee en una máquina estable
**Para** que una regresión que respete los techos de los runners pero rompa el presupuesto no llegue a una release

> Dev Spec: N/A (el gate ya existe: `--gate reference` del banco de INF-GRP-002)

> **Decisión del orquestador (2026-10-05), validada por el Arquitecto y el PO** (con sus ajustes: TD nueva y no ampliación de TD-GRP-002; caducidad atada a un hito; recordatorio automático en los PR del motor; condiciones de medición en el informe).

**Origen (INF-GRP-002, Enmienda 2026-10-05: calibración del gate).** Los runners compartidos de GitHub no pueden medir un presupuesto absoluto de 300 ms sin falsos positivos. Lo muestran 20 corridas del banco:

- **macOS**: el p95 de las ráfagas va de 299 a 1.831 ms en el mismo código. El temporizador se despierta de 64 a 139 ms tarde, con dos modas.
- **Linux**: crear un worktree dio un p95 de 366,5 ms una vez, cuando lo habitual va de 157 a 294 ms.

Desde la enmienda, en CI (`--gate ci`) el presupuesto **solo se reporta**. Lo que bloquea es un gate de regresión con techos calibrados por runner, confirmado midiendo otra vez. Eso deja esta deuda deliberada:

- En CI, una regresión que quede por debajo de los techos de regresión pero rompa los 300 ms no bloquea. En Linux, los techos de p95 de los escenarios estables siguen por debajo de 300 ms; la excepción es crear un worktree (techo de p95 de unos 550 ms). En macOS, el p95 de las ráfagas no tiene gate.
- El gate absoluto (`--gate reference`) solo existe si alguien corre el banco en la máquina de referencia.

### Alcance Técnico

1. **Disparadores mientras tanto** (ajustes del PO, 2026-10-05):
   - **Antes de cada release**: correr `cargo bench -p gitraptor-cli --bench engine -- --gate reference` en el Mac de referencia sobre el commit del tag y adjuntar el informe. Es un paso de la verificación de [INF-GRP-003](./INF-GRP-003-pipeline-release.md). Sin informe en verde, la release no se publica. Si la máquina de referencia no está disponible, la release espera.
   - **En los PR que tocan el motor** (`crates/core/src/{watch,daemon,channel}`): el job `engine bench` lo recuerda con un aviso y una línea en el job summary, y pide el informe de `--gate reference` adjunto al PR antes del merge.
   - **El informe registra las condiciones** (commit, máquina, SO, núcleos, carga y alimentación), para que las mediciones de distintas releases se puedan comparar.
2. **Salida (plan B de ADR-GRP-011 § 4)**: un runner dedicado o autoalojado con la máquina de referencia, donde el job corra con `--gate reference` y bloquee. Sus techos de regresión no hacen falta: el presupuesto es el gate.
3. Cuando exista el runner dedicado, el gate de regresión de los runners compartidos se mantiene como señal temprana, y esta TD se cierra.

- **Fuera de alcance**: que el motor cumpla bajo ráfaga (TD-GRP-002) y la medición del Cockpit (`t_render`).

### Plan de Verificación

#### Pruebas Automatizadas

- Un job de CI en el runner dedicado ejecuta el banco con `--gate reference` y falla con el retardo artificial de la sonda de INF-GRP-002 (Dev Spec, Enmienda 2026-10-05).
- Diez corridas seguidas en verde en ese runner sobre `main`.

#### Verificación Manual / Sandbox

- Hasta cerrar esta TD: el informe de `--gate reference` del Mac de referencia está adjunto a cada release. Si falta, la release no se publica.
- **Caducidad**: antes de la **primera release pública (beta) del MVP** (PO, 2026-10-05). NFR-04 consta en `non-functional.md` como **verificado en la máquina de referencia y reportado, no bloqueante, en CI**.
