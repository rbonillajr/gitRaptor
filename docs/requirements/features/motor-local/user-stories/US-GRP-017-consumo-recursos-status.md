---
id: US-GRP-017
title: "El desarrollador ve cuánto consume GitRaptor en su máquina"
type: us
status: draft
priority: high
created: 2026-10-05
updated: 2026-10-05
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

## Requisitos Técnicos (para la Dev Spec)

- `engine.resources` en `crates/api`: método de solo lectura, versión menor del contrato, fuera del perfil `mcp` (SEC-MCP-01). Valores tipados, sin cadenas de presentación (NFR-10).
- La CPU media se calcula en el daemon sobre una ventana de 10 min; no se lanza ningún proceso externo por petición (SEC-08).
- El disco se mide con el tamaño en disco de los archivos del perfil, con un tope de tiempo de lectura.
- Textos en/es en `apps/cli`.
