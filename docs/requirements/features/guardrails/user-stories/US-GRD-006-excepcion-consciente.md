---
id: US-GRD-006
title: "El desarrollador hace a conciencia una operación prohibida y queda constancia"
type: us
status: implemented
priority: high
created: 2026-10-04
updated: 2026-10-28
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRD-001
    - US-GRD-005
    - US-GRP-007
    - US-GRP-009
tags:
  - guardrails
  - excepcion-consciente
  - actores
  - sin-atribuir
---

# US-GRD-006: El desarrollador hace a conciencia una operación prohibida y queda constancia

## Descripción

**Como** desarrollador orquestador, **quiero** que las reglas se apliquen a cualquier actor, sea un agente o "sin atribuir", y tener una vía consciente y registrada para saltarme una regla en una operación concreta, **para** que un agente sin registrar no se cuele haciéndose pasar por mí sin que yo pierda el control de mi repo.

**Valor**: fail-safe sin dejar atrapado al humano (Q-GRD-1).

## Reglas cubiertas

BR-AUTH-003 (misma decisión para "agente X" y "sin atribuir"; excepción consciente) · BR-AUTH-001 (un agente no usa la excepción; anuncio, ventana para cancelar y auditoría, Q-GRD-19 y Q-GRD-24) · BR-CONS-004 (la excepción queda registrada) · BR-VAL-003 (la excepción salta también las ramas protegidas y las rutas prohibidas con `appliesTo: everyone`, Q-GRD-50) · alcance de la excepción en el MVP: push, borrar una rama local y rebase no interactivo (Q-GRD-47) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-001 (decisión), US-GRD-005 (registro); US-GRP-007 y US-GRP-009 (motor-local: agente detectado y registrado).
- **Externas**: el mecanismo que asegura que un agente no puede dar la confirmación humana es transversal (lo define el Arquitecto; riesgo R-GRD-3).
- **Transversal**: Windows, macOS y Linux.

## Criterios de Aceptación

**Esquema del escenario: La misma operación recibe la misma decisión sea cual sea el actor**

Dado el repo "demo" protegido
Cuando "<actor>" hace force-push de "feat-x" con Git directo
Entonces la operación se deniega con el mismo motivo

Ejemplos:
| actor |
| Claude Code (detectado) |
| codex (registrado) |
| un proceso sin atribuir |

**Escenario: El desarrollador usa la excepción consciente**

Dado el repo "demo" protegido, donde el force-push está denegado
Cuando el desarrollador confirma de forma consciente que quiere hacer ese force-push concreto
Entonces GitRaptor anuncia la excepción y abre una ventana en la que se puede cancelar
  Y al cerrarse la ventana sin cancelación, la operación se ejecuta
  Y el registro de "demo" anota la excepción con la operación, la regla saltada y el momento

**Escenario: Una excepción cancelada durante la ventana no se ejecuta**

Dado el desarrollador pidió una excepción para un force-push en "demo"
Cuando la cancela antes de que se cierre la ventana
Entonces la operación no se ejecuta
  Y la cancelación queda en el registro

**Escenario: La excepción vale para una sola operación**

Dado el desarrollador usó la excepción para un force-push en "demo"
Cuando otro proceso hace un segundo force-push
Entonces se deniega como antes

**Escenario: Un agente no puede usar la excepción**

Dado el agente "codex" registrado en "demo"
Cuando "codex" intenta usar la excepción consciente desde su canal
Entonces la excepción se rechaza, la operación no se ejecuta
  Y el intento queda en el registro

**Escenario: La excepción no cubre una operación fuera de su alcance**

Dado el repo "demo" protegido, donde hacer commit está denegado
Cuando el desarrollador pide la excepción consciente para un commit
Entonces la excepción se rechaza sin anunciarse y nada se ejecuta
  Y el motivo dice qué operaciones admite la excepción

## Requisitos Técnicos

