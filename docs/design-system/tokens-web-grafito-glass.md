---
id: DSYS-GRP-002
title: "Tokens web del tema «Grafito glass» (app de escritorio)"
type: design-system
status: draft
version: 0.1
created: 2026-11-08
updated: 2026-11-08
owner: Rene Bonilla
parent: DSYS-GRP-001
related: [DSYS-GRP-001, ADR-GRP-003, ADR-GRP-004, DSF-DSK-001]
tags: [design-system, design-tokens, web, css, desktop, glass, theming, accessibility]
---

# Tokens web del tema «Grafito glass»

Anexo de [DSYS-GRP-001](README.md) (v0.10). Define los valores concretos de la UI web de la app de escritorio (Tauri + React, UI-1): superficies, texto, acento, estado, agente, bordes, radios, espaciado, tipografía, sombras y movimiento, en oscuro y claro. Cierra H-1 de [DSF-DSK-001](../requirements/features/desktop/design/design-flow.md).

**Origen.** **Decisión de Rene (2026-11-08)**: eligió la propuesta B1 del mock de la app (estilo «grafito glass», versión 21 del mock). Los valores salen de las variables CSS de ese mock; la fuente HTML es bitácora privada y no se copia al repo. Donde el mock no traía un valor, o fallaba el contraste, el valor lo fijó el diseñador y está marcado con **[nuevo]** (lista completa en § 10).

**Alcance.** Solo la UI web de la app (`apps/desktop`). **No cambia** los tokens de la TUI de § 2 de DSYS-GRP-001 (paleta A, «Grafito y teal»): el teal sigue siendo el acento de la TUI; el acento de la app es azul (decisión de Rene en B1: azul para lo accionable). Los agentes mantienen la paleta categórica Okabe-Ito de la paleta A.

## 1. Principios del estilo

| Principio | Consecuencia visual |
|---|---|
| La ventana flota | La ventana (radio 24 px) se apoya en un fondo suave (`surface.wall`) con margen de 14 px y sombra amplia |
| Cada zona es un panel | Barra lateral, centro y detalle son paneles grafito de radio 20 px separados por gutters de 12 px; no hay bordes entre zonas |
| Azul solo para lo accionable | `accent` marca botones, enlaces, foco y selección activa; nunca un estado |
| Verde y naranja son estado | Verde = éxito, protección activa, worktree vivo. Naranja = aviso y «en vivo» |
| Coral ✳ = agente de Claude | `agent.claude` solo marca a Claude Code; los carriles del grafo usan la paleta categórica |
| Tipografía del sistema | SF en macOS; etiquetas en minúscula normal (sin versalitas ni tracking) |
| El color nunca va solo | Cada estado lleva símbolo y texto (DSYS-GRP-001 § 2.2) |

## 2. Superficies, bordes y vidrio

Los nombres CSS llevan el prefijo `--gr-`. La columna «Mock» conserva el nombre de la variable de origen para trazar el valor.

| Token (DTCG) | CSS | Oscuro | Claro | Mock |
|---|---|---|---|---|
| `glass.surface.window` | `--gr-surface-window` | `#26282d` | `#e6e8ec` | `--bg-app` |
| `glass.surface.panel` | `--gr-surface-panel` | `#2f3237` | `#f2f3f5` | `--bg-sidebar` |
| `glass.surface.control` | `--gr-surface-control` | `#383b41` | `#ffffff` | `--bg-surface` |
| `glass.surface.raised` | `--gr-surface-raised` | `#3f4249` | `#f6f7f9` | `--bg-surface-elevated` |
| `glass.surface.hover` | `--gr-surface-hover` | `#464a52` | `#e9ebef` | `--bg-surface-hover` |
| `glass.surface.scrim` (fondo del Dialog) | `--gr-surface-scrim` | `rgba(0,0,0,0.55)` | `rgba(20,24,32,0.35)` | — **[nuevo, DS-TS-DSK-003]** |
| `glass.border.subtle` | `--gr-border-subtle` | `rgba(255,255,255,0.07)` | `rgba(20,24,32,0.07)` | `--border-subtle` |
| `glass.border.strong` | `--gr-border-strong` | `rgba(255,255,255,0.13)` | `rgba(20,24,32,0.13)` | `--border-strong` |
| `glass.highlight` | `--gr-highlight` | `inset 0 1px 0 rgba(255,255,255,0.05)` | `inset 0 1px 0 rgba(255,255,255,0.8)` | `--hi` |

