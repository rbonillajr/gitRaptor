---
id: US-TMC-006
title: "El desarrollador sabe qué cambió en su repo, cuándo y quién lo hizo"
type: us
status: draft
priority: high
created: 2026-10-03
updated: 2026-10-03
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  rules:
    - BR-TMC-001
  stories: [US-TMC-001, US-TMC-004, US-GRP-002, US-GRP-007, US-GRP-009]
covers: [BR-TMC-CONS-005, BR-TMC-CONS-003, D-TMC-1, D-TMC-12, D-TMC-19]
blocked_by: []
tags: [time-machine, timeline, atribucion]
---

# US-TMC-006: El desarrollador sabe qué cambió en su repo, cuándo y quién lo hizo

## Descripción

**Como** desarrollador orquestador
**Quiero** consultar el timeline de mi repo con cada operación, su momento, su actor y su nivel de protección
**Para** decidir qué deshacer sin reconstruir la historia a mano

**Valor**: responde "qué cambió, cuándo y quién" (BR-10) sin afirmar nunca que algo lo hizo el humano.

## Reglas cubiertas

BR-TMC-CONS-005 (presentación del actor) · BR-TMC-CONS-003 (nivel de cobertura visible) · D-TMC-1, D-TMC-12, D-TMC-19 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Una operación de un agente detectado**

Dado que el agente "claude-1", detectado, hizo un commit en "feat-login" a las 10:00
Cuando el desarrollador consulta el timeline del repo
Entonces el timeline incluye ese commit con su momento, el worktree "feat-login", los archivos cambiados y el actor "claude-1 (detectado)"

**Escenario: Un agente registrado indica su origen**

Dado que "codex-1" está registrado como otro agente en "feat-pagos" y hizo un commit
Cuando el desarrollador consulta el timeline
Entonces el actor de ese commit figura como "codex-1 (registrado)"

**Escenario: Lo no atribuido nunca figura como humano**

Dado un cambio en "feat-login" que el motor dejó "sin atribuir"
Cuando el desarrollador consulta el timeline
Entonces el actor de ese cambio figura como "Tú u otro (sin atribuir)"
  Y ningún evento del timeline figura con el actor "humano"

**Escenario: Cada punto declara su nivel de protección**

Dado una operación lanzada desde GitRaptor y un cambio hecho con Git crudo sin hooks
Cuando el desarrollador consulta el timeline
Entonces la primera figura con "snapshot previo"
  Y el segundo figura con "capturado por observación"

**Escenario: Repo sin actividad registrada**

Dado un repo recién añadido sin operaciones registradas
Cuando el desarrollador consulta el timeline
Entonces el desarrollador recibe un timeline vacío con el aviso de que aún no hay actividad

## Requisitos Técnicos

- Consulta del timeline por repo sobre el oplog (TS-TMC-002) cruzada con los eventos del motor; el actor se resuelve con la atribución vigente en cada consulta (ADR-TMC-003 § 5).
- Cada punto muestra su nivel (`previo_garantizado`, `previo_hook`, `observacion`) y los eventos sin punto se muestran sin protección (ADR-TMC-004 § 4).
- El texto "Tú u otro (sin atribuir)" lo pone el cliente; el contrato solo tiene agente o sin atribuir.

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-001, US-TMC-004; US-GRP-002, US-GRP-007 y US-GRP-009 de motor-local (eventos y atribución).
- **Externas**: la vista navegable en la TUI se coordina con F-001-02 Cockpit y el design system; esta historia entrega el contenido.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
