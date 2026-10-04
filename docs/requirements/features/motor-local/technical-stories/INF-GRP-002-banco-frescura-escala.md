---
id: INF-GRP-002
title: "Banco de medición de frescura y escala"
type: inf
status: Dev Spec Pending
feature: motor-local
domain: GRP
priority: high
complexity: medium
created: 2026-10-03
updated: 2026-10-03
related:
  adrs: [ADR-GRP-011, ADR-GRP-010, ADR-GRP-005, ADR-GRP-006, ADR-GRP-013]
  stories: [US-GRP-001, US-GRP-002, US-GRP-012, TS-GRP-004]
  specs: []
ado:
  id: null
  url: null
tags: [motor-local, ci, rendimiento, latencia, p95, escala, nfr-04, nfr-05]
---

## INF-GRP-002: Banco de medición de frescura y escala

**Valor**: una regresión de latencia o de escala del motor rompe el CI y nombra la etapa que se pasó.

### Descripción

**Como** Arquitecto
**Quiero** un banco reproducible de latencia por etapa y de escala, convertido en gate de CI
**Para** que el motor cumpla sus 300 ms de NFR-04 y la escala de NFR-05 con el mismo dato que se usa en dogfooding (ADR-GRP-011)

> Dev Spec: `dev-specs/INF-GRP-002-banco-frescura-escala.md` | Pendiente
>
> **Depende de**: TS-GRP-004 (tiempos en el evento) y US-GRP-002 (observador implementado). **ADRs**: ADR-GRP-011 § 4 (banco y gates, que la Dev Spec debe seguir), ADR-GRP-010 (escenarios), ADR-GRP-005 y ADR-GRP-006 (huella del daemon y del perfil), ADR-GRP-013 (coste de resolver el actor).

### Alcance Técnico

- **Crear** el generador de repos de banco: uno de 100K commits o más, cacheado como artefacto de CI, con 10 worktrees.
- **Implementar** los escenarios de ADR-GRP-011 § 4: modificar un archivo, `git add`, commit, checkout, crear y borrar un worktree, y una ráfaga de 1.000 archivos en un worktree mientras se mide otro.
- **Crear** el suscriptor sin pantalla que registra la recepción en el cliente y, cuando exista el Cockpit, su TUI sin pantalla que registra el render.
- **Implementar** el cálculo del p95 por etapa y total, con descarte de calentamiento y el número mínimo de muestras de ADR-GRP-011.
- **Configurar** los gates: fallo si el p95 del motor supera 300 ms; fallo si el extremo a extremo llega a 500 ms cuando exista el Cockpit; aviso con la etapa nombrada si una etapa se pasa con el total dentro.
- **Medir** la huella del daemon (memoria y CPU en reposo), el crecimiento del perfil y el coste de resolver el actor de un evento.
- **Configurar** el banco en runners de Windows, macOS y Linux.
- **Fuera de alcance**: el histograma de dogfooding dentro del daemon (Dev Spec de US-GRP-002); el modo degradado y la reconciliación, que no cuentan para NFR-04; la medición del Cockpit hasta que exista F-001-02.

### Plan de Verificación

#### Pruebas Automatizadas

- **Sensibilidad**: un retardo artificial en el recomputo que lleva el p95 del motor por encima de 300 ms hace fallar el gate; uno menor que solo pasa una etapa produce aviso y no fallo.
- **Informe**: cada ejecución publica p50, p95 y máximo por etapa, escenario y SO.
- **Reproducibilidad**: dos ejecuciones seguidas sobre el mismo runner dan un p95 total dentro de una tolerancia que fija la Dev Spec.
- **Escala**: durante la ráfaga, el p95 de los otros nueve worktrees sigue dentro de presupuesto.
- **Coherencia**: los nombres de las etapas del informe coinciden con los del bloque de tiempos del contrato.

#### Verificación Manual / Sandbox

- Comparar las cifras del banco con las de SPIKE-GRP-002 y registrar en ADR-GRP-011 cualquier cambio de presupuesto.