**Uso por zona**

| Zona | Superficie |
|---|---|
| Ventana (detrás de los paneles y de la toolbar) | `surface.window` |
| Barra lateral, centro, cada bloque del detalle | `surface.panel` + `highlight`; sin borde |
| Botón, chip, campo, pestaña inactiva, píldora | `surface.control` + `highlight` |
| Pestaña activa del detalle, fila seleccionada | `surface.hover` + `highlight` |
| Menús y popovers | `surface.raised` + `shadow.popover` |

**Fondo suave (`surface.wall`)**

| Tema | Valor |
|---|---|
| Oscuro | `radial-gradient(1100px 620px at 8% 4%, rgba(92,150,190,0.55), transparent 62%), radial-gradient(900px 700px at 100% 100%, rgba(140,186,214,0.42), transparent 60%), #1a222b` |
| Claro | `radial-gradient(1100px 620px at 8% 4%, rgba(120,175,210,0.55), transparent 62%), radial-gradient(900px 700px at 100% 100%, rgba(236,222,200,0.7), transparent 60%), #c9dbe6` |

⚠️ **ASSUMPTION**: en el mock el fondo es parte de la página que simula el escritorio. En la app real es el fondo que pinta la propia ventana de Tauri (o un material translúcido de macOS); la elección es del Arquitecto (ADR-GRP-017 / `apps/desktop`). El token fija el aspecto; no prejuzga la técnica.

## 3. Texto

| Token | CSS | Oscuro | Claro | Mock | Contraste mínimo sobre `window`/`panel`/`control`/`raised`/`hover` |
|---|---|---|---|---|---|
| `glass.text.main` | `--gr-text-main` | `#eceef2` | `#1c1f25` | `--text-main` | oscuro 12.70 / 11.08 / 9.67 / 8.66 / 7.65; claro 13.46 / 14.87 / 16.51 / 15.40 / 13.83 |
| `glass.text.muted` | `--gr-text-muted` | `#b5b9c2` **[nuevo]** | `#5a606b` | `--text-muted` (`#a6abb5` oscuro) | oscuro 7.50 / 6.55 / 5.71 / 5.12 / 4.52; claro 5.16 / 5.70 / 6.32 / 5.90 / 5.30 |
| `glass.text.faint` | `--gr-text-faint` | `#a0a5ad` **[nuevo]** | `#63686f` **[nuevo]** | `--text-faint` (`#767c88` / `#8a909a`) | oscuro 5.96 / 5.20 / 4.53 / 4.06 / 3.59; claro 4.58 / 5.06 / 5.61 / 5.24 / 4.70 |
| `glass.text.disabled` | `--gr-text-disabled` | `#767c88` | `#8a909a` | `--text-faint` | Sin requisito: el texto deshabilitado está exento de WCAG 1.4.3. Solo glifos decorativos y controles deshabilitados |

