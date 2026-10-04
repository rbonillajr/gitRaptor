---
id: TS-TMC-002
title: "Oplog de la Time Machine con diario de intención y recuperación"
type: ts
status: draft
feature: time-machine
domain: GRP
priority: critical
complexity: medium
created: 2026-10-03
updated: 2026-10-03
related:
  adrs: [ADR-TMC-003, ADR-TMC-007, ADR-GRP-006, ADR-GRP-013]
  stories: [US-TMC-001, US-TMC-002, US-TMC-003, US-TMC-006, US-TMC-008, US-TMC-009, US-TMC-010, US-TMC-011, US-TMC-016, US-TMC-019, TS-GRP-001, TS-GRP-003]
  specs: []
ado:
  id: null
  url: null
tags: [time-machine, oplog, journal, inmutabilidad, recuperacion, nfr-12, d-tmc-18]
---

## TS-TMC-002: Oplog de la Time Machine con diario de intención y recuperación

**Valor**: toda operación y todo snapshot tienen un registro inmutable y un estado nombrado, así que una interrupción nunca deja el repo en un estado desconocido.

### Descripción

**Como** Arquitecto
**Quiero** un oplog por repo en el perfil, solo por anexión, con un diario de estados y una recuperación al arrancar
**Para** que undo, redo, restauración, timeline y purga lean y escriban un único registro que respeta D-TMC-18 y resiste un `kill -9` (NFR-12)

> Dev Spec: `dev-specs/TS-TMC-002-oplog-diario.md` | Pendiente
>
> **Depende de**: TS-GRP-001 (carpeta del perfil y convenciones del almacén) y TS-GRP-003 (ciclo de vida del daemon, donde corre la recuperación). **ADRs**: ADR-TMC-003 (modelo, estados, recuperación), ADR-TMC-007 § 4 (purga interrumpida), ADR-GRP-013 (atribución vigente que se consulta, nunca se copia).

### Alcance Técnico

- **Crear** el almacén del oplog por repo junto al almacén de snapshots, separado del almacén del motor, con versión de esquema y migraciones incluidas.
- **Implementar** los registros de snapshot y de operación (protegida, undo, redo, restauración y rechazo), inmutables una vez escritos.
- **Implementar** el diario de estados de operaciones y snapshots, solo por anexión, con los estados de ADR-TMC-003 § 3.
- **Impedir** en el propio esquema cualquier modificación o borrado de registros y del diario.
- **Registrar** el solicitante, el canal y la confirmación tal como se resolvieron, sin reescribirlos nunca.
- **Exponer** consultas por worktree, periodo, operación, snapshot y nivel que cruzan la atribución vigente del motor sin copiarla.
- **Implementar** la "última operación" y la pila de undo y redo por worktree de ADR-TMC-003 § 4.
- **Implementar** la recuperación al arrancar: snapshots pendientes descartados, referencias huérfanas eliminadas, operaciones a medias abortadas o interrumpidas, purgas a medias resueltas.
- **Implementar** la liberación de los locks de Git propios anotados en el diario, como excepción declarada a BR-TMC-CONS-004 (libera un lock propio sin tocar contenido) y sin tocar nunca un lock que no está anotado.
- **Encadenar** el hash de cada registro con el anterior y guardar la cabeza también fuera del oplog; una cadena rota se declara hueco con su causa (SEC-TMC-09).
- **Registrar** avisos pendientes de interrupción y de purga, con su estado de entrega.
- **Fuera de alcance**: el texto y el momento del aviso al usuario (US-TMC-019); la presentación del timeline (US-TMC-006 a 008); qué operaciones entran en un undo por agente (US-TMC-011).

### Plan de Verificación

#### Pruebas Automatizadas

- **Inmutabilidad**: cualquier intento de modificar o borrar un registro o una entrada del diario falla.
- **Solicitante congelado**: una corrección de atribución posterior no cambia el solicitante registrado, mientras el actor de los eventos sí refleja la atribución vigente.
- **Recuperación**: estados sembrados en cada punto del diario dan, al arrancar, el resultado de ADR-TMC-003 § 6.
- **Locks**: un lock propio anotado se libera; uno ajeno sigue en su sitio.
- **Pila**: undo, undo y redo deshacen y rehacen en el orden esperado; una operación nueva invalida el redo.
- **Aislamiento**: corromper el almacén del motor no impide listar ni restaurar snapshots.

#### Verificación Manual / Sandbox

- Revisar con Rene el modelo de estados frente a los escenarios de US-TMC-019 antes de cerrar la Dev Spec.
