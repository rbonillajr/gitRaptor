---
id: US-GRP-017
title: "El desarrollador ve cuánto consume GitRaptor en su máquina"
type: us
status: implemented
priority: high
created: 2026-10-05
updated: 2026-10-08
feature: motor-local
source: inline
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  adrs:
    - ADR-GRP-015
  stories:
    - US-GRP-001
    - US-GRP-002
    - US-GRP-018
    - TS-GRP-005
    - TS-GRP-006
tags:
  - motor-local
  - recursos
  - huella
  - status
  - m1
---

# US-GRP-017: El desarrollador ve cuánto consume GitRaptor en su máquina

## Descripción

**Como** desarrollador que deja GitRaptor encendido todo el día junto a sus agentes, **quiero** ver con `raptor status --resources` cuánta CPU, memoria y disco gasta GitRaptor y qué vigila, **para** confiar en que no me quita recursos y detectar enseguida si lo hace.

**Valor**: la preocupación por los recursos (Rene Bonilla, 2026-10-05) se puede medir cada día del dogfooding, no solo en CI. Es la herramienta con la que se mide el criterio de salida de recursos de M1.

> **Origen**: decisión del orquestador (2026-10-05), validada por el PO y el Arquitecto. La sección de recursos de `raptor doctor` va aparte, en US-GRP-018.

## Reglas cubiertas

RES-10 (visibilidad) y RES-01, RES-02, RES-04 y RES-05 como valores mostrados ([non-functional.md](../../../../architecture/non-functional.md) § Consumo de recursos) · NFR-10 (i18n) · SEC-12 (salida saneada).

## Dependencias

- **Historias**: US-GRP-001 (repos observados), US-GRP-002 (watchers en marcha).
- **Técnicas**: el método `engine.resources` del canal (ADR-GRP-015 § 4) lo crea esta historia en su Dev Spec; es aditivo y de solo lectura. La clase activa por pool solo aparece cuando exista TS-GRP-005; mientras tanto se muestra "no disponible".
- **Transversal**: verificado en macOS; Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

## Criterios de Aceptación

**Escenario: El desarrollador ve el consumo del motor**

Dado el motor observando el repo "demo" con 3 worktrees
Cuando el desarrollador ejecuta `raptor status --resources`
Entonces ve la CPU media de los últimos 10 minutos y el pico, la memoria residente del motor, los descriptores abiertos y las vigilancias activas
  Y ve el disco que ocupa el perfil y el que ocupa la Time Machine de "demo"
  Y cada valor muestra su objetivo y si lo cumple

**Escenario: Un valor fuera de objetivo se señala**

Dado el motor con una memoria residente mayor que su objetivo de RES-02
Cuando el desarrollador ejecuta `raptor status --resources`
Entonces la memoria aparece marcada como fuera de objetivo, con el objetivo al lado
  Y el comando termina con el mismo código de salida que cuando todo está dentro de objetivo

**Escenario: Salida para scripts**

Dado el motor observando el repo "demo"
Cuando el desarrollador ejecuta `raptor status --resources --json`
Entonces recibe un JSON con los mismos valores en unidades fijas (porcentaje, bytes y recuentos)
  Y ningún campo contiene texto de presentación ni contenido del repo

**Escenario: Sin el motor en marcha no se arranca solo para medir**

Dado el motor parado
Cuando el desarrollador ejecuta `raptor status --resources`
Entonces ve que el motor no está en marcha y el disco del perfil y de la Time Machine leídos del perfil
  Y el comando no arranca el motor

**Escenario: Un agente no ve el consumo por MCP**

Dado "Claude Code" conectado por MCP al repo "demo"
Cuando consulta las herramientas disponibles
Entonces ninguna expone el consumo de recursos del motor

## Enmienda (2026-10-07)

> **Origen**: Decisión del orquestador (2026-10-07), validada por el PO: observación por niveles (propuesta B, aceptada por Rene Bonilla el 2026-10-06). Con más de 100 repos, el desarrollador tiene que poder comprobar que el consumo escala con los repos activos y no con los observados.

> **Ajuste (2026-10-08)**: Decisión del orquestador (2026-10-08), validada por el PO. Los descriptores no se reparten por nivel porque no se pueden medir con honestidad: en macOS un stream de FSEvents por raíz no consume un descriptor por vigilancia, y en Linux todas las vigilancias de inotify comparten un descriptor. Por nivel se muestran los repos, worktrees y vigilancias, y para los dormidos el coste de sus redes de seguridad; los descriptores se muestran una vez, los del proceso. El contrato (ADR-GRP-010 N8) no cambia.

- **Dependencia nueva, no bloqueante**: TS-GRP-005 sigue igual; los niveles los da TS-GRP-006 (observación por niveles, del Arquitecto). Sin TS-GRP-006, los niveles se muestran como "no disponible" y el resto del escenario no cambia.
- **Regla**: RES-10 (visibilidad), con la enmienda de NFRs del Arquitecto para la observación por niveles.

**Escenario: El desarrollador ve los repos activos y dormidos y lo que cuesta cada nivel**

Dado el motor observando 12 repos, 3 con actividad reciente y 9 sin actividad desde hace más que el umbral de reposo
Cuando el desarrollador ejecuta `raptor status --resources`
Entonces ve cuántos repos están activos y cuántos dormidos
  Y ve, por nivel, los repos, worktrees y vigilancias que usa, y los descriptores del proceso en total
  Y la salida con `--json` trae los mismos recuentos y valores en unidades fijas

## Requisitos Técnicos (para la Dev Spec)

- `engine.resources` en `crates/api`: método de solo lectura, versión menor del contrato, fuera del perfil `mcp` (SEC-MCP-01). Valores tipados, sin cadenas de presentación (NFR-10).
- La CPU media se calcula en el daemon sobre una ventana de 10 min; no se lanza ningún proceso externo por petición (SEC-08).
- El disco se mide con el tamaño en disco de los archivos del perfil, con un tope de tiempo de lectura.
- Textos en/es en `apps/cli`.

## Estado de la implementación (2026-10-08)

Implementado en: PR #93, #173.

Notas (fuera del alcance de esta ficha o sin bloquearla):
- `pools` (TS-GRP-005) y `power_saving` (US-GRP-019) se rellenan en sus historias.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
