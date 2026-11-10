---
id: US-WTL-002
title: "El desarrollador descarta el worktree de un agente terminado, sabe cuánto disco ganó y lo puede restaurar"
type: us
status: ready
priority: high
milestone: M3
blocked_by:
  - US-CKP-017
  - US-WTL-003
created: 2026-11-10
updated: 2026-11-10
feature: worktree-lifecycle
related:
  context:
    - CTX-WTL-001
  adrs:
    - ADR-TMC-001
    - ADR-TMC-007
    - ADR-CKP-002
  stories:
    - US-CKP-017
    - US-TMC-002
    - US-DSK-006
  rules:
    - BR-WTL-006
    - BR-WTL-007
    - BR-WTL-008
    - BR-WTL-011
  specs: []
ado:
  id: null
  url: null
tags:
  - worktree-lifecycle
  - descartar
  - time-machine
  - disco
  - must
---

# US-WTL-002: El desarrollador descarta el worktree de un agente terminado, sabe cuánto disco ganó y lo puede restaurar

## Descripción

**Como** desarrollador orquestador, **quiero** descartar el worktree y la rama de un agente con un snapshot previo, sin poder borrar el de un agente que sigue vivo y con el disco que gané de verdad, **para** liberar espacio con 4–5 agentes a la vez sin perder trabajo por un descarte equivocado.

**Valor**: BR-07 (Must); NFR-01. Amplía US-CKP-017 con la protección del agente vivo y el informe de disco. Mismo comportamiento en la CLI, la TUI y la app.

## Criterios de Aceptación

**Escenario: Descarte con snapshot e informe de disco real**

Dado el worktree "feat-old" de "claude-1", sesión terminada, con 1 commit sin integrar y 2 archivos sin commitear
  Y `target/` copiado desde la semilla
Cuando el desarrollador descarta "feat-old" y confirma
Entonces GitRaptor guarda antes un snapshot con la rama "feat-old" y los 2 archivos sin commitear
  Y borra el worktree y la rama
  Y informa el espacio que ganó el disco, medido como lo libre antes y después, no la suma del tamaño de lo borrado

**Escenario: Restaurar dentro de la retención**

Dado "feat-old" descartado hace 10 días, con la retención por defecto de 30 días
Cuando el desarrollador lo restaura
Entonces vuelven el worktree, la rama "feat-old" con su commit y los 2 archivos sin commitear

**Escenario: La confirmación dice lo que no se guarda**

Dado "feat-old" con `target/` y `.env` ignorados
Cuando el desarrollador pide descartarlo
Entonces la confirmación avisa, antes de borrar, que los ignorados, artefactos de compilación incluidos, no se guardan en el snapshot y nombra `target/` y `.env`

**Escenario: Agente vivo protegido**

Dado el worktree "feat-wip" con "claude-2" en cualquiera de estos estados: activo, esperando una decisión humana, esperando el CI o colgado
Cuando el desarrollador pide descartar "feat-wip"
Entonces la petición se rechaza indicando que "claude-2" ocupa "feat-wip"
  Y no se borra nada ni se guarda ningún snapshot

**Escenario: Worktree ajeno con HEAD separado**

Dado un worktree con HEAD separado que no creó GitRaptor
Cuando el desarrollador pide descartarlo
Entonces GitRaptor lo rechaza porque está marcado «ajeno, no tocar»

**Escenario: El snapshot no se puede guardar**

Dado "feat-old" sin agente vivo
Cuando el desarrollador lo descarta y el snapshot falla
Entonces GitRaptor no borra el worktree ni la rama e informa que el descarte no se hizo porque no hubo snapshot

## Criterios de Certificación

- Verificar que el disco informado coincide con la diferencia de espacio libre antes y después, con artefactos compartidos con la semilla · *(método: medición en macOS con APFS, como el diario del 2026-11-09)*.
- Validar que el 100 % de los descartes se restauran dentro de la retención con rama y trabajo sin commitear · *(método: prueba automatizada con repos temporales)*.

## Requisitos Técnicos

_Pendiente — lo completa el Arquitecto en Fase 2 (el PO no llena esta sección)._

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño (confirmación de descarte en la TUI y en la app).

## Dependencias

- **De User Stories**: Requiere US-CKP-017 (descartar con Deshacer) y US-WTL-003 (los estados que definen un agente vivo) · Relacionado US-TMC-002, US-DSK-006 · Bloquea US-WTL-004.
- **Preguntas abiertas que esta historia no fija**: 10 (límite de descartes restaurables aparte de los 30 días), 13 (forzar el descarte con agente vivo; sin cerrar, no hay forma de forzarlo), 15 (si algún ignorado debe guardarse).
- **Cross-cutting / transversales**: cómo se mide el disco ganado con bloques compartidos: transversal (lo define el Arquitecto).
