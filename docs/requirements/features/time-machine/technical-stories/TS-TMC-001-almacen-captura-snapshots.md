---
id: TS-TMC-001
title: "Almacén de snapshots en el perfil y captura de estado"
type: ts
status: draft
feature: time-machine
domain: GRP
priority: critical
complexity: high
created: 2026-10-03
updated: 2026-10-03
related:
  adrs: [ADR-TMC-001, ADR-TMC-004, ADR-TMC-006, ADR-GRP-006, ADR-GRP-009, ADR-GRP-010]
  stories: [US-TMC-001, US-TMC-004, US-TMC-005, US-TMC-009, US-TMC-016, US-TMC-018, US-TMC-020, TS-GRP-001, TS-GRP-002, TS-TMC-002]
  specs: []
ado:
  id: null
  url: null
tags: [time-machine, snapshots, almacen, perfil, captura, nfr-01, d-tmc-11]
---

## TS-TMC-001: Almacén de snapshots en el perfil y captura de estado

**Valor**: cada snapshot queda fuera del alcance del push, del mantenimiento de Git y de los agentes, y guardarlo no cambia nada del repo del usuario.

### Descripción

**Como** Arquitecto
**Quiero** un almacén privado de snapshots por repo dentro del perfil y la captura que lo alimenta
**Para** que el snapshot previo, la captura por observación y la restauración compartan un único formato que cumple las cuatro garantías de D-TMC-11

> Dev Spec: `dev-specs/TS-TMC-001-almacen-captura-snapshots.md` | Pendiente
>
> **Depende de**: TS-GRP-001 (perfil), TS-GRP-002 (lectura de Git sin escrituras) y TS-TMC-002 (oplog). **Lo informa**: SPIKE-TMC-001 (costes y fallback). **ADRs**: ADR-TMC-001 (forma y garantías), ADR-TMC-004 (camino rápido y consistencia), ADR-TMC-006 (presupuesto por etapa).

### Alcance Técnico

- **Crear** el almacén privado por repo en la carpeta de datos del perfil, con su configuración fija, sin remotos ni hooks y con permisos solo del usuario.
- **Implementar** la siembra inicial del almacén desde el repo en segundo plano, por enlace solo con las condiciones de propiedad y permisos de SEC-TMC-06 y por copia en cualquier otro caso.
- **Excluir** el almacén de las copias de seguridad del SO y aplicar las cuotas de disco y la reserva del snapshot previo de SEC-TMC-12.
- **Implementar** la captura de un snapshot: contenido en bruto del working tree sin ignorados, estado preparado, ramas, HEAD, stash y worktrees.
- **Construir** los árboles del snapshot con un índice temporal propio del almacén, sin tocar nunca el índice del usuario.
- **Implementar** la captura incremental: solo se leen y se guardan las rutas cambiadas desde la captura anterior del mismo worktree.
- **Implementar** el camino rápido: si nada cambió desde la última captura válida, el snapshot reutiliza su árbol.
- **Implementar** el anclaje de commits en el almacén cuando el motor publica un commit nuevo o un movimiento de ref.
- **Registrar** en el snapshot las exclusiones declaradas (submódulos, repos anidados, la lista de credenciales de SEC-TMC-06 y archivos grandes en la captura por observación) con su motivo.
- **Verificar** por hash los objetos del almacén y revalidar árbol y metadatos antes de entregarlos para restaurar (SEC-TMC-09).
- **Implementar** el punto de validez: un snapshot existe solo con su referencia en el almacén y su registro confirmado en el oplog.
- **Implementar** el mantenimiento del almacén con periodo de gracia para objetos sueltos, sin tocar el repo del usuario.
- **Exponer** el tamaño del almacén y los tiempos por etapa como diagnóstico local.
- **Fuera de alcance**: cadencia y disparadores de la captura por observación (US-TMC-004); aplicar un snapshot al repo (TS-TMC-003); retención y purga (US-TMC-016); presentación del timeline (US-TMC-006).

### Plan de Verificación

#### Pruebas Automatizadas

- **Garantías**: push con todas las refs, mantenimiento agresivo de Git tras un reset destructivo y trabajo de un agente en el worktree no exponen ni alteran ningún snapshot (escenarios de INF-TMC-001).
- **Estado observable**: la huella del repo (working tree, índice, refs, status, stash y log de todas las refs) es idéntica antes y después de capturar.
- **Ida y vuelta**: fin de línea CRLF, archivos con filtros y sin convertir, ejecutables, enlaces y nombres Unicode se recuperan bit a bit desde el almacén.
- **Ignorados y excluidos**: ningún árbol del almacén contiene rutas ignoradas, credenciales de la lista ni repos anidados.
- **Seguridad**: pruebas de SEC-TMC-01, 06, 09 y 12 (permisos, siembra con pack ajeno, objeto corrupto, disco lleno).
- **Validez**: un snapshot sin referencia o sin registro confirmado no se lista.
- **Incremental**: con un solo archivo cambiado, la captura lee una sola ruta y reutiliza el resto del árbol.
- **Mantenimiento**: una captura en curso durante el mantenimiento del almacén conserva todos sus objetos.

#### Verificación Manual / Sandbox

- Medir en la máquina de dogfooding la siembra y el disco del almacén con un repo real de tamaño medio, y comparar con las cifras de SPIKE-TMC-001.
