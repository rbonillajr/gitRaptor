---
id: INF-TMC-001
title: "Arnés de caos y de garantías de los snapshots en los tres SO"
type: inf
status: partially-implemented
feature: time-machine
domain: GRP
priority: critical
complexity: medium
created: 2026-10-03
updated: 2026-10-08
related:
  adrs: [ADR-TMC-001, ADR-TMC-002, ADR-TMC-003, ADR-TMC-005, ADR-TMC-007, ADR-GRP-009]
  stories: [US-TMC-001, US-TMC-002, US-TMC-009, US-TMC-016, US-TMC-018, US-TMC-019, INF-GRP-001, TS-TMC-001, TS-TMC-003]
  specs: [DS-INF-TMC-001]
ado:
  id: null
  url: null
tags: [time-machine, ci, caos, nfr-12, nfr-01, garantias, d-tmc-11]
---

## INF-TMC-001: Arnés de caos y de garantías de los snapshots en los tres SO

**Valor**: una regresión que deja el repo irrecuperable tras una interrupción, o que expone o altera un snapshot, rompe el CI antes del merge.

### Descripción

**Como** Arquitecto
**Quiero** un arnés con inyección de fallos en cada paso de la Time Machine y escenarios hostiles de Git, convertido en gate de CI
**Para** verificar NFR-12 y las garantías de D-TMC-11 con el mismo criterio en Windows, macOS y Linux

> Dev Spec: [`dev-specs/INF-TMC-001-arnes-caos-recuperable.md`](../dev-specs/INF-TMC-001-arnes-caos-recuperable.md) | Primer corte implementado
>
> **Depende de**: INF-GRP-001 (huella "repo intacto" y repo canario, que se reutilizan), TS-TMC-001 (almacén) y TS-TMC-003 (aplicador). **ADRs**: ADR-TMC-003 § 6 (estados esperados tras recuperar), ADR-TMC-001 (garantías), ADR-TMC-002 § 3 (pasos), ADR-TMC-007 § 4 (purga).

### Alcance Técnico

- **Crear** puntos de inyección de fallo en cada transición del diario y en cada paso del aplicador, la captura y la purga, activables solo en builds de prueba.
- **Implementar** la muerte forzada del daemon en cada punto y la verificación al relanzar: estado del oplog, aviso único y repo recuperable con un undo.
- **Implementar** la comparación del repo con el snapshot previo tras la recuperación, archivo a archivo, índice y refs.
- **Reutilizar** la huella "repo intacto" de motor-local para verificar que guardar un snapshot y purgar no cambian nada del repo.
- **Implementar** los escenarios hostiles: push con todas las refs a un remoto temporal, mantenimiento agresivo tras un reset destructivo, agente que limpia y resetea, locks de Git ajenos y operaciones de Git a medias.
- **Ampliar** el repo canario de motor-local con los casos de SEC-TMC-02 (monitor del sistema de archivos, directorio de trabajo forzado hacia fuera, inclusión condicional hostil, filtros, firma global y plantillas con hooks) para todas las escrituras internas.
- **Implementar** los casos de caos y hostiles de seguridad: escritura concurrente durante el intercambio atómico, disco lleno con reserva del snapshot previo, almacén u oplog editados fuera del daemon, corpus de rutas hostiles y refs con opciones inyectadas (SEC-TMC-04, 09, 11, 12 y 14).
- **Configurar** el arnés como gate de CI en los tres SO para toda historia de la Time Machine que escribe.
- **Fuera de alcance**: rendimiento (Dev Spec de US-TMC-020 sobre INF-GRP-002); escenarios del motor (INF-GRP-001); pruebas con repos reales del usuario (nunca).

### Plan de Verificación

#### Pruebas Automatizadas

- **Sensibilidad**: un defecto sembrado (confirmar el snapshot antes de crear su referencia, o aplicar archivos antes de la transacción de refs) hace fallar el arnés.
- **Cobertura**: el informe lista cada punto de inyección con su resultado y falla si alguno no se ejecutó.
- **Garantías**: los cuatro escenarios de US-TMC-018 pasan en los tres SO.
- **Determinismo**: dos ejecuciones con la misma semilla dan el mismo resultado.

#### Verificación Manual / Sandbox

- Repetir en la máquina de dogfooding el escenario de muerte durante una restauración con 10 worktrees activos y anotar los tiempos de recuperación.

### Estado de la implementación (2026-10-08)

Implementado en: PR #189 (primer corte, [DS-INF-TMC-001](../dev-specs/INF-TMC-001-arnes-caos-recuperable.md)).

**Hecho**: puntos de fallo con nombre en la captura del previo, en las transiciones de la operación protegida y en cada paso del aplicador (feature `chaos`, solo en builds de test). El daemon real muere con `SIGKILL` en cada uno; la recuperación cierra los estados y `raptor undo` devuelve el worktree byte a byte (archivos, índice, `HEAD` y ramas). Escenarios hostiles con `raptor undo`: envío de todas las refs a un remoto, mantenimiento agresivo tras un reset destructivo, agente que limpia y resetea, locks de Git ajenos y merge a medias. Suite `apps/cli/tests/tm_chaos.rs`, en el CI de macOS y Linux.

**Pendiente**:

- Canario de SEC-TMC-02 sobre las escrituras internas.
- Casos de seguridad SEC-TMC-04, 09, 11, 12 y 14 (escritura concurrente durante el intercambio, disco lleno, almacén u oplog editados fuera del daemon, rutas hostiles y refs con opciones) como escenarios del arnés. Hoy, en parte, en `tm_apply` y `tm_store_safety`, en proceso.
- Puntos de la purga (ADR-TMC-007; la purga no está construida) y de la restauración a un punto del timeline (US-TMC-009).
- Sensibilidad (defecto sembrado) e informe de cobertura como artefacto del CI.
- Windows: *Pendiente: etapa de validación multiplataforma* (XP-33).
- Verificación manual con 10 worktrees en la máquina de dogfooding.
