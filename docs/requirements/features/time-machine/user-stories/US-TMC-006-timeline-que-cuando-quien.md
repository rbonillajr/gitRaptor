---
id: US-TMC-006
title: "El desarrollador sabe qué cambió en su repo, cuándo y quién lo hizo"
type: us
status: implemented
priority: high
created: 2026-10-03
updated: 2026-10-08
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
Entonces el timeline incluye ese commit con su momento, el worktree "feat-login", las rutas de los archivos cambiados (hasta un máximo) y el actor "Claude Code (detectado)"

**Escenario: Un agente registrado indica su origen**

Dado que "codex-1" está registrado como otro agente en "feat-pagos" y hizo un commit
Cuando el desarrollador consulta el timeline
Entonces el actor de ese commit figura como "codex-1 (registrado)"

**Escenario: Lo no atribuido nunca figura como humano**

Dado un cambio en "feat-login" que el motor no atribuye a ningún agente (valor `unattributed`)
Cuando el desarrollador consulta el timeline
Entonces el actor de ese cambio figura como "sin agente"
  Y ningún evento del timeline figura con el actor "humano"

**Escenario: Cada punto declara su nivel de protección**

Dado una operación lanzada desde GitRaptor y un cambio hecho con Git crudo sin hooks
Cuando el desarrollador consulta el timeline
Entonces la primera figura con "snapshot previo"
  Y el segundo figura con "capturado por observación"

**Escenario: Un commit con muchos archivos**

Dado un commit que cambió 25 archivos
Cuando el desarrollador consulta el timeline
Entonces figuran 20 rutas, "+5 más" y el total real, sin contenido ni mensajes

**Escenario: Un undo figura en el timeline**

Dado un undo hecho por un agente
Cuando el desarrollador consulta el timeline
Entonces la entrada del undo figura con su solicitante tal como se registró y con lo que deshizo, aunque la atribución cambie después

**Escenario: Repo sin actividad registrada**

Dado un repo recién añadido sin operaciones registradas
Cuando el desarrollador consulta el timeline
Entonces el desarrollador recibe un timeline vacío con el aviso de que aún no hay actividad

## Requisitos Técnicos

- Consulta del timeline por repo sobre el oplog (TS-TMC-002) cruzada con los eventos del motor; el actor se resuelve con la atribución vigente en cada consulta (ADR-TMC-003 § 5).
- Cada punto muestra su nivel (`previo_garantizado`, `previo_hook`, `observacion`) y los eventos sin punto se muestran sin protección (ADR-TMC-004 § 4).
- El texto de actor sin atribuir lo pone el cliente ("sin agente"/"no agent", i18n; enmienda 2026-10-08 validada por PO); el contrato solo tiene agente o `unattributed`.

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** [DS-US-TMC-006](../dev-specs/US-TMC-006-timeline-que-cuando-quien.md).

## Dependencias

- **Historias**: US-TMC-001, US-TMC-004; US-GRP-002, US-GRP-007 y US-GRP-009 de motor-local (eventos y atribución).
- **Externas**: la vista navegable en la TUI se coordina con F-001-02 Cockpit y el design system; esta historia entrega el contenido.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.

## Estado de la implementación (2026-10-08)

Implementado en: PR #198.

Notas (fuera del alcance de esta ficha o sin bloquearla):
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md), XP-35).
- La vista navegable del timeline en la TUI: F-001-02 (Cockpit). Los huecos (`reconciled`) y los filtros adicionales: US-TMC-007.
- La pista "(sin agente; inferido: X)" no está en el contrato actual de `TimelineEntry`: queda como mejora cuando el contrato lo lleve.
