---
id: US-TMC-017
title: "El desarrollador ajusta para sí cuánto tiempo se conservan los snapshots"
type: us
status: draft
priority: low
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
  stories: [US-TMC-016]
covers: [BR-TMC-TIME-001, D-TMC-5, D-TMC-15]
blocked_by: ["ADR de formato de la configuración (P8 de motor-local)", "F-001-04 Guardrails"]
tags: [time-machine, retencion, configuracion]
---

# US-TMC-017: El desarrollador ajusta para sí cuánto tiempo se conservan los snapshots

## Descripción

**Como** desarrollador orquestador
**Quiero** fijar la retención en mi perfil o en la configuración local personal de un repo
**Para** adaptar el espacio en disco a mi máquina sin imponérselo al equipo

**Valor**: la retención es una preferencia personal; el nivel de equipo no la admite (Q24).

## Reglas cubiertas

BR-TMC-TIME-001 (niveles admitidos) · D-TMC-5, D-TMC-15 — ver [business-rules.md](../business-rules.md)

## Criterios de Aceptación

**Escenario: La configuración local personal gana al perfil**

Dado una retención de 14 días en el perfil y de 7 días en la configuración local personal del repo
Cuando corresponde purgar en ese repo
Entonces se aplica la retención de 7 días

**Escenario: Solo el perfil define la retención**

Dado una retención de 14 días en el perfil y ninguna en el repo
Cuando corresponde purgar
Entonces se aplica la retención de 14 días

**Escenario: El nivel de equipo no se tiene en cuenta**

Dado una retención de 2 días en la configuración del repo compartida con el equipo y ninguna en los niveles personales
Cuando corresponde purgar
Entonces se aplica la retención por defecto de 30 días

**Escenario: Valor de retención no válido**

Dado una retención "mucho" en el perfil
Cuando la Time Machine lee la configuración
Entonces se aplica la retención por defecto de 30 días
  Y el desarrollador recibe el aviso del valor no válido

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño (flujo/UX):** Pendiente de diseño.
- **Dev Spec:** pendiente (lo genera el Arquitecto).

## Dependencias

- **Historias**: US-TMC-016.
- **Externas**: **bloqueada** por el ADR de formato de la configuración en tres niveles (P8 de motor-local) y por el comando de edición de Guardrails (F-001-04, Q27).
- **Transversal**: transversal (lo define el Arquitecto): verificación en repos temporales, nunca en un repo real; mismo comportamiento en Windows, macOS y Linux; mensajes en inglés y español.
