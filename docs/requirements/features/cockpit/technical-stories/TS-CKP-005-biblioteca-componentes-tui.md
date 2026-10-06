---
id: TS-CKP-005
title: "Biblioteca de componentes TUI v0: los 10 widgets del design system, con snapshots y galería"
type: ts
status: draft
feature: cockpit
domain: GRP
priority: high
complexity: medium
created: 2026-10-05
updated: 2026-10-05
related:
  adrs: [ADR-CKP-003, ADR-GRP-003, ADR-GRP-002]
  stories: [TS-CKP-004, INF-CKP-001, US-CKP-001, US-CKP-002, US-CKP-003, US-CKP-004, US-CKP-005, US-CKP-006, US-CKP-007, US-CKP-008, US-CKP-009, US-CKP-012, US-CKP-013, US-CKP-014, US-CKP-015, US-CKP-016, US-CKP-017, US-CKP-018, US-CKP-019, US-CKP-020, US-CKP-021, US-CKP-022, US-CKP-023, US-CKP-024]
  specs: [DS-TS-CKP-005]
ado:
  id: null
  url: null
tags: [cockpit, tui, ratatui, componentes, widgets, design-system, snapshots, insta, galeria, accesibilidad, nfr-09, sec-12, br-04]
---

## TS-CKP-005: Biblioteca de componentes TUI v0

**Valor**: las historias del Cockpit componen pantallas con widgets ya probados en todos los modos de color y de símbolos, en vez de dibujar cada una los suyos.

### Descripción

**Como** Arquitecto
**Quiero** los 10 componentes de DSYS-GRP-001 § 3 como widgets puros de `apps/cli`, con snapshots en cada modo y una galería navegable
**Para** que cada US-CKP solo arme su vista con el modelo de vista del componente, y que la accesibilidad (NFR-09), el saneado por tipo (SEC-12) y la adaptación desde 80×24 ya vengan resueltos (ADR-CKP-003 § 7, § 8 y § 10)

> Dev Spec: [`dev-specs/TS-CKP-005-biblioteca-componentes-tui.md`](../dev-specs/TS-CKP-005-biblioteca-componentes-tui.md) | Aprobada (2026-10-05)
>
> **Origen**: decisión de Rene (2026-10-05): los componentes se construyen como biblioteca antes de las historias del Cockpit. Ninguna TS los construía. **Depende de**: TS-CKP-004 (tokens y símbolos, ya en main). **Sobre**: INF-CKP-001 (ya en main: bucle, cliente, `present::SafeText`, catálogo i18n, `keymap` y target de biblioteca), cuya vista compone estos widgets con la convención de esta TS. **ADRs**: ADR-CKP-003 § 7 (layout), § 8 (`SafeText`), § 10 (tema y accesibilidad), § 12 (módulos de `apps/cli`, sin crates nuevos).

### Alcance Técnico

- **Implementar** en `apps/cli/src/tui/widgets/` los 10 componentes: Layout (regiones, StatusBar y pantalla de tamaño mínimo), AgentList/AgentRow, GraphLanes, DiffView, TimelineList, ConflictAlert, PolicyBanner, ConfirmPrompt, Notification (toast) y KeyHints/Help.
- **Definir** la convención de widget que usa INF-CKP-001: modelo de vista propio por componente, trait `Component` y envoltorio `themed()` que implementa `ratatui::widgets::Widget`.
- **Crear** `tui::style`: el tema de `crates/theme` mapeado a `ratatui::style`, glifos estructurales con juego ASCII y utilidades puras (`follow`, `modal_area`, recorte con elipsis, ajuste de líneas).
- **Usar** el `SafeText` de INF-CKP-001 (`present::SafeText`, reexportado como `model::SafeText`): todo texto de un widget es `SafeText`, así que el saneado SEC-12 se impone por tipo.
- **Probar** cada componente y estado con snapshots de `insta` sobre `Buffer` en truecolor, 256, 16, NO_COLOR, alto contraste y ASCII.
- **Crear** la galería `raptor ui gallery`: subcomando oculto, navegable con el teclado, con `--dump` para capturarla en texto.
- **Fuera de alcance**: el bucle de la app, el canal, las pantallas y el `keymap` (INF-CKP-001 y US-CKP). También la detección del modo desde flags, `NO_COLOR` y locale (US-CKP-005), la caducidad de 5 s del toast (update de INF-CKP-001) y los textos del catálogo i18n de producción.

### Componentes y las historias que los usan

Mapeo validado por el PO (2026-10-05). TS-CKP-005 es dependencia de todas estas historias.

| Componente | Historias |
|---|---|
| Layout / StatusBar | US-CKP-001, 003, 004, 005, 019, 022 |
| AgentList / AgentRow | US-CKP-001, 002, 003, 016 |
| GraphLanes | US-CKP-005, 022 |
| DiffView | US-CKP-012, 016 |
| TimelineList | US-CKP-016, 021 |
| ConflictAlert | US-CKP-004, 006, 007, 008, 009 |
| PolicyBanner | US-CKP-019, 020, 023 |
| ConfirmPrompt | US-CKP-013 a 020, 023, 024 |
| Notification | US-CKP-014 a 018, 021, 024 |
| KeyHints / Help | Todas las de la TUI (todas menos US-CKP-010 y 011) |

### Plan de Verificación

#### Pruebas Automatizadas

- **Snapshots**: un snapshot por componente y estado (45 estados). Cada uno contiene el texto con símbolos Unicode y sus tramos de estilo en truecolor, 256, 16, NO_COLOR y alto contraste, más el texto y los estilos en ASCII. El test falla si un modo de color cambia el texto. El primer estado de cada componente y todos los de Layout fijan además el juego ASCII en cada modo de color. Layout se prueba en 80×24, 100×30, 120×40 y 79×24.
- **Nada solo por color**: sin color, dos estados de un mismo componente nunca se ven iguales, ni con símbolos Unicode ni en ASCII.
- **ASCII**: el juego ASCII solo pinta ASCII.
- **Accesibilidad**: el foco tiene forma (borde grueso y marcador `›` o `>`). ConfirmPrompt empieza en No. KeyHints conserva siempre la ayuda `?`. La cabecera muestra siempre los recuentos de ⚡ y ⛔ (L-05). Por debajo de 80×24 solo se pinta el mensaje de tamaño mínimo con la salida. El noveno agente reutiliza el color del primero y conserva su nombre.
- **Fronteras (ADR-CKP-003 V5)**: los widgets no importan `gitraptor_core`, `gitraptor_git`, `gitraptor_policy` ni `gitraptor_api`. No usan colores literales, glifos fuera de comentarios ni textos de usuario. Las fronteras de toda la TUI (motor, Git, políticas, procesos) las comprueba `tests/tui_boundaries.rs` de INF-CKP-001.
- **Galería**: navegación pura (cursor por componente, estado y modo) y `--dump` con todas las historias.

#### Verificación Manual / Sandbox

- `raptor ui gallery` en los emuladores de terminal de macOS, recorriendo los componentes y los modos con `m`.
- Anchura real de `⚡`, `⛔`, `⚠` y `ℹ` y la galería en las consolas de Linux y Windows: **Pendiente: etapa de validación multiplataforma**.