- `text.muted` oscuro pasa de `#a6abb5` (mock) a `#b5b9c2`: con el original, el texto tenue sobre `raised` daba 4.37 y sobre `hover` 3.86.
- `text.faint` del mock (`#767c88`, 3.07 sobre `panel`) no llegaba a 4.5 en ningún fondo y se usa en texto real (contadores, «sin publicar»). Se sube para cumplir AA y el valor original pasa a `text.disabled`.
- Restricción: `text.faint` oscuro **no** va sobre `raised` ni sobre `hover` (4.06 y 3.59). Con el ajuste, `faint` y `muted` quedan cerca en oscuro: la jerarquía entre ambos la da también el tamaño y el peso (§ 7).
- `text.on-accent` (texto sobre un relleno `accent`): oscuro `#0b1f1c` (valor del mock) = 8.19:1; claro `#ffffff` **[nuevo]** = 4.91:1.

## 4. Acento, estado y agentes

Cada color de estado tiene dos tokens: el **relleno** (valor del mock, para puntos, íconos, bordes y fondos; exigencia no textual de 3:1) y el **texto** (`*.text`, ajustado para llegar a 4.5:1 en los cinco fondos de § 2).

| Token | CSS | Oscuro | Claro | Mock |
|---|---|---|---|---|
| `glass.accent` | `--gr-accent` | `#8ab4ff` | `#2a6be0` | `--accent-teal` (el nombre es heredado; el valor ya es azul) |
| `glass.accent.text` (enlaces, texto accionable) | `--gr-accent-text` | `#93baff` **[nuevo]** | `#2762ce` **[nuevo]** | — |
| `glass.accent.hover` (hover y pressed del Button primary) | `--gr-accent-hover` | `#a3c4ff` | `#1f5bc4` | — **[nuevo, DS-TS-DSK-003]** |
| `glass.accent.glow` | `--gr-accent-glow` | `rgba(138,180,255,0.14)` | `rgba(42,107,224,0.10)` | `--accent-teal-glow` |
| `glass.accent.border` | `--gr-accent-border` | `rgba(138,180,255,0.40)` | `rgba(42,107,224,0.35)` | `--accent-teal-border` |
| `glass.success` | `--gr-success` | `#5fd68e` | `#1d9a52` | `--status-success` |
| `glass.success.text` | `--gr-success-text` | `#5fd68e` | `#177840` **[nuevo]** | — |
| `glass.success.bg` | `--gr-success-bg` | `rgba(95,214,142,0.14)` | `rgba(29,154,82,0.10)` | `--status-success-bg` |
| `glass.warning` | `--gr-warning` | `#f5a524` | `#b8700a` | `--status-warning` |
| `glass.warning.text` | `--gr-warning-text` | `#f6aa2f` **[nuevo]** | `#955b08` **[nuevo]** | — |
| `glass.warning.bg` | `--gr-warning-bg` | `rgba(245,165,36,0.14)` | `rgba(184,112,10,0.10)` | `--status-warning-bg` |
| `glass.danger` | `--gr-danger` | `#ff8a94` | `#cf3a4c` | `--status-danger` |
| `glass.danger.text` | `--gr-danger-text` | `#ff9ea6` **[nuevo]** | `#be3546` **[nuevo]** | — |
| `glass.danger.bg` | `--gr-danger-bg` | `rgba(255,138,148,0.14)` | `rgba(207,58,76,0.10)` | `--status-danger-bg` |
| `glass.live` («en vivo», n.º de agentes) | `--gr-live` | `#f5a524` | `#c27406` | `--live` |
| `glass.live.text` | `--gr-live-text` | `#f6aa2f` **[nuevo]** | `#975a05` **[nuevo]** | — |
| `glass.live.bg` (fondo del Pill «en vivo») | `--gr-live-bg` | `rgba(245,165,36,0.14)` | `rgba(194,116,6,0.06)` | — **[nuevo, DS-TS-DSK-003]** |
| `glass.agent.claude` (✳) | `--gr-agent-claude` | `#e8775a` | `#c9583b` | `--claude` |
| `glass.agent.claude.text` | `--gr-agent-claude-text` | `#f0a794` **[nuevo]** | `#ab4b32` **[nuevo]** | — |

**Reglas de uso**

