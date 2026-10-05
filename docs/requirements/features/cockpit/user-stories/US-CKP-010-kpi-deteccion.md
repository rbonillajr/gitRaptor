---
id: US-CKP-010
title: "El desarrollador sabe qué parte de los conflictos reales se vio antes de ocurrir"
type: us
status: draft
priority: medium
created: 2026-10-04
updated: 2026-10-04
feature: cockpit
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
  stories:
    - US-CKP-006
tags:
  - cockpit
  - prediccion-conflictos
  - kpi
  - must
---

# US-CKP-010: El desarrollador sabe qué parte de los conflictos reales se vio antes de ocurrir

## Descripción

**Como** desarrollador orquestador, **quiero** un registro local de cada conflicto previsto y de cada conflicto real, **para** comprobar con datos si GitRaptor detecta al menos el 70 % de los conflictos antes de que ocurran.

**Valor**: mide el KPI principal de BR-06 (BRD § 9).

## Reglas cubiertas

BR-CKP-CONS-005 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-006; US-CKP-016 para los conflictos de merges lanzados desde el Cockpit (los de Git directo no la necesitan).
- **Técnicas**: TS-CKP-001. Escritura en el perfil vía daemon: reutiliza el contrato de US-CKP-004 (N9). **Huecos del motor**: DEP-CKP-11 (el daemon escribe el registro en el perfil) y DEP-CKP-14 (estado en conflicto publicado).

## Criterios de Aceptación

**Escenario: Un conflicto con ⚡ previo cuenta como detectado antes**

Dado ⚡ "claude-1 ↔ claude-2" en "src/api.rs" desde las 10:00
Cuando a las 11:00 el merge de las dos ramas choca en "src/api.rs"
Entonces el registro cuenta ese conflicto como detectado antes

**Escenario: Un conflicto que solo tuvo solape no cuenta como detectado**

Dado solo ⚠ entre "claude-3" y "claude-4" en "README.md"
Cuando su merge choca en "README.md"
Entonces el registro cuenta un conflicto real no detectado antes

**Escenario: Detectado por equivalencia, contado aparte**

Dado ⚡ "claude-1 ↔ claude-2" en "src/api.rs" a las 10:00 y "claude-1" integrado en "main" a las 10:30
Cuando a las 11:00 el merge de "claude-2" en "main" choca en "src/api.rs"
Entonces el registro lo cuenta como detectado antes por equivalencia, separado de los detectados por el par literal

**Escenario: Conflictos fuera de la observación van aparte**

Dado un conflicto real ocurrido durante un hueco de observación
Cuando el desarrollador consulta el registro
Entonces ese conflicto aparece listado aparte y no entra en el porcentaje

**Escenario: Previstos que no ocurrieron y retención**

Dado ⚡ previstos durante 100 días en "shop"
Cuando el desarrollador consulta el registro
Entonces ve el porcentaje de detectados antes y, aparte, los ⚡ que no llegaron a ocurrir
  Y solo aparecen los últimos 90 días, todo calculado en su máquina

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec). Equivalencia del "mismo par": ADR-CKP-001 § 9._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001.
- **Dev Spec:** pendiente.
