---
id: US-CKP-005
title: "La TUI se puede usar en una terminal pequeña, sin color o en ASCII"
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
    - US-CKP-001
tags:
  - cockpit
  - accesibilidad
  - i18n
  - layout
  - must
---

# US-CKP-005: La TUI se puede usar en una terminal pequeña, sin color o en ASCII

## Descripción

**Como** desarrollador orquestador, **quiero** usar la TUI en una terminal de 80×24, sin color, en ASCII, en inglés o en español y solo con el teclado, **para** no depender de mi terminal ni de distinguir colores para entender la flota.

**Valor**: NFR-09 y NFR-10 (accesibilidad e i18n) aplicados a BR-04.

## Reglas cubiertas

BR-CKP-EDGE-005 · BR-CKP-EDGE-006 (el color nunca es la única señal) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-CKP-001. El reparto con alertas y grafo se verifica a medida que entran US-CKP-008 y US-CKP-022.
- **Técnicas**: TS-CKP-004 (símbolos con fallback ASCII), INF-CKP-001.

## Criterios de Aceptación

**Escenario: Por debajo de 80×24, solo el aviso de tamaño mínimo**

Dado una terminal de 70×20
Cuando el desarrollador abre la TUI
Entonces la TUI muestra solo "Amplía la terminal a 80×24 como mínimo"
  Y al ampliarla a 80×24 aparece la lista sin reiniciar

**Escenario: El espacio se reparte por prioridad**

Dado una terminal de 80×24 con lista, alertas, grafo y detalle activos
Cuando no caben todos
Entonces se reduce primero el grafo, después el detalle, y la lista y las alertas siempre se ven

**Escenario: Sin color o en ASCII se entiende igual**

Dado `NO_COLOR` definido o la opción `--ascii`
Cuando el desarrollador mira la lista
Entonces cada estado se distingue por su símbolo o su equivalente ASCII, explicado en la ayuda, nunca solo por color

**Escenario: Idioma del usuario**

Dado el idioma del sistema en español
Cuando el desarrollador abre la TUI
Entonces todos los textos de la TUI están en español; con el idioma en inglés, en inglés

**Escenario: Teclado primero, con ayuda**

Dado la TUI abierta
Cuando el desarrollador pulsa "?"
Entonces la TUI muestra las teclas disponibles en el panel actual, y cada acción se puede lanzar sin ratón

## Requisitos Técnicos

- El aviso de tamaño mínimo conserva el tamaño actual y añade "Amplía la terminal a 80×24 como mínimo". El reparto por prioridad ya existe en `layout()` y se prueba ahí; alertas y grafo los cablean US-CKP-008 y US-CKP-022.
- En ASCII, todo texto del catálogo que pinta la vista se pliega con `Glyphs::fold` (un único punto de paso, `Say`). La ayuda `?` se filtra por el panel actual y explica los símbolos; la barra de teclas explica los estados en pantalla cuando cabe.
- En Windows, el idioma del sistema es el de la interfaz del usuario: el escenario 4 queda pendiente allí (etapa de validación multiplataforma) y la historia, `partially-implemented` hasta entonces.
- Detalle, decisiones D1 a D10 y plan: [DS-US-CKP-005](../dev-specs/US-CKP-005-terminal-pequena-sin-color.md).

## Diseño y Dev Spec

- **Diseño:** DSYS-GRP-001 (tokens, símbolos, contenido); ADR-GRP-004 § 3 (teclado primero).
- **Dev Spec:** [DS-US-CKP-005](../dev-specs/US-CKP-005-terminal-pequena-sin-color.md). Pendiente: etapa de validación multiplataforma (terminales de Linux y Windows).
