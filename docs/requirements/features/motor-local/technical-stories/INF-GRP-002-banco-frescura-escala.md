---
id: INF-GRP-002
title: "Banco de medición de frescura y escala"
type: inf
status: ready
feature: motor-local
domain: GRP
priority: high
complexity: medium
created: 2026-10-03
updated: 2026-10-07
related:
  adrs: [ADR-GRP-011, ADR-GRP-010, ADR-GRP-005, ADR-GRP-006, ADR-GRP-013, ADR-GRP-015]
  stories: [US-GRP-001, US-GRP-002, US-GRP-012, US-GRP-017, TS-GRP-004, TD-GRP-002, TS-GRP-006]
  specs: [DS-INF-GRP-002]
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

> Dev Spec: `dev-specs/INF-GRP-002-dev-spec.md` | Hecha (2026-10-05). Comando: `cargo bench -p gitraptor-cli --bench engine`. Deuda abierta: [TD-GRP-002](./TD-GRP-002-motor-bajo-rafaga.md)
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
- **Incorporar** lo que dejó SPIKE-GRP-002 (Enmienda 2026-10-04 de ADR-GRP-010 y ADR-GRP-011; decisión del orquestador, validada por el Arquitecto):
  - `t0` al fin del comando en los escenarios de Git; la detección se aísla solo en "modificar un archivo".
  - Debounce medido como duración efectiva (`t_recv` → `t_flush`) y **calibración de la holgura del temporizador** por SO; con esos datos, la Dev Spec decide entre una constante por SO y la calibración en tiempo de ejecución.
  - Ahead/behind con la primitiva en proceso de `crates/git` (`gix`) frente a `git rev-list --count --left-right`, en una rama a 50K commits de la base, con y sin `commit-graph`.
  - **Recreación del stream** (alta y baja de worktrees con escrituras concurrentes en los demás) y **pérdida silenciosa** recuperada por la reconciliación periódica dentro de su intervalo: gate de corrección (100% recuperado y marcado como hueco), sin gate de latencia. En macOS bloquea además cualquier PR que suba `notify`.
  - Worktrees de más de 5.000 archivos (coste del recomputo y del modo degradado) y RSS aislado del daemon.
- **Incorporar** el escenario de escala por niveles (Enmienda 2026-10-07 de la Dev Spec; decisión del orquestador, validada por el Arquitecto): 100 repos pequeños observados, 5 activos y 95 dormidos, con los techos de consumo en reposo de 10 worktrees, el tiempo de despertar y el retraso de un dormido (RES-11 y RES-12). Se implementa con TS-GRP-006.
- **Fuera de alcance**: el histograma de dogfooding dentro del daemon (Dev Spec de US-GRP-002); el modo degradado y la reconciliación, que no cuentan para NFR-04; la medición del Cockpit hasta que exista F-001-02.

### Plan de Verificación

#### Pruebas Automatizadas

- **Sensibilidad**: un retardo artificial en el recomputo que lleva el p95 del motor por encima de 300 ms hace fallar el gate; uno menor que solo pasa una etapa produce aviso y no fallo.
- **Informe**: cada ejecución publica p50, p95, p99 y máximo por etapa, escenario y SO.
- **Escala**: durante la ráfaga, el p95 de los otros nueve worktrees sigue dentro de presupuesto.
- **Recreación del stream**: con un escritor activo en los demás worktrees, 40 altas y bajas no dejan ningún cambio sin publicar tras la reconciliación; un evento descartado sin marca se recupera en la siguiente reconciliación periódica.
- **Escala por niveles** (`tiered-scale`): con 100 repos observados, la CPU, el RSS y los descriptores en reposo siguen bajo los techos de 10 worktrees, los almacenes de los dormidos están cerrados y un commit en un dormido se publica dentro del intervalo del barrido más 2 s. Las cifras marginales y el despertar van como aviso hasta tener línea base.
- **Coherencia**: los nombres de las etapas del informe coinciden con los del bloque de tiempos del contrato y con los nombres canónicos de ADR-GRP-011 § 2: `t0`, `t_recv`, `t_flush`, `t_computed`, `t_persisted`, `t_published`, `t_client_recv` y `t_render`.

#### Verificación Manual / Sandbox

- **Reproducibilidad** (movida desde las pruebas automatizadas, decisión del orquestador 2026-10-05, validada por el Arquitecto): dos ejecuciones seguidas en la misma máquina dan un p95 total dentro de max(25 ms, 20 %).

- Comparar las cifras del banco con las de SPIKE-GRP-002 y registrar en ADR-GRP-011 cualquier cambio de presupuesto.

### Notas de integración

Nota de integración (Time Machine, ADR-TMC-006 y US-TMC-020): el banco añade el escenario 'operación protegida con trabajo sin commitear' sobre el repo de referencia de SPIKE-TMC-001, con 1 y con 10 worktrees activos, con gate de p95 < 200 ms del snapshot previo y aviso por etapa. El gate del motor se ejecuta con la Time Machine activa.

Nota (2026-10-05, Dev Spec): **gate de huella añadido por Rene** (CPU y RSS en reposo y en ráfaga, descriptores y watches). La ráfaga de 10.000 archivos se suma como escenario de estrés a la de 1.000. Los incumplimientos bajo ráfaga los recoge [TD-GRP-002](./TD-GRP-002-motor-bajo-rafaga.md).
