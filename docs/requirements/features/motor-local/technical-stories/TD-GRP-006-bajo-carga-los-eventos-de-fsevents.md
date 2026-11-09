---
id: TD-GRP-006
title: "Bajo carga, los eventos de FSEvents del directorio Git llegan tarde o no llegan y us_grp_010 falla también en main"
type: td
status: draft
feature: motor-local
domain: GRP
priority: medium
complexity: medium
created: 2026-11-09
updated: 2026-11-09
source: inline
related:
  adrs: [ADR-GRP-010, ADR-GRP-012]
  stories: [TD-GRP-005, US-GRP-010]
ado:
  id: null
  url: null
tags: [motor-local, deuda-tecnica, observador, fsevents, carga, tests-inestables]
---

## TD-GRP-006: Bajo carga, los eventos de FSEvents del directorio Git llegan tarde o no llegan y us_grp_010 falla también en main

**Valor**: la Time Machine registra a tiempo los commits aunque la máquina esté cargada, y `us_grp_010` deja de fallar por azar en el CI y en local.

### Descripción

**Como** desarrollador que trabaja con agentes en una máquina cargada
**Quiero** que el motor vea a tiempo las escrituras en el directorio Git aunque el observador del SO se retrase o pierda eventos
**Para** que la Time Machine muestre la atribución actual sin depender de la suerte

**Origen: PR de TD-GRP-005 (rama `fix/detect-sample-window`), 2026-11-09.** Medido en la máquina de dogfooding (macOS, carga 34–40).

**Medición**:

- `crates/core/tests/us_grp_010.rs::the_time_machine_sees_the_current_attribution` falla 2 de 10, también con el código de `main`.
- En la corrida que falla no llega ningún evento FS del directorio `.git` (ningún `git_dir_touched` del router). Solo un sondeo (`poll_repo` de la Time Machine o el sondeo de respaldo) ve moverse el índice, y el commit no aparece antes del plazo del test.
- En otras corridas los eventos del directorio Git llegaron entre 530 ms y 1,4 s después de la escritura.

**Diagnóstico** (Arquitecto, 2026-11-09): no es una carrera de S3, es latencia o pérdida de FSEvents bajo carga (observador, ADR-GRP-010). TD-GRP-005 ya cubre lo que dependía de S3: el aviso de escritura de la lectura del repo con tope de 3 s y la barrera de muestreo.

### Alcance Técnico

- **Medir** la latencia y las pérdidas de los eventos del directorio Git bajo carga, con un contador o una traza entera en el router (SEC-04: sin rutas).
- **Decidir** con la medición si cambia el sondeo de respaldo, la espera del test o ambos.
- **Estabilizar** `us_grp_010` por señal, sin `sleep` fijos.
- **Fuera de alcance**: la regla S3 (ADR-GRP-012) y lo ya cerrado en TD-GRP-005.
- **Linux/Windows**: pendiente de la etapa de validación multiplataforma.

### Plan de Verificación

- **Integración bajo carga**: `us_grp_010` pasa 10 de 10 seguidas con la suite de otro crate corriendo en paralelo.
- **Traza**: la traza del router muestra la latencia p50/p99 de los eventos del directorio Git y cuántos faltaron, sin rutas (SEC-04).
- **No regresión**: TD-GRP-005 y US-GRP-010 siguen en verde.
