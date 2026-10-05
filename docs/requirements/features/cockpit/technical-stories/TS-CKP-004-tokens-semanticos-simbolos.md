---
id: TS-CKP-004
title: "Tokens semánticos y símbolos con fallback en el tema de la TUI"
type: ts
status: draft
feature: cockpit
domain: GRP
priority: high
complexity: low
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-GRP-003, ADR-CKP-003, ADR-GRP-002]
  stories: [US-CKP-001, US-CKP-005]
  specs: []
ado:
  id: null
  url: null
tags: [cockpit, design-tokens, tema, simbolos, accesibilidad, nfr-09, dtcg, style-dictionary, br-04]
---

## TS-CKP-004: Tokens semánticos y símbolos con fallback en el tema de la TUI

**Valor**: ninguna pantalla del Cockpit usa colores ni glifos literales, y todas se leen igual en truecolor, 256 colores, 16 colores, sin color y en ASCII.

### Descripción

**Como** Arquitecto
**Quiero** los tokens semánticos y los símbolos del design system definidos en `packages/design-tokens` y generados hacia `crates/theme`
**Para** que cada widget pida un significado y el tema resuelva su color, su glifo, su fallback y su anchura (ADR-GRP-003, DSYS-GRP-001 § 2, ADR-CKP-003 § 7 y § 10, NFR-09)

> Dev Spec: `dev-specs/TS-CKP-004-tokens-semanticos-simbolos.md` | Pendiente
>
> **Depende de**: ninguna historia. Hoy `packages/design-tokens` solo tiene dos colores primitivos y `crates/theme` un tipo de color sin tokens. Las decisiones de DSYS-GRP-001 § 8 (color de acento, paleta de agentes validada para daltonismo, tema por defecto) siguen abiertas: los valores que use esta historia son provisionales y se marcan como ⚠️ **ASSUMPTION** en la Dev Spec. **ADRs**: ADR-GRP-003 (tokens DTCG y generación), ADR-CKP-003 § 7 (anchura por símbolo) y § 10 (tema agnóstico de la biblioteca de TUI). **Habilita**: todas las pantallas de BR-04 a BR-07, la CLI de solo lectura y la accesibilidad de NFR-09 (BR-CKP-EDGE-006).

### Alcance Técnico

- **Definir** en `packages/design-tokens` los tokens semánticos de DSYS-GRP-001 § 2.1 (texto, fondo, marca, estado, Git y los ocho colores de agente) sobre primitivos, cada uno con valor truecolor, índice de 256 colores y fallback de 16.
- **Definir** una variante de alto contraste del juego semántico.
- **Definir** los símbolos de DSYS-GRP-001 § 2.2 como tokens con glifo, fallback ASCII y anchura en columnas.
- **Configurar** la generación de tokens del stack (ADR-GRP-001) hacia `crates/theme` y un control de CI que falla si el código generado no coincide con los tokens.
- **Exponer** en `crates/theme` el tema por token semántico y por símbolo para cada modo (truecolor, 256, 16, sin color, alto contraste, ASCII), sin depender de la biblioteca de TUI.
- **Exponer** la asignación de color por agente, que reutiliza los colores a partir del noveno agente (BR-CKP-EDGE-006).
- **Fuera de alcance**: la elección final del acento y de la paleta (DSYS-GRP-001 § 8, decisión pendiente); la detección del modo desde flags, `NO_COLOR` y locale, que hace la TUI en la historia de accesibilidad; los widgets; las variables CSS de la Fase 3.

### Plan de Verificación

#### Pruebas Automatizadas

- **Generación**: el control de CI falla con un token editado sin regenerar `crates/theme`.
- **Completitud**: cada token semántico tiene sus tres profundidades y su valor de alto contraste; cada símbolo tiene glifo, fallback y anchura; todo fallback es ASCII puro.
- **Independencia**: el árbol de dependencias de `crates/theme` no incluye la biblioteca de TUI.
- **Agentes**: el agente noveno recibe el color del primero y conserva su nombre y su símbolo como señal distintiva.
- **Contraste**: comprobación del contraste de texto sobre fondo en alto contraste, con el umbral que fije DSYS-GRP-001 § 8; hasta entonces, informe sin gate.

#### Verificación Manual / Sandbox

- Pintar la paleta y los símbolos en los emuladores de terminal de macOS, con fondo claro y oscuro, en truecolor, 256 y 16 colores.
- Anchura de `⚡`, `⛔` y `⚠` en terminales de Linux y Windows: **Pendiente: etapa de validación multiplataforma**.
