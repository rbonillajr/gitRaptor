---
id: US-DSK-013
title: "El usuario integra o pone al día la rama de un agente desde la app y puede deshacerlo"
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
    - US-CKP-014
    - US-CKP-015
    - US-CKP-016
    - US-CKP-020
    - US-CKP-024
    - US-DSK-008
    - US-DSK-009
  design:
    - DSF-DSK-001
tags:
  - desktop
  - ui-2
  - merge
  - rebase
  - must
---

# US-DSK-013: El usuario integra o pone al día la rama de un agente desde la app y puede deshacerlo

## Descripción

**Como** usuario que revisó el trabajo de un agente, **quiero** integrarlo en la rama base o ponerlo al día sobre ella desde la app, viendo antes qué va a pasar, **para** cerrar el ciclo del agente sin la terminal y con la opción de volver atrás.

**Valor**: BR-27 (UI-2, Must); BR-07. Nivel **UI-2**. Imprescindible.

## Reglas

- Las del Cockpit: preparar, confirmar el plan, ejecutar y "Deshacer" (US-CKP-014); casos límite de US-CKP-024.
- Tocar el trabajo de otro actor exige confirmar el plan concreto (US-CKP-020); la app no lo confirma con un control más débil que la TUI (Q-DSK-3).
- Si hay un conflicto previsto, el usuario lo ve antes de confirmar (US-DSK-008).
- Un merge o rebase que choca queda detenido y el usuario decide cómo salir (US-CKP-016).

## Dependencias

- **Reutiliza**: US-CKP-014, US-CKP-015, US-CKP-016, US-CKP-020, US-CKP-024; operaciones `merge-into-base` y `rebase-onto-base`; `operation.prepare`, `operation.run`, `operation.cancel`.
- **Historias**: US-DSK-009 (revisar antes), US-DSK-008 (conflicto previsto), US-DSK-014 (resolver dentro de la app).
- **Qué falta en el motor**: DEP-DSK-1 si el plan toca trabajo ajeno. transversal (lo define el Arquitecto).

## Criterios de Aceptación

**Escenario: Integrar la rama de un agente que terminó**

Dado "feat-pagos" de "claude-1" terminada y sin conflictos previstos con "main"
Cuando el usuario revisa el plan de integración y lo confirma
Entonces "main" contiene los commits de "feat-pagos"
  Y el historial registra la integración con su punto de recuperación previo

**Escenario: Deshacer una integración**

Dado la integración de "feat-pagos" en "main" recién hecha
Cuando el usuario la deshace
Entonces "main" vuelve a como estaba antes de integrar

**Escenario: Poner al día una rama con conflicto previsto**

Dado "feat-login" con un conflicto previsto contra "main" en "login.rs"
Cuando el usuario pide ponerla al día
Entonces el usuario ve el conflicto previsto antes de confirmar
  Y si confirma y el rebase choca, "feat-login" queda detenida con "login.rs" en conflicto

**Escenario: El estado cambió después de revisar el plan**

Dado un plan de integración de "feat-pagos" revisado por el usuario
Cuando "claude-1" crea un commit nuevo en "feat-pagos" antes de confirmar
Entonces la integración no se ejecuta con el plan viejo
  Y el usuario revisa el plan actualizado

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño y Dev Spec

- **Diseño:** pendiente (UI-2 no está en DSF-DSK-001).
- **Dev Spec:** pendiente.
