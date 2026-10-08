---
id: US-GRD-005
title: "El desarrollador cuenta las acciones peligrosas que Guardrails bloqueó en cada repo"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-07
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRD-001
    - US-GRP-009
tags:
  - guardrails
  - registro-decisiones
  - kpi
---

# US-GRD-005: El desarrollador cuenta las acciones peligrosas que Guardrails bloqueó en cada repo

## Descripción

**Como** desarrollador orquestador, **quiero** consultar por repo cada operación denegada con su motivo y su actor, **para** medir cuántas acciones peligrosas se evitaron, el KPI del BRD (§ 9).

**Valor**: el valor de Guardrails se puede demostrar con datos de dogfooding.

## Reglas cubiertas

BR-CONS-004 (qué se anota, con todos sus campos; en el perfil, nunca en el repo; Q-GRD-10; el KPI cuenta solo lo verificado y lo anotado en modo degradado va aparte) · BR-TIME-002 (retención de 90 días) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-001 (primera denegación); US-GRP-009 (motor-local: agente registrado para el actor "agente X").
- **Externas**: ninguna.
- **Transversal**: Windows, macOS y Linux; nada sale de la máquina (NFR-03).

## Criterios de Aceptación

**Escenario: Una denegación queda registrada con todos sus datos**

Dado el repo "demo" protegido y el agente "codex" registrado en el worktree "feat-x"
Cuando "codex" hace force-push de "feat-x" con Git directo y se deniega
Entonces el registro de "demo" tiene una entrada con el momento, el repo, el worktree, la rama, el actor "codex", la operación, la decisión, la regla, el nivel que la causó y la capa
  Y el actor queda como "sin atribuir" cuando lo intenta un proceso que el motor no asigna a ningún agente

**Escenario: Se cuentan las acciones bloqueadas de un periodo**

Dado el repo "demo" con 3 denegaciones esta semana y 1 la semana anterior
Cuando el desarrollador consulta las acciones bloqueadas de "demo" de esta semana
Entonces obtiene 3

**Escenario: Las entradas que GitRaptor no pudo verificar se muestran aparte**

Dado el repo "demo" con 3 denegaciones verificadas esta semana y 2 entradas anotadas en modo degradado, que GitRaptor no pudo verificar
Cuando el desarrollador consulta las acciones bloqueadas de "demo" de esta semana
Entonces obtiene 3
  Y las 2 entradas sin verificar aparecen aparte, fuera del recuento
  Y si el desarrollador pide incluirlas, obtiene 5

**Escenario: Las operaciones permitidas sin regla no se anotan**

Dado el repo "demo" protegido
Cuando un proceso hace 10 commits permitidos
Entonces el registro de "demo" no tiene entradas nuevas

**Escenario: El registro no está en el repo**

Dado el repo "demo" con entradas en su registro
Cuando se preparan todos los cambios del repo para un commit
Entonces no se recoge nada del registro de decisiones

**Escenario: Las entradas caducan a los 90 días**

Dado una entrada del registro de hace 91 días y otra de hace 89 días
Cuando el desarrollador consulta el registro de "demo"
Entonces solo aparece la de hace 89 días

## Requisitos Técnicos

- **Gobierno**: ADR-GRD-006 § 1 (tabla propia en el almacén por repo del perfil; los permitidos sin regla no se anotan), § 2 (agregación; el KPI suma `count`), § 3 (90 días desde `lastAt`; las consultas filtran por fecha) y § 6 (consulta y KPI).
- **Campos**: el actor sale del `git` más cercano (ADR-GRD-003 § 4); el repo, de la clave del almacén; la operación va normalizada y nunca con argv; `reasons` lleva regla y nivel, y `layer` la capa (ADR-GRD-006 § 1).
- **KPI**: cuenta `denial` más las peticiones `rejected` y `expired`. Excluye por defecto `origin = spool-unverified`, que se muestra aparte y se incluye con un filtro explícito; `exception-rejected` también va aparte (ADR-GRD-006 § 6).
- **Modo degradado**: el cliente del hook escribe en el spool y el daemon lo ingiere marcado como `spool-unverified` (ADR-GRD-006 § 5; ADR-GRD-003 § 4).
- **Crates**: `crates/core` módulo `guardrails` (escritura, agregación, purga e ingesta del spool), `crates/api` (consulta paginada y KPI) y `apps/cli` (consulta y escritura del spool desde `raptor hook`).
- **Enablers**: la suite de registro de INF-GRD-001 (huella del repo sin cambios) **bloquea el merge**. SPIKE-GRD-001 y TS-GRD-001 no aplican.
- **NFR, SEC y verificación**: NFR-GRD-09; SEC-GRD-09, 11 y 12. ADR-GRD-006 Validación 1 a 7, 10 y 11.
- **Enmiendas en motor-local**: ADR-GRP-006 § 4 (tabla `guardrails_decisions` y excepción de escritura de clientes para el spool) y ADR-GRP-005 § 1 (la misma excepción). El actor "agente X" depende de US-GRP-009.

## Diseño y Dev Spec

- **Diseño:** no aplica.
- **Dev Spec:** [DS-US-GRD-005](../dev-specs/US-GRD-005-registro-de-bloqueos.md) (2026-10-07). Primera entrega: escenarios 1, 2, 3 (el de los permitidos), 4, 5 y 6 con el daemon. Queda diferido el escenario de las entradas en modo degradado (el spool, `spool-unverified`), con dueño en la Dev Spec (Fuera de alcance).