- Texto en un color de estado: `*.text`. Punto, ícono, borde o fondo tintado: relleno y `*.bg`.
- Relleno sobre `panel` / `control` (contraste no textual, mínimo 3:1): oscuro, todos ≥ 3.86 salvo `agent.claude` sobre `panel` 4.42 y sobre `control` 3.86; claro, `success` 3.26 / 3.62, `warning` 3.53 / 3.92, `danger` 4.34 / 4.81, `agent.claude` 3.83 / 4.25, `accent` 4.42 / 4.91. Sobre `window` en claro, `success` da 2.95: un punto de estado verde no va directamente sobre la ventana, solo sobre `panel` o `control`.
- Anillo de foco: `accent` con 2 px y desplazamiento de 2 px (valores del mock); ≥ 3:1 contra todas las superficies (oscuro ≥ 4.26; claro ≥ 4.00 sobre `window`).

**Carriles del grafo (paleta categórica de la paleta A, sin cambios)**

| Token | Oscuro | Claro | Mock |
|---|---|---|---|
| `glass.lane.base` | `#bc8cff` | `#8250df` | `--base` |
| `glass.lane.1` | `#e69f00` | `#a35f00` | `--a1` |
| `glass.lane.2` | `#56b4e9` | `#1f6fa8` | `--a2` |
| `glass.lane.3` | `#009e73` | `#00704f` | `--a3` |
| `glass.lane.4` | `#f0e442` | `#7a7200` | `--a4` |
| `glass.lane.5` | `#4fa3e0` | `#0b4f8a` | `--a5` |
| `glass.lane.6` | `#d55e00` | `#b83a00` | `--a6` |
| `glass.lane.7` | `#cc79a7` | `#a8457f` | `--a7` |

Son los mismos valores que `agent.1` … `agent.7` y `git.branch.base` de la TUI. Contraste no textual sobre `panel`: oscuro de 3.33 (`lane.6`) a 9.73 (`lane.4`); claro de 4.47 a 7.57. Sobre `control` en oscuro, `lane.3` y `lane.6` bajan a 3.28 y 2.90: **el carril y el punto de agente viven sobre `panel`, nunca sobre `control`**, y siempre llevan el nombre del agente al lado (WCAG 1.4.1). `agent.8` de la TUI (blanco/negro) no se usa en el grafo del mock: con más de 7 agentes el color se repite y el nombre y el símbolo siguen en la fila y el carril (US-CKP-002).

## 5. Radios, espaciado y dimensiones

**Radios**

| Token | CSS | Valor | Uso |
|---|---|---|---|
| `glass.radius.xs` | `--gr-radius-xs` | 6 px | Insignia del repo, botón de ícono pequeño, `kbd` |
| `glass.radius.sm` | `--gr-radius-sm` | 8 px | Campo, selector, botón de la vista de Time Machine |
| `glass.radius.md` | `--gr-radius-md` | 12 px | Botón de la toolbar, pestañas del detalle (10 px) y su contenedor (14 px) |
| `glass.radius.lg` | `--gr-radius-lg` | 20 px | Paneles |
| `glass.radius.window` | `--gr-radius-window` | 24 px | Ventana |
| `glass.radius.pill` | `--gr-radius-pill` | 999 px | Selector de repo, buscador, «Integrar cambios», píldoras |

Las filas del árbol usan 9 px y los grupos 7 px (valores observados en el mock). **[nuevo]**: se redondean a `radius.sm` (8) para no multiplicar radios; la diferencia de 1 px no se aprecia.

**Espaciado**

| Token | CSS | Valor | Origen |
|---|---|---|---|
| `glass.gutter` | `--gr-gutter` | 12 px | Hueco entre paneles (mock: `gap: 12px` del contenedor) |
| `glass.window.margin` | `--gr-window-margin` | 14 px | Margen de la ventana sobre el fondo; padding horizontal del contenedor de paneles |
| `glass.space.1` … `.6` | `--gr-space-1` … `-6` | 4, 8, 12, 16, 20, 24 px | **[nuevo]** escala de base 4 que cubre los valores del mock (4, 6, 8, 10, 12, 14, 16, 18). Los intermedios de 6, 10, 14 y 18 se redondean al escalón más cercano al implementar, salvo los 14 px de `window.margin` y del padding del detalle |

