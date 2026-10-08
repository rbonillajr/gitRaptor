---
id: US-CKP-025
title: "La TUI ofrece observar el repo en el que se abre y, fuera de un repo, lleva a los ya observados"
type: us
status: in-progress
priority: medium
created: 2026-10-07
updated: 2026-10-07
feature: cockpit
source: inline
related:
  context:
    - CTX-CKP-001
  rules:
    - BR-CKP-001
    - BR-GRP-001
  stories:
    - US-CKP-001
    - US-CKP-003
    - US-CKP-004
    - US-GRP-020
    - US-GRP-022
tags:
  - cockpit
  - repo-no-observado
  - repos-descubiertos
  - confirmacion-humana
  - should
---

# US-CKP-025: La TUI ofrece observar el repo en el que se abre y, fuera de un repo, lleva a los ya observados

## Descripción

**Como** desarrollador que abre `raptor` desde cualquier carpeta, **quiero** que la TUI me ofrezca observar el repo en el que estoy si aún no lo está y que, fuera de un repo, me lleve a mis repos observados, **para** llegar a la flota sin saber de memoria qué comando falta.

**Valor**: corrige lo que vio Rene Bonilla en el dogfooding (2026-10-06): abierta desde su carpeta personal, la TUI decía "add it with `raptor repo add`" aunque el repo ya estaba observado. También es donde el desarrollador ve y decide los repos descubiertos (US-GRP-020).

> **Origen**: Decisión del orquestador (2026-10-07), validada por el PO, sobre la propuesta A3 aceptada por Rene Bonilla (2026-10-06) y el hallazgo 1 del dogfooding.

## Reglas cubiertas

BR-CKP-AUTH-002 (solicitante por ascendencia) — ver [business-rules.md](../business-rules.md) · BR-AUTH-001 y BR-AUTH-003 del Motor local (observar exige confirmación humana) — ver [business-rules.md](../../motor-local/business-rules.md)

## Dependencias

- **Historias**: US-CKP-001; US-GRP-001 (añadir un repo). Los escenarios de repos descubiertos dependen de US-GRP-020 y US-GRP-022.
- **Relación con US-CKP-004**: cuando exista una vista recordada, US-CKP-004 abre el último repo usado y gana sobre esta historia; esta historia cubre la apertura fuera de un repo **sin vista recordada**.
- **Relación con US-CKP-003**: sin ningún repo observado sigue valiendo su guía de primer repo.
- **Transversal**: verificado en macOS; Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

## Criterios de Aceptación

**Escenario: En un repo no observado la TUI pregunta y por defecto no observa**

Dado el repo "notes" sin observar y el repo "shop" observado
Cuando el desarrollador abre la TUI dentro de "notes" y responde con Intro a "¿Observar este repo? [s/N]"
Entonces "notes" no está observado
  Y la TUI sigue disponible para ir a "shop"

**Escenario: Responder "s" observa el repo y lo muestra**

Dado el repo "notes" sin observar
Cuando el desarrollador abre la TUI dentro de "notes" y responde "s"
Entonces "notes" está observado
  Y la vista muestra sus worktrees

**Escenario: Una TUI lanzada por un agente no ofrece observar**

Dado el repo "notes" sin observar y la TUI abierta desde la terminal de "claude-1"
Cuando la TUI arranca dentro de "notes"
Entonces no pregunta si observar el repo
  Y "notes" no está observado

**Escenario: Fuera de un repo, con varios observados, la TUI ofrece elegir**

Dado los repos "shop" y "api" observados y ninguna vista recordada
Cuando el desarrollador abre la TUI desde su carpeta personal
Entonces la TUI ofrece elegir entre "shop" y "api"
  Y no sugiere añadir un repo

**Escenario: Fuera de un repo, con un solo repo observado, la TUI lo abre**

Dado "shop" como único repo observado y ninguna vista recordada
Cuando el desarrollador abre la TUI desde su carpeta personal
Entonces la vista muestra "shop"

**Escenario: La TUI propone los repos descubiertos**

Dado "billing" descubierto en la raíz "~/code"
Cuando el desarrollador abre la TUI
Entonces la TUI avisa "¿Observar billing?" y permite aceptarlo o descartarlo
  Y si no decide nada, "billing" sigue descubierto, sin observar

## Requisitos Técnicos

> Arquitecto, 2026-10-07. Decisión del orquestador, validada por el Arquitecto. Diseño en ADR-GRP-010, Enmienda (2026-10-07) N4, N6 y N8, aceptada (**Decisión de Rene (2026-10-07)**).

- **Al arrancar**, la TUI resuelve el cwd con `repo.locate`. Si está dentro de un repo no observado, hay una terminal interactiva y el solicitante (`hello.requester`) no es un agente, pregunta "¿Observar este repo? [s/N]", con "N" por defecto. Con "s" llama a `repo.add`, comando reservado que el daemon vuelve a autorizar (SEC-03). Si la TUI la lanzó un agente, no pregunta.
- **Fuera de un repo**: lee los repos observados de la instantánea. Con uno, lo abre. Con varios, ofrece elegir, salvo que haya una vista recordada.
- **Repos descubiertos**: se leen con `discovery.candidates` y llegan con el evento `repo.discovered` (capacidad `discovery.events`). Aceptar es `repo.add` y descartar es `discovery.dismiss`, los dos reservados. Con un solicitante agente, la TUI no los muestra.
- **Niveles**: la vista de flota muestra el `tier` de cada repo (capacidad `observation.tiers`) y no despierta ningún repo dormido. Abrir un repo concreto se suscribe a él y lo despierta (ADR-GRP-010 N4): la TUI muestra "reconciliando" hasta 2 s (RES-12) con el último estado conocido.
- **Textos**: los nombres de repos y rutas pasan por el saneador de SEC-12. Los textos van con i18n en y es (guía de contenido del design system).
- **Verificación**: la `App` sin pantalla sobre `TestBackend` contra un daemon de prueba, como en US-CKP-001, con un solicitante humano y con uno agente simulado.

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (estados vacíos, preguntas con valor por defecto); ADR-GRP-004 § 3.
- **Dev Spec:** [DS-US-CKP-025](../dev-specs/US-CKP-025-tui-repo-no-observado.md).

## Estado de la implementación (2026-10-07)

- **Hechos**: escenarios 1, 2 y 3 (la pregunta en un repo no observado, solo para la persona, con su valor por defecto). Los escenarios 4 y 5 ya los cubría el PR #126.
- **Diferido**: escenario 6 (repos descubiertos) y los niveles, que dependen de US-GRP-020/022 y de ADR-GRP-010 N4. Decisión del orquestador (2026-10-07), validada por el PO.
