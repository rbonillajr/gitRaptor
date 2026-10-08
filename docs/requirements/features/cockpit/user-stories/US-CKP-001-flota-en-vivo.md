---
id: US-CKP-001
title: "El desarrollador ve en vivo qué agente trabaja en cada worktree de un repo"
type: us
status: implemented
priority: high
created: 2026-10-04
updated: 2026-10-08
feature: cockpit
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-CALC-001
    - BR-CKP-CONS-001
    - BR-CKP-CONS-003
    - BR-CKP-VAL-002
    - BR-CKP-TIME-001
    - BR-CKP-WF-001
  dev-spec:
    - DS-US-CKP-001
  stories:
    - US-GRP-001
    - US-GRP-002
    - US-GRP-007
    - US-GRP-008
tags:
  - cockpit
  - worktrees-en-vivo
  - esqueleto-andante
  - must
---

# US-CKP-001: El desarrollador ve en vivo qué agente trabaja en cada worktree de un repo

## Descripción

**Como** desarrollador orquestador, **quiero** ver en una sola vista cada worktree del repo con su agente, estado de sesión, rama, archivos modificados, ahead/behind y última actividad, al día en vivo, **para** saber qué hace cada agente sin saltar entre terminales.

**Valor**: BR-04 (Must). Esqueleto andante del Cockpit: la primera vista útil y la base de todas las demás.

## Reglas cubiertas

BR-CKP-CALC-001 · BR-CKP-CONS-001 · BR-CKP-CONS-003 · BR-CKP-VAL-002 · BR-CKP-TIME-001 · BR-CKP-WF-001 (fila, estados y símbolos; el orden por atención y las terminadas son de US-CKP-002) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-001 y US-GRP-002 (estado y eventos publicados), US-GRP-007 (sesiones de Claude Code), US-GRP-008 ("sin atribuir").
- **Técnicas**: INF-CKP-001 (cliente del canal, saneado único, gate de 100 ms), TS-CKP-004 (tokens y símbolos), TS-GRP-004 (canal y contrato; DEP-CKP-6).
- **Huecos del motor**: DEP-CKP-4 (última actividad). Mientras no se publique, la columna dice "no disponible"; la historia no espera por ello.

## Criterios de Aceptación

**Escenario: Una fila por worktree, con el agente primero**

Dado el repo "shop" observado, con el worktree principal en "main" y el worktree "feat-pagos" con una sesión Activa de "claude-1", 2 archivos modificados y 3 commits por delante y 1 por detrás de la rama base "main" (copia local)
Cuando el desarrollador abre la TUI en "shop"
Entonces la vista muestra el worktree principal en la primera fila
  Y la fila de "feat-pagos" muestra "claude-1", Activo, la rama, 2 archivos y "↑3 ↓1" respecto a "main", con la antigüedad de la copia local "no disponible"

**Escenario: Un cambio se ve en menos de medio segundo**

Dado la TUI abierta en "shop" con 10 worktrees y 100K commits
Cuando "claude-1" modifica un tercer archivo en "feat-pagos"
Entonces la fila muestra 3 archivos en menos de 500 ms p95 de extremo a extremo
  Y la TUI tarda como mucho 100 ms p95 desde que recibe el evento hasta que lo pinta (gate de CI sin pantalla)

**Escenario: Lo no atribuido nunca se presenta como "humano"**

Dado un commit en "feat-docs" hecho desde una terminal sin agente detectado
Cuando el desarrollador mira la fila de "feat-docs"
Entonces el actor se presenta "Tú u otro (sin atribuir)"

**Escenario: Un campo que el motor no publica no se calcula en la TUI**

Dado el daemon ya en marcha y el motor sin publicar la última actividad del worktree
Cuando el desarrollador abre la TUI
Entonces la columna de última actividad dice "no disponible"
  Y el proceso de la TUI no lanza Git (auditoría de procesos de INF-GRP-001 limitada a su PID) ni abre el perfil (V5 y V6 de ADR-CKP-003)

**Escenario: Texto no confiable no altera la terminal**

Dado un worktree cuya rama lleva en el nombre una secuencia de escape que borraría la pantalla
Cuando la TUI pinta su fila
Entonces la secuencia aparece neutralizada y visible, y la terminal no cambia de estado

> **Enmienda (2026-10-06), ajuste del PO al implementar.** El motor cuenta el ahead/behind contra la rama base (`DivergenceView`), no contra el remoto, y no publica la antigüedad de la copia local. El escenario 1 nombra la referencia real y la antigüedad se muestra "no disponible" (BR-CKP-CALC-001, misma enmienda).

## Requisitos Técnicos

Ver la [Dev Spec](../dev-specs/US-CKP-001-flota-en-vivo.md) (DS-US-CKP-001). En resumen:

- La vista compone StatusBar, AgentList y KeyHints de la biblioteca TS-CKP-005, que se amplía en tres puntos: fila sin agente, columnas que crecen y recuentos "no disponibles".
- Las sesiones llegan por `sessions.list` tras la instantánea del repo y se fusionan con `session.state` con un único upsert (D1, enmienda de ADR-CKP-003 § 4).
- `--theme`, `--no-color` y `--ascii`, más la detección del fondo, se resuelven antes del lector de eventos (D3).
- El gate de frescura de punta a punta es el escenario `tui-modify` del banco INF-GRP-002 (D4).

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (TUI, enmienda del 2026-10-06); ADR-GRP-004 § 3; ADR-CKP-003.
- **Dev Spec:** [DS-US-CKP-001](../dev-specs/US-CKP-001-flota-en-vivo.md).

## Estado de la implementación (2026-10-08)

Implementado en: PR #118 (ajustes en #126, #129, #130, #131, #136 y #175).

- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
