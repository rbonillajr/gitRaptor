---
id: US-TMC-011
title: "El desarrollador deshace solo lo que hizo un agente en un periodo"
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
  stories: [US-TMC-002, US-TMC-007, US-TMC-012, US-TMC-013, US-GRP-007, US-GRP-009, US-GRP-010]
covers: [BR-TMC-WF-002, BR-TMC-VAL-001, BR-TMC-EDGE-002, BR-TMC-AUTH-001, D-TMC-2, D-TMC-22, D-TMC-23]
blocked_by: ["P17 de motor-local (D-TMC-22)"]
tags: [time-machine, undo, undo-por-agente, bloqueada-p17]
---

# US-TMC-011: El desarrollador deshace solo lo que hizo un agente en un periodo

## Descripción

**Como** desarrollador orquestador
**Quiero** usar `raptor undo --agent <id> --since <duración>` para deshacer solo las operaciones de ese agente en el periodo
**Para** revertir el trabajo de un agente que se equivocó sin perder lo que hicieron los demás ni yo

**Valor**: es la promesa central de la Time Machine y la demo de validación del MVP (BRD § 13).

## Reglas cubiertas

BR-TMC-WF-002 (undo por agente con atribución vigente) · BR-TMC-VAL-001 · BR-TMC-EDGE-002 (nada de huecos) · BR-TMC-AUTH-001 (solicitante, D-TMC-23) · D-TMC-2, D-TMC-22 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: Deshacer lo de un agente en los últimos 20 minutos**

Dado que en los últimos 20 minutos "claude-1" hizo dos commits y editó "api.rs"
  Y un cambio "sin atribuir" modificó "README.md"
  Y el solicitante del undo queda sin atribuir
Cuando pide deshacer lo de "claude-1" en los últimos 20 minutos
  Y lo confirma de forma interactiva
Entonces se revierten los dos commits y la edición de "claude-1"
  Y el cambio en "README.md" sigue intacto

**Escenario: Lo de otros agentes no entra**

Dado que "claude-2" hizo un commit en el mismo periodo en otros archivos
  Y un solicitante sin atribuir pide deshacer lo de "claude-1" en ese periodo
Cuando lo confirma de forma interactiva
Entonces el commit de "claude-2" sigue aplicado

**Escenario: Se usa la atribución vigente**

Dado una sesión detectada como "claude-1" que el desarrollador corrigió a "codex-1"
Cuando un solicitante sin atribuir pide deshacer lo de "claude-1" en el periodo de esa sesión
Entonces ninguna operación de esa sesión se deshace

**Escenario: Lo ocurrido en un hueco nunca entra**

Dado que "claude-1" estaba registrado antes de un hueco de observación y en el hueco aparecieron commits
Cuando un solicitante sin atribuir pide deshacer lo de "claude-1" en un periodo que incluye el hueco
Entonces los commits del hueco siguen aplicados

**Escenario: Un agente pide deshacer lo de otro agente**

Dado commits de "claude-2" en los últimos 20 minutos
Cuando "claude-1" pide deshacer lo de "claude-2" en ese periodo
Entonces la petición se rechaza con el motivo
  Y el repo no cambia

**Escenario: Agente sin operaciones en el periodo**

Dado que "claude-9" no tiene operaciones en los últimos 20 minutos
Cuando se pide deshacer lo de "claude-9" en ese periodo
Entonces el repo no cambia
  Y el solicitante recibe el motivo

## Requisitos Técnicos

- Conjunto = operaciones con atribución vigente al agente en el periodo; excluye "sin atribuir", otros agentes y huecos (BR-TMC-WF-002).
- Destino por ruta y por ref: estado anterior al primer cambio del agente, sin tocar cambios de otros actores; el solape detiene (ADR-TMC-005 § 5).
- Bloqueada hasta que se cierre P17 de motor-local (D-TMC-22).

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-002, US-TMC-007, US-TMC-012, US-TMC-013; US-GRP-007, US-GRP-009 y US-GRP-010 de motor-local.
- **Externas**: **bloqueada por P17 de motor-local** (D-TMC-22): si al retirar una corrección los eventos vuelven a la atribución detectada, cambia qué entra en este undo.
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
