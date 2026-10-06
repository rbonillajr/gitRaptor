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
updated: 2026-10-05
related:
  adrs: [ADR-GRP-003, ADR-CKP-003, ADR-GRP-002]
  stories: [US-CKP-001, US-CKP-005]
  specs: [DS-TS-CKP-004]
ado:
  id: null
  url: null
tags: [cockpit, design-tokens, tema, simbolos, accesibilidad, nfr-09, dtcg, style-dictionary, br-04, paleta-a, terminal-clara, osc-11, colorfgbg]
---

## TS-CKP-004: Tokens semánticos y símbolos con fallback en el tema de la TUI

**Valor**: ninguna pantalla del Cockpit usa colores ni glifos literales, y todas se leen igual en truecolor, 256 colores, 16 colores, sin color y en ASCII.

### Descripción

**Como** Arquitecto
**Quiero** los tokens semánticos y los símbolos del design system definidos en `packages/design-tokens` y generados hacia `crates/theme`
**Para** que cada widget pida un significado y el tema resuelva su color, su glifo, su fallback y su anchura (ADR-GRP-003, DSYS-GRP-001 § 2, ADR-CKP-003 § 7 y § 10, NFR-09)

> Dev Spec: [`dev-specs/TS-CKP-004-tokens-semanticos-simbolos.md`](../dev-specs/TS-CKP-004-tokens-semanticos-simbolos.md) | Aprobada (2026-10-05)
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
- **Contraste**: en alto contraste, **gate WCAG AA ≥ 4.5:1** (truecolor y 256) para el texto sobre su fondo; se sube si DSYS-GRP-001 § 8 fija otro umbral. En el juego normal y en 16 colores, informe sin gate. (Decisión del orquestador (2026-10-05), validada por Arquitecto y PO: el alto contraste pinta su propio fondo, así que el cálculo es determinista; ver la Dev Spec § 5, D8.)

#### Verificación Manual / Sandbox

- Pintar la paleta y los símbolos en los emuladores de terminal de macOS, con fondo claro y oscuro, en truecolor, 256 y 16 colores.
- Anchura de `⚡`, `⛔`, `⚠` y `ℹ` en terminales de Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

### Enmienda (2026-10-05): paleta A y terminal clara

**Decisión de Rene Bonilla (2026-10-05)**: la paleta es la **opción A, «Grafito y teal»**; deja de ser provisional y cierra DSYS-GRP-001 § 8.1 y § 8.2. **Hallazgo que corrige**: en el juego normal la TUI hereda el fondo y el texto de la terminal, pero cada token tenía una sola variante pensada para fondo oscuro; en una terminal clara el foco amarillo y el agente 4 no se leían. Detalle en la [Dev Spec](../dev-specs/TS-CKP-004-tokens-semanticos-simbolos.md), § 8.

**Alcance que se añade**:
- **Definir** la variante para terminal clara de cada token semántico (truecolor, 256 y 16), con agentes Okabe-Ito oscurecidos.
- **Detectar** el fondo de la terminal: OSC 11 con timeout corto y sin bloquear, `COLORFGBG` como alternativa y oscura en último caso; override `--theme light|dark|high-contrast` y variable `GITRAPTOR_THEME`. `NO_COLOR` y el alto contraste no cambian.
- Sale del "fuera de alcance": la elección del acento y de la paleta.

**Verificación que se añade**:
- **Detección**: respuestas OSC 11 simuladas (formatos de 1 a 4 dígitos, BEL y ST, `rgba:`, mezcladas con otras teclas), `COLORFGBG`, sin respuesta, solo DA1, precedencia de flag y variable, `NO_COLOR` sin consulta; E/S contra una pseudoterminal (responde, no soporta OSC 11, muda con timeout y modo restaurado).
- **Contraste**: **gate WCAG AA ≥ 4.5:1** (truecolor y 256) para el texto normal y el tenue de las dos variantes sobre sus fondos típicos (blanco, `#1e1e1e`, Solarized oscuro), y para todo primer plano de la variante clara sobre blanco; informe del resto. Decisión del orquestador (2026-10-05), validada por Arquitecto.
- Pendiente: detección en Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

