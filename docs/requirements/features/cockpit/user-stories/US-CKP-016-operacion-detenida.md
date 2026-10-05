---
id: US-CKP-016
title: "Un merge o rebase que choca queda detenido y el desarrollador decide cómo salir"
type: us
status: draft
priority: high
created: 2026-10-04
updated: 2026-10-04
feature: cockpit
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
  stories:
    - US-CKP-014
    - US-CKP-015
    - US-TMC-015
tags:
  - cockpit
  - acciones-por-agente
  - conflicto-real
  - operacion-en-curso
  - must
---

# US-CKP-016: Un merge o rebase que choca queda detenido y el desarrollador decide cómo salir

## Descripción

**Como** desarrollador orquestador, **quiero** que un merge o rebase que choca o que tarda quede a la vista, con Abortar, Abrir en el editor y Cancelar, **para** salir de un conflicto real sin perder trabajo ni quedar con el repo a medias.

**Valor**: BR-07 (Must); NFR-01 (cero pérdida).

## Reglas cubiertas

BR-CKP-WF-003 · BR-CKP-WF-008 · BR-CKP-EDGE-002 — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-014 y US-CKP-015; US-TMC-015 (la Time Machine rechaza con una operación en curso); US-CKP-013 para el escenario del editor (Should).
- **Técnicas**: TS-CKP-002 (Abortar y Cancelar no están gobernados; solo necesitan el ejecutor).
- **Huecos del motor**: DEP-CKP-14 (estado en conflicto y rutas sin fusionar publicados).

## Criterios de Aceptación

**Escenario: El merge choca y queda detenido**

Dado "feat-pagos" y "main" con cambios que chocan en "src/api.rs"
Cuando el desarrollador integra "feat-pagos"
Entonces la TUI muestra "Merge detenido: 1 archivo en conflicto" con "src/api.rs", y ofrece Abortar y Abrir en el editor
  Y el repo queda tal como lo deja Git; la TUI no aborta sola ni resuelve el conflicto

**Escenario: Abortar devuelve el estado previo y habilita Deshacer**

Dado el merge de "feat-pagos" detenido
Cuando el desarrollador elige Abortar
Entonces "main" vuelve al commit previo al merge, sin operación en curso, y Deshacer vuelve a estar disponible

**Escenario: Deshacer no se ofrece con la operación en curso**

Dado el merge de "feat-pagos" detenido
Cuando el desarrollador busca Deshacer
Entonces Deshacer aparece desactivado con "aborta primero el merge en curso"

**Escenario: Operación de Git a medias hecha por el agente**

Dado un rebase a medias en "feat-login" lanzado por "claude-3" con Git directo
Cuando el desarrollador selecciona "feat-login"
Entonces la fila muestra "rebase en curso" y solo ofrece ver diff y abrir en el editor

**Escenario: HEAD separado**

Dado "feat-tmp" con HEAD separado
Cuando el desarrollador mira su fila
Entonces la fila muestra "HEAD separado" en lugar de una rama

**Escenario: Cancelar una operación lenta**

Dado el rebase de "feat-pagos" lanzado desde la TUI, en curso desde hace 2 minutos por un hook lento
Cuando el desarrollador elige Cancelar
Entonces la operación se interrumpe como un Ctrl-C y la TUI la muestra detenida, fallida con Deshacer o sin cambios, según lo que dejó Git
  Y nunca se cancela sola por tiempo

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto (Dev Spec). Cancelar: ADR-CKP-002 § 6._

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001.
- **Dev Spec:** pendiente.