**Dimensiones** (valores observados en el mock; se afinan en el primer prototipo, § 2.4 del design flow)

| Elemento | Valor |
|---|---|
| Barra de título | 56 px de alto |
| Toolbar | 58 px de alto; botones centrados |
| Barra lateral | 252 px |
| Detalle (derecha) | 352 px |
| Cabecera del grafo | 38 px |
| Fila del grafo | 44 px |
| Columnas del grafo (por defecto) | Branch/Tag 195 px, Graph 175 px, Commit message flexible, Author 190 px, Date 120 px, Sha 84 px; se muestran u ocultan en Column settings (el mensaje siempre visible) |
| Fila del árbol: repo, rama, grupo | 30 px, 28 px, 24 px |
| Trazo de los carriles | 1.5 px |
| Objetivo mínimo de clic | 24 × 24 px (WCAG 2.5.8); los botones de acción, 32 px |
| Anillo de foco (grosor y separación; barra de selección) | 2 px, `glass.size.ring` → `--gr-size-ring` **[nuevo, DS-TS-DSK-003]** |
| Ancho del Dialog | 480 px, `glass.size.dialogWidth` → `--gr-size-dialog-width` **[nuevo, DS-TS-DSK-003]** |

**Capas (z)** **[nuevo, DS-TS-DSK-003]**: números sin unidad, iguales en oscuro y claro.

| Token | CSS | Valor | Capa |
|---|---|---|---|
| `glass.z.sticky` | `--gr-z-sticky` | 10 | Cabeceras fijas |
| `glass.z.drawer` | `--gr-z-drawer` | 20 | Cajón del detalle |
| `glass.z.popover` | `--gr-z-popover` | 40 | Menu y Popover |
| `glass.z.scrim` | `--gr-z-scrim` | 50 | Fondo del Dialog |
| `glass.z.dialog` | `--gr-z-dialog` | 51 | Dialog |
| `glass.z.toast` | `--gr-z-toast` | 60 | Toast |
| `glass.z.tooltip` | `--gr-z-tooltip` | 70 | Tooltip |

## 6. Sombras

| Token | CSS | Valor |
|---|---|---|
| `glass.shadow.window` | `--gr-shadow-window` | `0 40px 90px -30px rgba(0,0,0,0.6), 0 0 0 1px var(--gr-border-strong), var(--gr-highlight)` (igual en claro y oscuro, como en el mock) |
| `glass.shadow.panel` | `--gr-shadow-panel` | `var(--gr-highlight)`: los paneles no proyectan sombra |
| `glass.shadow.popover` oscuro | `--gr-shadow-popover` | `0 18px 40px rgba(0,0,0,0.45), 0 0 0 1px var(--gr-border-strong)` |
| `glass.shadow.popover` claro | `--gr-shadow-popover` | `0 18px 40px rgba(20,24,32,0.18), 0 0 0 1px var(--gr-border-strong)` |

## 7. Tipografía

| Token | CSS | Valor |
|---|---|---|
| `glass.font.ui` | `--gr-font-ui` | `-apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", system-ui, sans-serif` |
| `glass.font.mono` | `--gr-font-mono` | `ui-monospace, "SF Mono", "JetBrains Mono", Menlo, monospace` |

Sin fuente embebida en UI-1: la app usa la del sistema (macOS primero). Esto reemplaza la nota de ADR-GRP-003 («Inter o Geist; JetBrains Mono o Geist Mono») para la app; la enmienda corresponde al Arquitecto. La mono cae a `JetBrains Mono` solo si está instalada.