- **Gobierno**: ADR-GRD-003 § 1 (el actor no cambia la decisión) y § 4 (actor desde el `git` más cercano: detectado, registrado o "sin atribuir"); ADR-GRD-007 § 1 a § 3 (la excepción es un comando reservado que relaja, con un token de un solo uso).
- **R-GRD-3** (un agente no puede dar la confirmación humana): controles de ADR-GRP-005 § 6 en el daemon; los vectores no detectables quedan aceptados por acción hasta el factor fuera de banda (ADR-GRD-007 § 2). No hay excepción desde el MCP (el canal rechaza el método), desde un agente (`exception-rejected` en el registro y en la auditoría) ni en modo degradado (ADR-GRD-007 § 1 y § 3).
- **Anuncio y ventana antes del token** (D5 (Q-GRD-19) y D10 (Q-GRD-24)): toda excepción, también la aprobación explícita en el Cockpit, se anuncia en todos los clientes y espera una ventana cancelable; solo al cerrarse sin cancelación se emite el token. Cancelada, no se emite token, la operación no se ejecuta y la cancelación queda en el registro con el `kind` `exception-cancelled` y en la auditoría (ADR-GRD-007 § 1 y § 3, paso 3; ADR-GRD-006 § 1).
- **Token**: de ≥ 128 bits; el daemon guarda solo su hash, ligado a la transición exacta, al `git` hijo directo del `raptor` solicitante y a un TTL que rige hasta la primera presentación. Una transición distinta no queda cubierta (ADR-GRD-007 § 3; ADR-GRD-003 § 6).
- **Crates**: `apps/cli` (`raptor guard exec`: anuncio, ventana y lanzamiento del `git` hijo), `crates/core` módulo `guardrails` (normalización del argv con el traductor de los hooks, autorización, emisión y consumo del token; DS-US-GRD-006 D3), `crates/api` (método reservado) y `crates/policy` (la misma evaluación para cualquier actor).
- **Enablers**: la suite de token de INF-GRD-001 **bloquea el merge**. SPIKE-GRD-001 no bloquea, pero aporta los casos de normalización del argv y el criterio humano o agente en Windows, que ADR-GRD-007 § 2 marca como ⚠️ **ASSUMPTION**. TS-GRD-001 no aplica.
- **NFR, SEC y verificación**: NFR-GRD-06; SEC-GRD-04, 05 y 06. ADR-GRD-003 Validación 5 y 8; ADR-GRD-006 Validación 8; ADR-GRD-007 Validación 1 a 10 y 12.
- **Enmiendas en motor-local**: ADR-GRP-005 § 6 con SEC-03 (la excepción como comando reservado con D5 (Q-GRD-19)) y ADR-GRP-013 § 1 (auditoría de cada uso). Las filas "detectado" y "registrado" del esquema dependen de US-GRP-007 y US-GRP-009.

## Diseño y Dev Spec

- **Diseño:** no aplica. Esta historia es de CLI y daemon (`raptor guard exec`); la excepción y el anuncio en la TUI son del Cockpit (US-CKP-019).
- **Dev Spec:** [DS-US-GRD-006](../dev-specs/US-GRD-006-excepcion-consciente.md) (2026-10-21, status `draft`; decisiones del orquestador validadas por el Arquitecto). Dos PR: PR-A (contrato y piezas puras) y PR-B (daemon, plantilla 4 del dispatcher, `raptor guard exec` y suites e2e, con la suite de token de INF-GRD-001). Cierra también el escenario de la excepción de US-GRD-017. Q-GRD-47 (alcance: push, borrar una rama local y rebase) y Q-GRD-50 (salta también las políticas `everyone`) quedan decididas: decisión del orquestador (2026-10-21), validada por Arquitecto y PO.

## Estado de la implementación (2026-10-28)

Implementado en: PR #295 (PR-A) y PR #301 (PR-B). Dev Spec: [DS-US-GRD-006](../dev-specs/US-GRD-006-excepcion-consciente.md).

Completa: `raptor guard exec`, daemon, plantilla 4 del dispatcher, cliente del hook y suites e2e (escenarios 1 a 6 y suite de token de INF-GRD-001). Verificado en macOS; Linux lo cubre el CI de ubuntu.

Pendiente: Windows (ligadura por handle, Ctrl-C, Validación 12) en la etapa de validación multiplataforma; el escenario de D5 con `diffSize`/`commitFormat` en un rebase (`the_exception_skips_everyone_commit_policies_in_a_rebase`) queda sin test: US-GRD-009 ya está en main, pero el anuncio aún no evalúa esas reglas sobre los commits del rebase, así que hoy la excepción se rechaza (fail-closed); seguimientos de seguridad sin Critical ni High: M-01 c (filter/merge driver, `gpg.program`, `core.sshCommand` y `credential.helper` heredan el token en su entorno; un nieto no puede presentarlo), M-03 (el rechazo repetido de un token vivo aún salta el tope del registro), L-02 (`ExceptionToken` deriva `PartialEq`), L-03 (la relectura de drivers de la llamada 2 usa su `path`, no el worktree ligado).
