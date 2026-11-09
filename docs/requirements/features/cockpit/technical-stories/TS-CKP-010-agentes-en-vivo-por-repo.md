---
id: TS-CKP-010
title: "Recuento de agentes en vivo por repo en el ámbito global"
type: ts
status: ready
feature: cockpit
domain: GRP
priority: medium
complexity: medium
created: 2026-11-08
updated: 2026-11-08
related:
  adrs: [ADR-CKP-003, ADR-GRP-016, ADR-GRP-013, ADR-GRP-017]
  stories: [US-DSK-004, US-CKP-001, US-CKP-002, US-GRP-007]
  specs: [DS-US-DSK-004]
ado:
  id: null
  url: null
tags: [cockpit, desktop, ambito-global, sesiones, agentes-en-vivo, br-ckp-calc-001, capacidad]
---

## TS-CKP-010: Recuento de agentes en vivo por repo en el ámbito global

**Valor**: la barra lateral de la app y el selector de la TUI dicen cuántos agentes trabajan en cada repo sin pedir la lista de sesiones de todos.

### Descripción

**Como** Arquitecto
**Quiero** que el ámbito global del canal publique, para cada repo observado, cuántas sesiones de agente presentes tiene, en la instantánea y con un evento cuando cambie
**Para** que los clientes muestren "Agentes · N en vivo" y el recuento de cada repo sin derivarlo (BR-CKP-CALC-001) y sin suscribirse al ámbito de cada repo, y cerrar el hueco G1 de [DS-US-DSK-004](../../desktop/dev-specs/US-DSK-004-flota-en-vivo.md) (ADR-CKP-003 § 4, N3)

**Estado actual (verificado en el código el 2026-11-08).** El resumen de repo del ámbito global trae estado, ruta, atención y nivel, pero ninguna sesión (`crates/api/src/scope.rs`, `RepoSummaryView`); las sesiones solo llegan por repo (`sessions.list` y `session.state`, del ámbito del repo). **Decisión del orquestador (2026-11-08), validada por Arquitecto**: se publica el recuento y no la lista, porque es lo que pintan los clientes y mantiene acotado el tamaño de la instantánea global.

### Alcance Técnico

- **Añadir** al resumen de cada repo del ámbito global el recuento de sesiones presentes, separado en activas e inactivas, como cambio aditivo anunciado con una capacidad nueva (ADR-GRP-016), sin subir el protocolo; un cliente sin la capacidad ve las mismas formas que hoy.
- **Publicar** un evento del ámbito global cuando cambie el recuento de un repo (una sesión aparece, cambia entre activa e inactiva o termina), emitido en el mismo paso del daemon que publica el cambio de sesión, para que la instantánea global y sus eventos converjan.
- **Publicar** el recuento como "no disponible" cuando el sistema no detecta sesiones, nunca como cero (BR-CKP-CALC-001, la misma regla que la atención del repo).
- **Recalcular** los recuentos al arrancar el daemon y al añadir o retirar un repo, a partir de las sesiones que el daemon conoce, sin inventar sesiones de un periodo no observado.
- **Excluir** el recuento de las conexiones con perfil `mcp`, como el resto de datos de sesión que esa vista no lleva.
- **Fuera de alcance**: pintar el recuento en la app (la siguiente historia de la app que toque la barra lateral, tras US-DSK-004) y en el selector de la TUI; sesiones terminadas e historial; el detalle por worktree, que sigue en el ámbito del repo; Windows, donde la detección no existe y el recuento sale "no disponible".

### Plan de Verificación

#### Pruebas Automatizadas

- **Forma por capacidad**: con la capacidad, la instantánea global y el evento llevan el recuento; sin ella, la forma es la de hoy y el evento no llega.
- **Cambios**: una sesión que aparece, pasa a inactiva o termina cambia el recuento del repo correcto y produce un solo evento global por cambio.
- **Convergencia**: una instantánea global tomada después de N eventos da los mismos recuentos que la instantánea anterior más esos eventos aplicados.
- **Sin detección**: el recuento sale "no disponible" y nunca cero.
- **Arranque y repos**: tras reiniciar el daemon con sesiones presentes, y tras añadir o retirar un repo, los recuentos coinciden con las sesiones del daemon.
- **Perfil `mcp`**: la vista del MCP no lleva el recuento.

#### Verificación Manual / Sandbox

- En macOS con dos repos y tres sesiones de Claude Code: `raptor status --json` o un cliente de prueba del canal muestran los recuentos de cada repo, y cerrar una sesión los actualiza sin pedir otra instantánea.
