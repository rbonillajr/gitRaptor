---
id: TS-TMC-003
title: "Capa de escritura acotada y aplicador de estados"
type: ts
status: draft
feature: time-machine
domain: GRP
priority: critical
complexity: high
created: 2026-10-03
updated: 2026-10-03
related:
  adrs: [ADR-TMC-002, ADR-TMC-001, ADR-TMC-003, ADR-GRP-001, ADR-GRP-009]
  stories: [US-TMC-002, US-TMC-003, US-TMC-009, US-TMC-010, US-TMC-011, US-TMC-014, US-TMC-015, US-TMC-019, TS-GRP-002, TS-TMC-001, TS-TMC-002]
  specs: []
ado:
  id: null
  url: null
tags: [time-machine, escritura, restauracion, locks, seguridad-rutas, nfr-01, nfr-02, nfr-07]
---

## TS-TMC-003: Capa de escritura acotada y aplicador de estados

**Valor**: undo, redo y restauración llevan el repo a un estado destino sin ejecutar código del usuario, sin pisar a un agente que trabaja a la vez y sin salir del worktree.

### Descripción

**Como** Arquitecto
**Quiero** una capa de escritura separada de la de lectura y un aplicador que lleve worktrees, índice y refs al estado de un snapshot
**Para** que todas las escrituras de la Time Machine en el repo sigan un único protocolo recuperable y el motor no pueda alcanzarlas (Q21)

> Dev Spec: `dev-specs/TS-TMC-003-escritura-aplicador.md` | Pendiente
>
> **Depende de**: TS-GRP-002 (reglas de invocación y resolución de Git), TS-TMC-001 (almacén) y TS-TMC-002 (diario). **ADRs**: ADR-TMC-002 (frontera, reglas y orden de aplicación), ADR-GRP-009 § 3 (invocación que se hereda).

### Alcance Técnico

- **Crear** la capa de escritura interna de la Time Machine, separada de la de lectura, con una lista cerrada de operaciones de bajo nivel, sin porcelana y con argumentos fijos (SEC-TMC-02).
- **Neutralizar** en toda escritura interna la configuración de sistema y global, los hooks, los filtros, la firma y los transportes, con directorios de Git y de trabajo explícitos (SEC-TMC-02).
- **Separar** revisiones y rutas de las opciones y validar cada nombre de ref del repo y de los metadatos antes de usarlo (SEC-TMC-14).
- **Configurar** la comprobación estática en CI: solo la Time Machine usa la capa de escritura; ni el observador del motor ni el ejecutor de operaciones de usuario la alcanzan.
- **Implementar** el aplicador con el orden de ADR-TMC-002 § 3: locks, objetos, refs, archivos, índice y cierre, con cada paso anotado en el diario.
- **Implementar** la transacción de refs con valor anterior esperado, que falla entera si un agente movió una ref.
- **Implementar** el reemplazo por intercambio atómico con comparación de lo desplazado frente al snapshot previo, que deshace el intercambio y reporta solape si difiere (SEC-TMC-11).
- **Reportar** como "no restaurable con garantía" toda ruta en un sistema de archivos sin intercambio atómico.
- **Implementar** el borrado de rutas sobrantes sin atravesar enlaces ni dispositivos, sin tocar nunca ignorados ni exclusiones declaradas.
- **Garantizar** con apertura relativa a la raíz que ninguna escritura sale del worktree, y rechazar árboles con rutas hostiles o colisiones de nombre en el sistema de archivos destino (SEC-TMC-04).
- **Implementar** la instalación del índice destino con el protocolo de lock de Git y la recreación de worktrees sin checkout.
- **Exponer** los puntos de comprobación previos a aplicar, que completan las precondiciones de US-TMC-015 y el aviso de US-TMC-014.
- **Fuera de alcance**: elegir el estado destino (US-TMC-002, 003, 009, 010 y 011); decidir permisos y solape (US-TMC-012 y 013); rollback automático ante un fallo (descartado, ADR-TMC-002 § 3); push o force-push (nunca).

### Plan de Verificación

#### Pruebas Automatizadas

- **Frontera**: la comprobación estática falla si el motor importa la capa de escritura o si se añade una operación de remoto.
- **Sin código configurable**: el repo canario de SEC-TMC-02 deja 0 marcadores y 0 escrituras fuera del worktree tras aplicar.
- **Concurrencia**: un agente simulado que mueve una rama durante la aplicación hace fallar la transacción sin cambios; uno que escribe un archivo entre la comparación y el intercambio deja esa ruta reportada como solape y su contenido conservado (SEC-TMC-11).
- **Rutas hostiles**: el corpus de SEC-TMC-04 y los nombres de ref de SEC-TMC-14 no producen ninguna escritura fuera del worktree ni ninguna invocación con opciones inyectadas.
- **Exactitud**: tras aplicar un snapshot, working tree, índice y refs coinciden con él salvo las rutas reportadas.
- **Locks**: con un lock de Git ajeno presente, la aplicación no empieza y el lock sigue ahí.
- **Caos**: muerte del daemon en cada paso, cubierta por INF-TMC-001.

#### Verificación Manual / Sandbox

- En Windows, restaurar con un editor que mantiene abierto un archivo afectado y comprobar que la operación queda interrumpida y que el undo la recupera.
