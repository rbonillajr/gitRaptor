---
id: US-DSK-009
title: "El usuario revisa en la app qué cambió en una copia de trabajo o en un commit"
type: us
status: draft
priority: high
created: 2026-10-28
updated: 2026-10-28
feature: desktop
source: inline
related:
  context:
    - CTX-DSK-001
  stories:
    - US-CKP-012
    - US-DSK-004
    - US-DSK-005
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-2
  - diff
  - must
---

# US-DSK-009: El usuario revisa en la app qué cambió en una copia de trabajo o en un commit

## Descripción

**Como** usuario que revisa el trabajo de un agente, **quiero** ver lado a lado o en línea qué cambió, archivo por archivo, en una copia de trabajo o en un commit concreto, **para** decidir si lo integro, lo pido corregir o lo deshago sin abrir otra herramienta.

**Valor**: BR-27 (UI-2, Must); BR-07. Nivel **UI-2**. Imprescindible.

## Reglas

- Las del diff del Cockpit (Q-CKP-8, BR-CKP-CALC-005): lo que entraría al merge y, aparte, lo sin commitear; con topes de tamaño y detección de binarios.
- El contenido de los archivos es texto no confiable: se muestra, nunca se interpreta (SEC-12).

## Dependencias

- **Reutiliza**: US-CKP-012 (revisar lo que un agente integraría, DEP-CKP-3); `diff.worktree`.
- **Historias**: US-DSK-004 y US-DSK-005 (desde dónde se abre).
- **Qué falta en el motor**: DEP-DSK-3, el diff de un commit cualquiera y de un archivo entre dos puntos. transversal (lo define el Arquitecto).

## Criterios de Aceptación

**Escenario: Diff de la copia de trabajo de un agente**

Dado "feat-pagos" con 2 commits propios y 1 archivo sin commitear
Cuando el usuario revisa sus cambios
Entonces ve por separado lo que entraría al merge y lo que está sin commitear, archivo por archivo

**Escenario: Diff de un commit**

Dado el commit "a1b2c3d" en el carril de "feat-login"
Cuando el usuario lo revisa
Entonces ve los archivos que cambió ese commit y sus líneas añadidas y quitadas

**Escenario: Archivo demasiado grande o binario**

Dado "logo.png" y un "datos.json" de 40 MB cambiados en "feat-pagos"
Cuando el usuario revisa los cambios
Entonces sabe que esos archivos cambiaron sin que se muestre su contenido, y por qué

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** pendiente (UI-2 no está en DSF-DSK-001).
- **Dev Spec:** pendiente.
