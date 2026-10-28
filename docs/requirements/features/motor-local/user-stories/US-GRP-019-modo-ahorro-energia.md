---
id: US-GRP-019
title: "GitRaptor gasta menos batería cuando el portátil no está enchufado"
type: us
status: draft
priority: medium
created: 2026-10-05
updated: 2026-10-27
feature: motor-local
source: inline
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  adrs:
    - ADR-GRP-015
    - ADR-GRP-010
    - ADR-CKP-001
  stories:
    - TS-GRP-005
    - US-GRP-004
    - US-GRP-017
tags:
  - motor-local
  - recursos
  - bateria
  - ahorro-energia
---

# US-GRP-019: GitRaptor gasta menos batería cuando el portátil no está enchufado

## Descripción

**Como** desarrollador que trabaja con el portátil desenchufado, **quiero** que GitRaptor reduzca su trabajo de fondo mientras voy con batería, sin dejar de protegerme, **para** que tenerlo encendido no me acorte la batería.

**Valor**: GitRaptor deja de competir con la batería del usuario y mantiene el snapshot previo, Guardrails y los cambios en vivo.

> **Origen**: decisión de Rene Bonilla (2026-10-05): con batería se espacian las reconciliaciones y se pausa el predictor. Forma y prioridad: decisión del orquestador (2026-10-05), validada por el PO (historia observable, Could, fuera de M1) y el Arquitecto (el mecanismo vive en TS-GRP-005).

## Reglas cubiertas

RES-08 · BR-CONS-005, Enmienda 2026-10-27 (un evento perdido sin marca se recupera, como mucho, en el intervalo de reconciliación vigente: 15 min con batería; la cifra de 15 min sigue como ⚠️ **ASSUMPTION**) · BR-CKP-CALC-001 (par "sin calcular" con motivo).

> **BR-CONS-005**: Decisión del orquestador (2026-10-27), validada por PO, sobre la propuesta del Arquitecto (ADR-GRP-015 § 2; DS-TS-GRP-005, G5).

## Dependencias

- **Historias**: TS-GRP-005 (mecanismo y fuente de energía inyectable), US-GRP-004 (reconciliación periódica), US-GRP-017 (muestra el modo).
- **Externas**: el predictor (TS-CKP-001, Cockpit) para el escenario del predictor. Si aún no existe, ese escenario se verifica cuando llegue.
- **Transversal**: verificado en macOS con fuente de energía simulada; Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

## Criterios de Aceptación

**Escenario: Con batería se espacian las reconciliaciones**

Dado `engine.powerSaving` en "auto" y el portátil con batería
Cuando pasa una hora
Entonces el motor hace una reconciliación periódica cada 15 minutos en lugar de cada 5
  Y los cambios en vivo siguen llegando dentro de su presupuesto de frescura

**Escenario: Con batería, un cambio perdido sin marca se recupera dentro del intervalo vigente**

Dado `engine.powerSaving` en "auto", el portátil con batería y un cambio en el worktree "feat-login" que el motor no vio ni marcó como hueco
Cuando pasan 15 minutos
Entonces el cambio figura en el historial como recuperado por la reconciliación, según la regla de los huecos

**Escenario: Con batería se pausa el predictor**

Dado `engine.powerSaving` en "auto", el portátil con batería y dos worktrees que tocan el mismo archivo
Cuando el desarrollador abre la vista de conflictos
Entonces el par aparece como "sin calcular" con el motivo "ahorro de energía"
  Y al enchufar el portátil el par se calcula en menos de un minuto

**Escenario: El ahorro nunca quita protección**

Dado el portátil con batería
Cuando un agente lanza una operación protegida en el worktree "feat-login"
Entonces se toma el snapshot previo igual que con corriente
  Y Guardrails decide igual que con corriente

**Esquema del escenario: El desarrollador elige el modo**

Dado `engine.powerSaving` en "<modo>" en el perfil y el portátil "<fuente>"
Cuando el desarrollador ejecuta `raptor status --resources`
Entonces ve el modo de ahorro "<estado>"

Ejemplos:
| modo | fuente | estado |
| auto | con batería | activo |
| auto | enchufado | inactivo |
| on | enchufado | activo |
| off | con batería | inactivo |