| Rol | CSS | Tamaño / peso | Observado en |
|---|---|---|---|
| `type.title` | `--gr-type-title` | 17 px / 600, tracking -0.01em | Título de vista (Time Machine) |
| `type.body-strong` | `--gr-type-body-strong` | 13.5 px / 500 | Repo del árbol, CTA «Integrar cambios», filas |
| `type.body` | `--gr-type-body` | 13 px / 400 | Ramas, pestañas del detalle (500), cabeceras de panel (500) |
| `type.secondary` | `--gr-type-secondary` | 12.5 px / 400–500 | Cabecera del grafo, filtros, subtítulos |
| `type.meta` | `--gr-type-meta` | 12 px / 400 | Contadores, «sin publicar», grupos |
| `type.caption` | `--gr-type-caption` | 11.5 px / 500 | Etiquetas de grupo, ahead/behind |
| `type.mono` | `--gr-type-mono` | 12 px / 400 (11.5 en ahead/behind) | SHA, ramas, comandos |

- Etiquetas en minúscula normal: `text-transform: none`, `letter-spacing: 0` (la excepción es `type.title`).
- Números con `font-variant-numeric: tabular-nums` (contadores, ↑↓, hora).
- ⚠️ **ASSUMPTION**: `line-height` no está en el mock. **[nuevo]** Propuesta: 1.4 en texto de párrafo y 1 en filas de altura fija. Se ajusta en el prototipo.

## 8. Movimiento

| Token | Valor | Origen |
|---|---|---|
| `glass.motion.fast` | `150ms ease` | Giro del chevron del árbol (mock); rango de ADR-GRP-003 «Sistema de movimiento» (150–250 ms) |
| `prefers-reduced-motion: reduce` | Sin transiciones ni animación del pulso de «en vivo» | **[nuevo]**; WCAG 2.3.3 y § 7 del design flow |

## 9. Salida CSS y tema

`@gitraptor/tokens` (`packages/design-tokens`) genera este bloque desde un DTCG JSON nuevo, `tokens/glass.json`, junto a `color.json`, `semantic.json` y `symbol.json`; no toca los tres existentes. **[nuevo]**: el nombre del archivo y el prefijo `--gr-` son del diseñador; el Arquitecto los confirma en la enmienda de ADR-GRP-003.

Selección de tema (ADR-GRP-003, enmienda D7): `auto` sigue `prefers-color-scheme`; la preferencia manual del usuario fija `data-theme="dark"` o `data-theme="light"` en `:root`. El mock parte de oscuro; R-2 del design flow fija `auto` como predeterminado.

