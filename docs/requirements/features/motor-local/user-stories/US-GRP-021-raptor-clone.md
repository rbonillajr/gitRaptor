---
id: US-GRP-021
title: "El desarrollador clona un repo con raptor clone y decide en el momento si GitRaptor lo observa"
type: us
status: draft
priority: low
created: 2026-10-07
updated: 2026-10-07
feature: motor-local
source: inline
related:
  context:
    - CTX-GRP-001
  rules:
    - BR-GRP-001
  stories:
    - US-GRP-001
    - US-GRP-020
tags:
  - motor-local
  - clonar
  - confirmacion-humana
  - could
---

# US-GRP-021: El desarrollador clona un repo con raptor clone y decide en el momento si GitRaptor lo observa

## Descripción

**Como** desarrollador que empieza a trabajar en un repo nuevo, **quiero** clonarlo con `raptor clone <url>` y responder ahí mismo si GitRaptor lo observa, **para** que el repo quede protegido antes de lanzar el primer agente sobre él.

**Valor**: un paso menos que clonar y luego añadir. Complementa US-GRP-020 para quien no declara carpetas de código.

> **Origen**: Decisión del orquestador (2026-10-07), validada por el PO, sobre la propuesta A2 aceptada por Rene Bonilla (2026-10-06).

## Reglas cubiertas

BR-AUTH-003 (observar exige confirmación humana; no por defecto) · BR-AUTH-001 · NFR-02 (sin shell, argv fijo) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRP-001 (añadir un repo). Independiente de US-GRP-020: si el clon cae en una raíz declarada y el desarrollador no lo acepta, sigue el camino normal de descubierto.
- **Transversal**: verificado en macOS; Linux y Windows: **Pendiente: etapa de validación multiplataforma**. Quién pide la acción (el humano o un agente) lo resuelve el motor; transversal (lo define el Arquitecto).

## Criterios de Aceptación

**Escenario: Clonar y aceptar la observación**

Dado una URL de un repo accesible para el desarrollador
Cuando ejecuta `raptor clone <url>` en su terminal y responde "s" a "¿Observar este repo?"
Entonces el repo queda clonado igual que con `git clone <url>`, sin que GitRaptor ejecute nada más ni cambie nada en él
  Y queda observado

**Escenario: La respuesta por defecto es no observar**

Dado una URL de un repo accesible
Cuando el desarrollador ejecuta `raptor clone <url>` y responde con Intro
Entonces el repo queda clonado
  Y no está observado

**Escenario: Sin terminal interactiva no se observa nada**

Dado `raptor clone <url>` lanzado desde un script sin terminal interactiva
Cuando termina el clon
Entonces el repo queda clonado, no está observado
  Y la salida dice cómo añadirlo

**Escenario: Un agente puede clonar pero no observar**

Dado una sesión de Claude Code con una terminal abierta
Cuando el agente ejecuta `raptor clone <url>` desde esa terminal
Entonces el repo queda clonado sin la pregunta de observar
  Y no está observado

**Escenario: Una URL que haría algo más que clonar se rechaza**

Dado una URL que empieza por guion o que usa un transporte que ejecuta comandos
Cuando el desarrollador ejecuta `raptor clone` con ella
Entonces se rechaza con el motivo antes de llamar a Git
  Y no se crea ninguna carpeta

**Escenario: Si Git falla, raptor clone falla igual**

Dado una URL que no existe o una carpeta de destino que ya existe y no está vacía
Cuando el desarrollador ejecuta `raptor clone <url>`
Entonces ve el error de Git y el comando termina con error
  Y no se pregunta ni se observa nada

## Requisitos Técnicos

> Arquitecto, 2026-10-07. Decisión del orquestador, validada por el Arquitecto.

- **`raptor clone <url> [<carpeta>]` vive en `apps/cli` y lanza el Git del sistema él mismo**, no el daemon. Usa el Git resuelto como en ADR-GRP-009 § 4 (ruta absoluta, ≥ 2.38), con argv fijo `clone -- <url> <carpeta>` y la entrada, la salida y los errores heredados de la terminal. Con eso funcionan el progreso y la petición de credenciales, y el resultado es el mismo que con `git clone`, configuración del usuario incluida (como el ejecutor, ADR-CKP-002 § 6). Un `raptor clone` no añade opciones de Git que el usuario no pidió.
- **La URL se valida antes de llamar a Git**: se rechazan las que empiezan por `-` y los transportes que ejecutan comandos (`ext::`, `fd::`). Además, el transporte `ext` se prohíbe en la propia invocación. Una URL rechazada no crea ninguna carpeta.
- **La carpeta de destino la calcula el CLI** con la misma regla que Git y se la pasa de forma explícita. Así sabe qué repo añadir sin leer la salida de Git. Si Git falla, `raptor clone` termina con el mismo código y no pregunta nada.
- **Solo pregunta** "¿Observar este repo? [s/N]" con una terminal interactiva y cuando el solicitante que resuelve el daemon (`hello.requester`) no es un agente. Con "s" llama a `repo.add`, comando reservado que el daemon vuelve a autorizar (SEC-03). En cualquier otro caso, imprime cómo añadirlo.
- **Responder "N" no es descartar**: si el clon cae en una raíz declarada, queda como descubierto (BR-AUTH-003, condición 6).
- **Verificación**, con repos y carpetas temporales (NFR-01) y sin red: clonar de un repo *bare* local por ruta; un corpus de URL hostiles; sin terminal interactiva; un agente simulado; y que se propague el error de Git.

## Diseño y Dev Spec

- **Diseño:** presentación en la CLI según DSYS-GRP-001.
- **Dev Spec:** pendiente.