```css
:root {
  color-scheme: dark;
  --gr-surface-window: #26282d;
  --gr-surface-panel: #2f3237;
  --gr-surface-control: #383b41;
  --gr-surface-raised: #3f4249;
  --gr-surface-hover: #464a52;
  --gr-border-subtle: rgba(255, 255, 255, 0.07);
  --gr-border-strong: rgba(255, 255, 255, 0.13);
  --gr-highlight: inset 0 1px 0 rgba(255, 255, 255, 0.05);
  --gr-text-main: #eceef2;
  --gr-text-muted: #b5b9c2;
  --gr-text-faint: #a0a5ad;
  --gr-text-disabled: #767c88;
  --gr-text-on-accent: #0b1f1c;
  --gr-accent: #8ab4ff;
  --gr-accent-text: #93baff;
  --gr-accent-glow: rgba(138, 180, 255, 0.14);
  --gr-accent-border: rgba(138, 180, 255, 0.4);
  --gr-success: #5fd68e;
  --gr-success-text: #5fd68e;
  --gr-success-bg: rgba(95, 214, 142, 0.14);
  --gr-warning: #f5a524;
  --gr-warning-text: #f6aa2f;
  --gr-warning-bg: rgba(245, 165, 36, 0.14);
  --gr-danger: #ff8a94;
  --gr-danger-text: #ff9ea6;
  --gr-danger-bg: rgba(255, 138, 148, 0.14);
  --gr-live: #f5a524;
  --gr-live-text: #f6aa2f;
  --gr-agent-claude: #e8775a;
  --gr-agent-claude-text: #f0a794;
  --gr-lane-base: #bc8cff;
  --gr-lane-1: #e69f00;
  --gr-lane-2: #56b4e9;
  --gr-lane-3: #009e73;
  --gr-lane-4: #f0e442;
  --gr-lane-5: #4fa3e0;
  --gr-lane-6: #d55e00;
  --gr-lane-7: #cc79a7;
  --gr-surface-wall: radial-gradient(1100px 620px at 8% 4%, rgba(92, 150, 190, 0.55), transparent 62%), radial-gradient(900px 700px at 100% 100%, rgba(140, 186, 214, 0.42), transparent 60%), #1a222b;
  --gr-shadow-window: 0 40px 90px -30px rgba(0, 0, 0, 0.6), 0 0 0 1px var(--gr-border-strong), var(--gr-highlight);
  --gr-shadow-panel: var(--gr-highlight);
  --gr-shadow-popover: 0 18px 40px rgba(0, 0, 0, 0.45), 0 0 0 1px var(--gr-border-strong);
  --gr-radius-xs: 6px;
  --gr-radius-sm: 8px;
  --gr-radius-md: 12px;
  --gr-radius-lg: 20px;
  --gr-radius-window: 24px;
  --gr-radius-pill: 999px;
  --gr-gutter: 12px;
  --gr-window-margin: 14px;
  --gr-space-1: 4px;
  --gr-space-2: 8px;
  --gr-space-3: 12px;
  --gr-space-4: 16px;
  --gr-space-5: 20px;
  --gr-space-6: 24px;
  --gr-font-ui: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", system-ui, sans-serif;
  --gr-font-mono: ui-monospace, "SF Mono", "JetBrains Mono", Menlo, monospace;
  --gr-motion-fast: 150ms ease;
}

/* Claro: los mismos tokens se repiten, con estos valores, en
   :root[data-theme="light"] y en
   @media (prefers-color-scheme: light) { :root:not([data-theme="dark"]) { … } } */
:root[data-theme="light"] {
  color-scheme: light;
  --gr-surface-window: #e6e8ec;
  --gr-surface-panel: #f2f3f5;
  --gr-surface-control: #ffffff;
  --gr-surface-raised: #f6f7f9;
  --gr-surface-hover: #e9ebef;
  --gr-border-subtle: rgba(20, 24, 32, 0.07);
  --gr-border-strong: rgba(20, 24, 32, 0.13);
  --gr-highlight: inset 0 1px 0 rgba(255, 255, 255, 0.8);
  --gr-text-main: #1c1f25;
  --gr-text-muted: #5a606b;
  --gr-text-faint: #63686f;
  --gr-text-disabled: #8a909a;
  --gr-text-on-accent: #ffffff;
  --gr-accent: #2a6be0;
  --gr-accent-text: #2762ce;
  --gr-accent-glow: rgba(42, 107, 224, 0.1);
  --gr-accent-border: rgba(42, 107, 224, 0.35);
  --gr-success: #1d9a52;
  --gr-success-text: #177840;
  --gr-success-bg: rgba(29, 154, 82, 0.1);
  --gr-warning: #b8700a;
  --gr-warning-text: #955b08;
  --gr-warning-bg: rgba(184, 112, 10, 0.1);
  --gr-danger: #cf3a4c;
  --gr-danger-text: #be3546;
  --gr-danger-bg: rgba(207, 58, 76, 0.1);
  --gr-live: #c27406;
  --gr-live-text: #975a05;
  --gr-agent-claude: #c9583b;
  --gr-agent-claude-text: #ab4b32;
  --gr-lane-base: #8250df;
  --gr-lane-1: #a35f00;
  --gr-lane-2: #1f6fa8;
  --gr-lane-3: #00704f;
  --gr-lane-4: #7a7200;
  --gr-lane-5: #0b4f8a;
  --gr-lane-6: #b83a00;
  --gr-lane-7: #a8457f;
  --gr-surface-wall: radial-gradient(1100px 620px at 8% 4%, rgba(120, 175, 210, 0.55), transparent 62%), radial-gradient(900px 700px at 100% 100%, rgba(236, 222, 200, 0.7), transparent 60%), #c9dbe6;
  --gr-shadow-popover: 0 18px 40px rgba(20, 24, 32, 0.18), 0 0 0 1px var(--gr-border-strong);
}
```

El claro solo lista lo que cambia respecto al oscuro; radios, espaciado, tipografía, movimiento, `shadow.window` y `shadow.panel` no cambian y no se repiten. La salida generada emite el bloque claro dos veces (selector `data-theme` y `@media`), porque CSS no permite compartirlo.

## 10. Contraste y valores que no venían del mock

**Gate de contraste.** El gate de contraste de `crates/theme` (WCAG AA, 4.5:1 texto) se extiende a `glass.json`: cada token `text.*` y `*.text` contra `surface.window`, `panel`, `control`, `raised` y `hover` (con la excepción de `text.faint` oscuro, § 3), y cada relleno no textual contra `panel` y `control` a 3:1. Los valores de este documento se comprobaron con la fórmula de luminancia relativa de WCAG 2.x sobre los hex opacos; las superficies son opacas, así que el cálculo no depende del fondo.

Pendiente del gate: los tintes `*.bg` (alfa de 0.10–0.14) y su texto encima. El texto sobre `success.bg` y compañeros se comprueba cuando existan los componentes (UI-1), porque depende de la superficie que haya debajo.

**Valores inventados o ajustados por el diseñador (no venían del mock, o el mock fallaba AA):**

| Valor | Por qué |
|---|---|
| `text.muted` oscuro `#b5b9c2` | El mock (`#a6abb5`) daba 4.37 sobre `raised` y 3.86 sobre `hover` |
| `text.faint` `#a0a5ad` / `#63686f` | El mock (`#767c88` / `#8a909a`) no llegaba a 4.5 en ningún fondo; el valor del mock pasa a `text.disabled` |
| `accent.text` `#93baff` / `#2762ce` | El relleno claro `#2a6be0` da 4.00 sobre `window` y 4.11 sobre `hover` |
| `success.text`, `warning.text`, `danger.text`, `live.text`, `agent.claude.text` en claro, y `warning.text`, `danger.text`, `live.text`, `agent.claude.text` en oscuro | Los rellenos del mock se quedaban entre 2.95 y 4.34 en claro y entre 3.86 y 4.36 en oscuro sobre `hover` o `raised` |
| `text.on-accent` claro `#ffffff` | El mock fija `#0b1f1c` (pensado para el teal) en el botón de commit; sobre el azul claro `#2a6be0` daría menos de 4.5 |
| Escala `space.1`–`6`, `radius.xs`, redondeo de 9 y 7 px a 8 px | El mock usa valores sueltos |
| `line-height`, `prefers-reduced-motion` | No están en el mock |
| Nombres DTCG (`glass.*`) y prefijo CSS `--gr-` | Los nombres del mock (`--bg-app`, `--accent-teal`…) reflejan la paleta anterior (el acento seguía llamándose «teal» siendo azul) |
| Archivo `tokens/glass.json` | Sin él no hay dónde generar la salida CSS |

**Decisiones abiertas para el Arquitecto** (enmienda de ADR-GRP-003): técnica del fondo `surface.wall` en Tauri (§ 2), tipografía del sistema frente a Inter/Geist (§ 7), prefijo y archivo de tokens (§ 9).
