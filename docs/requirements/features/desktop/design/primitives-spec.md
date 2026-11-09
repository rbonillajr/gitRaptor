---
title: Primitivas del design system B1 (especificación visual y de interacción)
status: draft
updated: 2026-11-09
related: TS-DSK-003
---

# Primitivas del design system B1

Especificación de las 14 primitivas de `apps/desktop/src/ui` para el tema «grafito glass» (claro y oscuro), más `CopyCommand` (§ 2.16), que ya existía en el código y se especifica aquí desde la enmienda del 2026-11-09. Una primitiva se implementa leyendo su sección y §1; ninguna decisión visual queda abierta. Lo que falta en los tokens está en §3 y bloquea solo lo que allí se indica.

**Fuentes.** Tokens: `packages/design-tokens/css/glass.css` (único origen de valores; utilidades Tailwind de `apps/desktop/src/styles/app.css` donde existen, `bg-(--gr-x)` donde no). Contexto: [design-flow.md](design-flow.md) § 2.2, 4.b, 5, 7, 8; [DSYS-GRP-002](../../../../design-system/tokens-web-grafito-glass.md); ADR-GRP-003 (Radix + Tailwind v4 + CVA, «Definición de diseño de componentes») y ADR-GRP-004 (§ 3 UX). Estilo existente: `Panel.tsx` y `TitleBar.tsx` (`BUTTON`). Mock B1: la herramienta `Artifact read` lo devolvió truncado (se cortó dentro del CSS); leí el HTML guardado y de ahí salen **solo** el contenido del menú Fetch y de Column settings y los anchos de columna de § 2.14. Sus colores son los de la paleta previa; mandan los de `glass.css`.

**Convenciones.** «action» = `--gr-size-target-action` (32 px); «min» = `--gr-size-target-min` (24 px). Todas las alturas, radios, tipos y colores son tokens `--gr-*`. Las únicas constantes sin token (2 px de anillo y de barra de selección) están en §3.

## 1. Reglas transversales

### 1.1 Anillo de foco único

`:focus-visible { outline: 2px solid var(--gr-accent); outline-offset: 2px }` en toda primitiva interactiva (igual que `TitleBar`). Contraste del anillo ≥ 3:1 contra las cinco superficies: oscuro ≥ 4.26, claro ≥ 4.00 (calculado).
- Variante **interior** `outline-offset: -2px` en filas (TreeRow, TableRow), ítems de Menu y cualquier elemento dentro de un contenedor con `overflow` que recortaría el anillo.
- Es `outline`, no `box-shadow`: sobrevive a `forced-colors` y no lo recorta el `--gr-highlight`.
- Ningún componente quita el outline sin sustituirlo. El foco por ratón no lo muestra (`:focus-visible`).

### 1.2 Receta de estados (aplica salvo que la primitiva diga otra cosa)

| Estado | Regla |
|---|---|
| reposo | Lo define cada primitiva |
| hover | Fondo `--gr-surface-hover`; texto e icono pasan a `--gr-text-main` |
| foco visible | §1.1; no cambia el fondo |
| pressed | Igual que hover y sin `--gr-highlight` (hundido). Ver `--gr-surface-pressed` en §3 |
| disabled | `aria-disabled="true"` (nunca solo `disabled`: sigue enfocable y el Tooltip con el motivo funciona, design-flow § 2.2 «se desactivan con motivo»). Texto e icono `--gr-text-disabled`, sin hover ni pressed, `cursor: not-allowed`, **sin `opacity`**. El clic no hace nada |
| error | Solo Button peligro, Pill, Toast y filas con error; ningún otro control tiene estado de error en B1 |
| pending | `aria-busy="true"` + `aria-disabled="true"`; el motivo lo escribe el llamador (ADR-GRP-004 § 3, «esperando la autenticación del sistema») |

Claro y oscuro: ninguna primitiva ramifica por tema; `glass.css` cambia los valores (`:root[data-theme="light"]` y `prefers-color-scheme`). Prohibido `dark:` de Tailwind y hex literales.

### 1.3 Movimiento

Toda transición usa `--gr-motion-fast` (150 ms ease) y solo sobre `background-color`, `color`, `border-color`, `opacity` y el giro del chevron. Sin transformaciones de escala ni desplazamientos. Con `prefers-reduced-motion: reduce` no hay transición ni animación. Se resuelve **una vez** en `app.css`: `@media (prefers-reduced-motion: reduce) { :root { --gr-motion-fast: 0s } }`, más `animation: none` en los overlays de Radix. El cambio de estado sigue ocurriendo, solo que sin transición. No existe pulso de «en vivo» en B1 (el punto es estático; evita inventar una duración sin token).

### 1.4 Contraste verificado (WCAG 2.2 AA)

Calculado con la luminancia relativa de WCAG sobre los hex de `glass.css`; los tintes `*-bg` y `accent-glow` se compusieron (alfa) sobre la superficie indicada. Texto ≥ 4.5, no textual ≥ 3. Cada fila es una regla de la que dependen las secciones siguientes.

| # | Par | Oscuro / claro | Resultado y regla |
|---|---|---|---|
| C1 | `text-on-accent` sobre `accent` / sobre `accent-text` | 8.19 · 8.73 / 4.91 · 5.64 | Pasa. Primary y su hover |
| C2 | `accent-text` (enlaces) sobre window…hover | ≥ 4.53 / ≥ 4.60 | Pasa en las cinco superficies |
| C3 | `text-muted` sobre window…hover | ≥ 4.52 / ≥ 5.16 | Pasa; hover oscuro es el mínimo (4.52) |
| C4 | `text-faint` | 3.59–5.96 / 4.58–5.61 | **Falla** sobre raised y hover en oscuro. Ninguna primitiva lo usa para texto; se usa como no textual (C9) |
| C5 | `*-text` sobre su `*-bg` compuesto | success 3.80–5.99, warning 3.64–5.73, danger 3.71–5.83 / 3.96–4.93 | **Falla** en raised/hover (oscuro) y en panel/window/hover (claro; en oscuro warning también en control, 4.45). Prohibido texto `*-text` sobre `*-bg`. Pill: etiqueta en `text-main` (≥ 5.98 / ≥ 12.10) y solo el glifo en `*-text` |
| C6 | `accent-text` sobre `accent-glow` compuesto | 3.61–4.99 / 4.07–4.93 | **Falla** (oscuro control 4.41, raised, hover; claro panel 4.47). Filas, chips y segmentos seleccionados usan `text-main` (≥ 6.09 / ≥ 11.92) |
| C7 | `text-muted` sobre `accent-glow` compuesto | 3.60–5.67 / 4.57–5.54 | Pasa solo sobre window/panel. **La fila seleccionada vive solo sobre `panel`** (4.98 / 5.02) |
| C8 | `danger-text` sobre `surface-control`; con capa `danger-bg` encima | 5.72 · 4.53 / 5.55 · 4.81 | Pasa. Botón peligro (hover) |
| C9 | Toggle apagado: borde y pulgar `text-faint` vs superficies | ≥ 3.59 / ≥ 4.58 | Pasa (≥ 3). Pulgar encendido `text-on-accent` sobre `accent` 8.19 / 4.91 |
| C10 | `accent` (anillo, barra, indicadores) vs superficies | ≥ 4.26 / ≥ 4.00 | Pasa |
| C11 | `text-on-accent` sobre `lane-1,2,3,4,5,7` | ≥ 5.00 / ≥ 4.96 | Pasa. **`lane-6` oscuro da 4.43: fuera del RepoBadge** |
| C12 | Rellenos de estado como punto/glifo sobre `panel` / `control` | success claro 3.26 / 3.62; `danger` 4.34 / 4.81 | Pasa (≥ 3). Sobre `window` claro `success` da 2.95 y `live` 2.95: **StatusDot nunca sobre `window`** |
| C13 | `lane-3` y `lane-6` oscuro sobre control (3.28, 2.90) | — | Fondo del RepoBadge solo sobre `panel` |
| C14 | `text-main` sobre `raised` / `text-muted` sobre `raised` | 8.66 · 5.12 / 15.40 · 5.90 | Pasa. Menu, Popover, Dialog, Toast, Tooltip |
| C15 | `text-disabled` | 2.12–3.52 / 2.62–3.21 | Exento de 1.4.3 (controles deshabilitados); no se usa para información que no esté en otro sitio |

Sin hover distinto no hay pares nuevos que probar: ninguna primitiva compone un par fuera de esta tabla.

### 1.5 Capas (z-index)

Overlays de Radix en portal sobre `body`. Sin token hoy (§3, `--gr-z-*`); estos son los valores de la propuesta.

| Capa | z-index | Notas |
|---|---|---|
| Contenido, grafo | 0 | |
| TableHeader pegajoso | 10 | Dentro del centro |
| Cajón de detalle (< 1000 px) | 20 | Nunca tapa el foco (design-flow § 7) |
| Popover y Menu | 40 | Un Menu abierto desde un Dialog sube a 52 (`data-in-dialog`); en UI-1 no hay |
| Dialog: scrim / contenido | 50 / 51 | Modal: resto `inert` (Radix `aria-hidden`) |
| Toast (viewport) | 60 | Encima del Dialog: no toma foco y es `aria-live` |
| Tooltip | 70 | Siempre encima, también dentro de un Dialog o un Toast |

### 1.6 Objetivo mínimo (WCAG 2.5.8)

| Primitiva | Alto del objetivo |
|---|---|
| Button md, IconButton md, SegmentedControl (contenedor), Menu item, Chip con cierre | action (32 px) |
| Button sm, IconButton sm, Chip interactivo, segmento, Toggle (alto del área de clic), cierre del Toast, TreeRow grupo | min (24 px) |
| TreeRow repo / rama, TableRow | 30 / 28 / 44 px (`--gr-size-tree-*`, `--gr-size-graph-row`) |
| Pill, StatusDot, RepoBadge, Tooltip, Chip estático | No interactivos: sin mínimo |

El enlace dentro de un párrafo queda exento; un `Button variant="link"` aislado mide al menos min de alto.

### 1.7 `Esc` consumido

**Enmienda aditiva (2026-11-09), Decisión del orquestador (2026-11-09), validada por Arquitecto** (DS-US-DSK-008 D18, T006; DSF-DSK-001 D-16). Un `Esc` cierra una sola capa. La primitiva o vista que lo consume llama `event.preventDefault()` en ese `keydown`: `Dialog` (lo escucha en `document`), `Popover` (en su superficie), `Toast` (en su elemento) y las vistas del centro (en su `onKeyDown`). Los atajos de ventana (`useShortcuts`) no actúan sobre un `Esc` con `defaultPrevented`. Orden de capas: diálogo, popover, toast con foco, cajón del detalle, vista del centro.

## 2. Primitivas

Convención de las tablas: «tipo» = `font: var(--gr-type-*)`; `pad-x` = padding horizontal.

### 2.1 Button

Props: `variant: "primary" | "secondary" | "danger" | "link"`, `size: "md" | "sm"`, `shape?: "pill"` (solo la CTA «Integrar cambios»), `pending`. Un solo botón `primary` por vista. Lo que relaja una protección es `secondary`, nunca `primary` ni el que recibe el foco (P2).

| Variante | Reposo (fondo · borde · texto) | Hover | Pressed |
|---|---|---|---|
| primary | `accent` · ninguno · `text-on-accent` | fondo `accent-text` (C1; interino, §3) | = hover |
| secondary | `surface-control` + `--gr-highlight` · 1 px `border-strong` · `text-main` | fondo `surface-hover` | = hover sin highlight |
| danger | `surface-control` · 1 px `danger` · `danger-text` (C8) | capa `danger-bg` sobre `surface-control` | = hover sin highlight |
| link | transparente · ninguno · `accent-text` (C2) | subrayado | texto `text-main`, subrayado |
| disabled | primary: fondo `surface-hover`; secondary/danger: `surface-control` y borde `border-subtle`; link: transparente. Texto `text-disabled`, sin subrayado | — | — |

| Propiedad | md | sm |
|---|---|---|
| Alto | action | min |
| `pad-x` / gap | `space-3` / `space-2` | `space-2` / `space-1` |
| Tipo | `type-body-strong` | `type-secondary` |
| Radio | `radius-md` (`radius-pill` con `shape="pill"`) | `radius-sm` |
| Icono | 16 px (`space-4`), `currentColor`, trazo 1.5 | igual |

ARIA y teclado: `<button type="button">`; Enter y Espacio activan; `aria-disabled` según §1.2; `pending` añade `aria-busy`. Si navega fuera de la app se renderiza `<a>` con el mismo estilo. Motion: §1.3 sobre fondo y texto. Error: la variante `danger` es el estado de error; no hay variante `danger` rellena en B1 (P2: lo destructivo no es lo prominente).

### 2.2 IconButton

Cuadrado action (`size="md"`) o min (`"sm"`); radio `radius-md` (sm: `radius-sm`); icono 16 px.

| Variante | Reposo | Hover | Activo (`aria-pressed` / `aria-expanded` = true) |
|---|---|---|---|
| default (barra de título) | `surface-control` + `--gr-highlight`, icono `text-muted` | `surface-hover`, icono `text-main` | icono `accent-text` |
| ghost (toolbar, engranaje) | transparente, icono `text-muted` | igual que default | igual |

Pressed y disabled según §1.2. Es el `BUTTON` de `TitleBar` extraído sin cambios de aspecto. `aria-label` obligatorio por tipo (`label: string`); `aria-expanded`/`aria-controls`/`aria-haspopup` los pasa el llamador. El Tooltip repite la etiqueta y añade el atajo.

### 2.3 Chip

Estático (`<span>`, p. ej. `HEAD`, `↑4`, `sin publicar`) o interactivo (`<button>`, filtro) con cierre opcional.

| Propiedad | Valor |
|---|---|
| Alto | estático `space-5` (20 px); interactivo min (24 px); con cierre action no se usa: el cierre es un IconButton sm dentro del chip |
| `pad-x`, gap, radio | `space-2`, `space-1`, `radius-pill` |
| Tipo | `type-caption`; datos (hash, `↑4`) `type-mono` |
| Reposo | fondo `surface-control`, borde 1 px `border-strong`, texto `text-muted` |
| Hover (interactivo) | fondo `surface-hover`, texto `text-main` |
| Seleccionado (`aria-pressed="true"`) | fondo `accent-glow`, borde 1 px `accent`, texto `text-main` (C6), con ✓ delante (el color no va solo) |
| Foco / pressed / disabled | §1.1 / §1.2 |

ARIA: estático sin rol; interactivo `button` con `aria-pressed`; cierre `aria-label` = `ui.chip.remove`. Teclado: Enter/Espacio alterna; en un chip con cierre, Supr o Retroceso lo quitan. Solo sobre `panel` o `control`.

### 2.4 Pill de estado

Informa, no se pulsa (el botón de conflictos de la barra de título es un Button). Alto `space-5`, `pad-x` `space-2`, gap `space-1`, `radius-pill`, tipo `type-caption`. Etiqueta siempre visible, glifo de 12 px (`space-3`).

| Variante | Fondo | Glifo (color) | Etiqueta (`text-main`, C5) | Símbolo (Lucide; hoy SVG en línea como `TitleBar`) |
|---|---|---|---|---|
| reversible | `success-bg` | `success-text` | `ui.pill.reversible` | `Undo2` (⟲ recuperable, P7) |
| pide confirmación | `warning-bg` | `warning-text` | `ui.pill.confirm` | `CircleAlert` |
| no se puede deshacer | `danger-bg` | `danger-text` | `ui.pill.irreversible` | `OctagonX` |
| en vivo | `warning-bg` (interino, `live-bg` ausente, §3) | StatusDot `live` | `ui.pill.live` | punto, sin glifo |

ARIA: `role="status"` solo si aparece o cambia en pantalla (aria-live polite); si es estático, texto plano. «En vivo» es un dato del motor: no se pinta si el motor no lo confirma (P6). Un Pill de color sin etiqueta es un error de tipo. Sin hover/pressed/disabled. Motion: ninguna.

### 2.5 StatusDot

Caja de `space-2` (8 px), `inline-block`, no interactiva.

| `tone` | Forma | Color | Etiqueta (`ui.status.*`) |
|---|---|---|---|
| active | círculo lleno | `success` | Activo |
| inactive | anillo de `--gr-size-lane-stroke` (1.5 px), interior transparente | `text-muted` | Inactivo |
| conflict | rombo lleno (`clip-path` de 4 puntos) | `danger` | Conflicto previsto |
| live (solo para Pill) | círculo lleno | `live` | En vivo |

La forma distingue el estado sin color (WCAG 1.4.1) y el nombre va en texto al lado. `role="img"` + `aria-label`; con `decorative` (texto adyacente ya lo dice) va `aria-hidden`. Solo sobre `panel` o `control` (C12). Sin estados interactivos ni movimiento.

### 2.6 RepoBadge

Cuadrado `space-5` (20 px), `radius-xs`, inicial en mayúscula del repo, tipo `type-caption`, `aria-hidden` (el nombre del repo siempre está al lado).

| Parte | Token |
|---|---|
| Fondo | `lane-N` con N elegido por `hash(repoId) mod 6` sobre la lista `[1, 2, 3, 4, 5, 7]` |
| Inicial | `text-on-accent` (C11) |

Excluidos: `lane-base` (es la línea de la base en el grafo) y `lane-6` (C11). `lane-2` y `lane-5` son azules cercanos; con más de 6 repos el color se repite y la inicial más el nombre desambiguan. Solo sobre `panel` (C13). No interactiva. ⚠️ **ASSUMPTION**: color estable por `repoId`, no por orden de alta, para que no cambie al borrar un repo.

### 2.7 SegmentedControl

Un grupo de opciones excluyentes (p. ej. pestañas del detalle si se usan como filtro, tema).

| Parte | Reposo | Hover | Seleccionado |
|---|---|---|---|
| Contenedor | alto action, fondo `surface-control`, `pad` `space-1`, `radius-md`, `--gr-highlight` | — | — |
| Segmento | alto min, `pad-x` `space-3`, `radius-sm`, tipo `type-secondary`, texto `text-muted` | fondo `surface-hover`, texto `text-main` | fondo `accent-glow`, borde 1 px `accent`, texto `text-main` (C6, C10) |

Disabled y pressed según §1.2 (por segmento o del grupo). ARIA: contenedor `role="radiogroup"` con nombre; segmentos `role="radio"` + `aria-checked`. Teclado (APG radio group): una sola parada de Tab (el seleccionado); Flechas izquierda/derecha y arriba/abajo mueven **y seleccionan**, con vuelta al extremo; Home/End primero/último; Espacio selecciona. Motion: §1.3 sobre fondo y borde.

### 2.8 Toggle (switch)

| Parte | Apagado | Encendido |
|---|---|---|
| Pista | 32 × 20 px (`size-target-action` × `space-5`), `radius-pill`, fondo `surface-control`, borde 1 px `text-faint` | fondo y borde `accent` |
| Pulgar | 12 px (`space-3`), `text-faint`, a `space-1` del borde | `text-on-accent`, desplazado `space-3` |
| Área de clic | alto min (24 px) y la etiqueta entera es clicable | igual |

Hover: borde de la pista `text-muted` (apagado) / sin cambio (encendido). Foco: §1.1 sobre la pista. Disabled: borde y pulgar `text-disabled`, etiqueta `text-disabled`. Contraste C9. Tipo de la etiqueta `type-body`, gap `space-2`; descripción opcional `type-meta` `text-muted` enlazada con `aria-describedby`.
ARIA: `role="switch"` + `aria-checked`, `<label>` asociada. Teclado: Espacio y Enter alternan. El estado se anuncia por el propio switch; no añadir texto «activado». Motion: el pulgar desliza y la pista cambia de color con `--gr-motion-fast` (§1.3).

### 2.9 TreeRow

Fila del árbol de la barra lateral (repo → Worktrees / Ramas locales / Remotas → filas). Contenedor `role="tree"` con `aria-label` (caller), filas `role="treeitem"`, hijos en `role="group"`.

| Propiedad | repo | worktree / rama / remota | grupo |
|---|---|---|---|
| Alto | `size-tree-repo` (30 px) | `size-tree-branch` (28 px) | `size-tree-group` (24 px) |
| Tipo | `type-body-strong` | `type-body` | `type-caption`, texto `text-muted` |
| Texto | `text-main` | `text-main` | `text-muted` |

Anatomía y medidas comunes: `pad-x` `space-2`; sangría por nivel `space-4` (16 px); chevron en caja de `space-4`, gira 90° al abrir con `--gr-motion-fast` (hoja: espaciador de la misma caja); luego `RepoBadge` (repo) o `StatusDot` (worktree) y la etiqueta con elipsis; a la derecha un slot de metadatos (`type-mono`, `text-muted`, nunca `text-faint`, C4), cuenta ⚡ en `danger-text`, conteo de agentes en `live-text`. Línea guía: 1 px vertical `border-subtle` en el centro del chevron de cada ancestro (`nivel × space-4 + space-2 + 8 px`), decorativa. Radio de la fila `radius-sm`, margen horizontal `space-2`.

| Estado | Fondo | Otros |
|---|---|---|
| reposo | transparente | |
| hover | `surface-hover` | texto `text-main` |
| seleccionado | `accent-glow` | barra interior izquierda de 2 px `accent`, texto `text-main`; no cambia en hover (C6, C7) |
| foco | anillo interior (§1.1) | independiente de la selección |
| disabled (p. ej. «no disponible: la carpeta ya no existe») | transparente | `aria-disabled`, texto `text-muted` (informa, no es `text-disabled`), sin acciones |
| loading | 3–6 barras de la altura de fila, fondo `surface-hover`, `radius-sm`, **sin animación**; `aria-busy` en el árbol | texto sr-only `ui.tree.loading` |

ARIA por fila: `aria-level`, `aria-setsize`, `aria-posinset` (obligatorios con virtualización), `aria-expanded` solo en padres, `aria-selected` (selección única). El nombre accesible es la frase completa que pasa el llamador (design-flow § 7: «claude-1, activo, rama feat-pagos…»); chevron e iconos `aria-hidden`.
Teclado (APG tree, foco itinerante: una sola parada de Tab, la fila enfocada o la seleccionada):

| Tecla | Acción |
|---|---|
| Abajo / Arriba | Siguiente / anterior fila visible |
| Derecha | Padre cerrado: abre. Padre abierto: primer hijo. Hoja: nada |
| Izquierda | Padre abierto: cierra. Cerrado u hoja: va al padre |
| Inicio / Fin | Primera / última fila visible |
| Enter | Activa: selecciona y ejecuta la acción de la fila (abre detalle o centra el grafo); en repo y grupo también alterna |
| Espacio | Selecciona sin activar |
| `*` | Abre todos los hermanos del nivel |
| Carácter imprimible | Salto por el comienzo de la etiqueta (type-ahead, 500 ms) |

La selección no sigue al foco (cada selección abre un detalle, es costosa). El clic en el chevron alterna sin seleccionar; el clic en la fila selecciona y, en repo y grupo, alterna.

### 2.10 Menu

Menú de acciones (`DropdownMenu` de Radix). Contenido del menú Fetch (mock B1, `role=menuitemradio`; título «Elige la operación que ejecuta este botón»): Fetch de todos los remotos · Pull (fast-forward si se puede) · Pull (solo fast-forward) · Pull (rebase). Las claves de esas etiquetas son de la toolbar (`toolbar.fetch.*`), no `ui.*`. ⚠️ **ASSUMPTION**: en UI-1 solo «Fetch» está activo (design-flow H-10); las tres de Pull salen `aria-disabled` con el motivo «Disponible en UI-2» hasta que el PO decida.

| Parte | Especificación |
|---|---|
| Contenedor | fondo `surface-raised`, `radius-md`, `--gr-shadow-popover` (ya incluye el borde), `pad-y` `space-2`, ancho `fit-content` entre `size-sidebar` y `size-detail`, z 40, separado del disparador `space-2` |
| Título de grupo | `type-caption`, `text-muted`, `pad-x` `space-3` |
| Ítem | alto action, `pad-x` `space-3`, gap `space-2`, tipo `type-body`, `text-main`, radio `radius-sm` con margen `space-1` |
| Ítem resaltado (`data-highlighted`) | fondo `surface-hover`; con teclado, anillo interior (§1.1) |
| Ítem disabled | `aria-disabled`, `text-disabled`; el motivo en su Tooltip |
| Radio | círculo de 14 px, anillo 1.5 px `text-muted`; marcado: anillo y punto `accent` (C10) |
| Casilla | 16 px, `radius-xs`, anillo 1.5 px `text-muted`; marcada: fondo `accent`, check `text-on-accent` |
| Pista a la derecha | `type-meta`, `text-muted` (atajos con `Kbd` fuera de B1: texto) |
| Separador | 1 px `border-subtle`, margen `space-1` |

ARIA: `role="menu"`, ítems `menuitem` / `menuitemradio` / `menuitemcheckbox` con `aria-checked`. El disparador (Fetch partido: botón principal + IconButton con el chevron) lleva `aria-haspopup="menu"` y `aria-expanded`. Teclado: en el disparador, Enter, Espacio o Abajo abren con el primer ítem enfocado; Arriba abre con el último. Dentro: Arriba/Abajo (con vuelta), Inicio/Fin, type-ahead, Enter/Espacio activan, Escape cierra y devuelve el foco al disparador, Tab cierra. Un ítem radio cierra el menú; uno de casilla lo deja abierto. Motion: opacidad `--gr-motion-fast` al abrir y cerrar; con reduced motion, ninguna. Posición: `bottom-start`, `collisionPadding` `--gr-window-margin`.

### 2.11 Popover

Panel no modal con controles (Radix `Popover`). Caso: **Column settings** (engranaje de la cabecera). Mismo contenedor que Menu (`surface-raised`, `radius-md`, `--gr-shadow-popover`, z 40), `pad` `space-3`, ancho `size-sidebar`, gap vertical `space-1`.

Contenido de Column settings (mock B1): título «Column settings» (`type-body-strong`); una fila por columna con su Toggle (§ 2.8, etiqueta a la izquierda): Branch / Tag, Graph, Commit message (encendido y `aria-disabled`, pista «Siempre visible» en `type-meta` `text-muted`; se usa `text-muted` y no `text-disabled` porque es información), Author, Date / Time, Sha; al pie un `Button variant="link" size="sm"` «Restablecer». B1 usa Toggle y no la casilla del mock porque no hay primitiva Checkbox en este alcance.

ARIA: contenido `role="dialog"` con `aria-label` = el título; disparador IconButton ghost con `aria-haspopup="dialog"`, `aria-expanded`, `aria-controls`. Teclado: al abrir, el foco va al primer Toggle; Tab recorre los Toggle y el enlace sin trampa de foco (no modal); Escape cierra y devuelve el foco al engranaje; clic fuera cierra. Los cambios se aplican al instante y se guardan por usuario; no hay «Aplicar». Posición: `bottom-end`, separación `space-2`. Motion: igual que Menu.

### 2.12 Toast

Aviso temporal, no modal (Radix `Toast`). Contenedor `surface-raised`, `radius-md`, `--gr-shadow-popover`, `pad` `space-3` `space-4`, ancho máximo `size-detail`, gap `space-2`; fila: glifo de estado (16 px) · mensaje (`type-body`, `text-main`) · acción opcional (`Button variant="secondary" size="sm"`) · cierre (IconButton ghost sm, `ui.common.close`).

| Tipo | Glifo | Rol / anuncio | Cierre automático |
|---|---|---|---|
| success | ✔ `success-text` | `role="status"`, polite | 5 s |
| info | ninguno | `role="status"`, polite | 5 s |
| warning | `warning-text` | `role="status"`, polite | 5 s |
| error | `danger-text` | `role="alert"`, assertive | no; queda hasta cerrarlo (queda además en el centro de avisos) |

**Prefijo oculto** (**Enmienda aditiva (2026-11-09), Decisión del orquestador (2026-11-09), validada por Arquitecto**, DS-US-DSK-007 D15): delante del mensaje, un texto `sr-only` dice el tipo (`ui.toast.kind.<kind>`; `info` no lleva). Prop opcional `srLabel` lo sustituye por un texto de la pantalla (p. ej. «Bloqueo» en el toast de un bloqueo de Guardrails); sin ella, el de siempre.

Un toast con acción («Rehacer», «Deshacer», «Ver») sigue los 5 s del design-flow. El temporizador se pausa con hover, con foco dentro y con la ventana sin foco (WCAG 2.2.1). Escape cierra el toast enfocado. Máximo 3 visibles; el resto en cola. El viewport es una región con `aria-label` = `ui.toast.region` y atajo F8 para llevar el foco (Radix).
Posición: abajo al centro, margen `--gr-window-margin`. Un toast **nunca recibe foco automático** ni tapa el elemento enfocado (design-flow § 7): si en `focusin` el rectángulo del elemento enfocado cruza el del viewport, el viewport pasa a arriba al centro, bajo `size-titlebar` + `size-toolbar`, mientras dure ese foco. Motion: opacidad `--gr-motion-fast`; con reduced motion, aparece y desaparece sin transición.

### 2.13 Dialog

Modal (Radix `Dialog`; `kind="confirm"` usa `role="alertdialog"`). Scrim a pantalla completa (token ausente, §3), z 50; contenido z 51, centrado.

| Parte | Especificación |
|---|---|
| Contenedor | fondo `surface-raised`, `radius-lg`, `--gr-shadow-popover` + `--gr-highlight`, `pad` `space-6`, gap `space-4`, ancho `size-dialog-width` (§3), máx `100% − 2 × --gr-window-margin` |
| Título | `type-title`, `text-main`, enlazado con `aria-labelledby` |
| Descripción | `type-body`, `text-muted` (C14), `aria-describedby`; obligatoria por tipo |
| Pie | botones alineados a la derecha, gap `space-2`, Cancelar **antes** en el DOM que la acción |
| Cierre (X) | IconButton ghost sm arriba a la derecha; `showClose` por defecto `true` en `info`, `false` en `confirm` |

Foco y teclado (design-flow § 5, D-3, P2): el foco inicial va a **Cancelar** en `confirm` y al botón seguro («Entendido») en `info`; se marca con `data-autofocus` y se pasa a `onOpenAutoFocus`. Enter activa el botón enfocado: con el foco en Cancelar, Enter **no confirma**. Tab y Shift+Tab ciclan dentro (trampa de foco). Escape cierra y el foco vuelve a quien abrió (`onCloseAutoFocus` al disparador; si lo abrió código, a la referencia guardada). `dismissible={false}` (p. ej. d3, esperando el factor del sistema) ignora Escape y clic fuera, pero Cancelar sigue existiendo. El scroll del fondo se bloquea y el resto de la app queda `inert`.
Variantes visuales: `confirm` con acción peligrosa usa `Button variant="danger"`; con acción reversible, `secondary`; nunca `primary` para lo que cambia algo (P2). Motion: opacidad del scrim y del contenido con `--gr-motion-fast`; con reduced motion, sin transición.

### 2.14 Tooltip

Etiqueta flotante (Radix `Tooltip`). Fondo `surface-raised`, `--gr-shadow-popover`, `radius-sm`, `pad` `space-1` `space-2`, tipo `type-secondary`, `text-main` (C14), ancho máximo `size-sidebar`, sin flecha, z 70, separación `space-2`, `collisionPadding` `--gr-window-margin`.

| Aspecto | Regla |
|---|---|
| Apertura | Foco de teclado: inmediata. Hover: 500 ms; grupos contiguos (toolbar): 300 ms entre uno y otro |
| Cierre | Escape, salir con el puntero (el contenido es «hoverable», WCAG 1.4.13), perder el foco, o al activar el disparador |
| Contenido | Texto plano, una o dos líneas; atajo al final en `type-mono`. Nunca controles ni enlaces |
| Relación | IconButton: el Tooltip repite `aria-label` (no se anuncia dos veces: el disparador no recibe `aria-describedby` si el texto es igual). Control `aria-disabled`: el Tooltip es el motivo y se enlaza con `aria-describedby` |
| Glosario | Los términos de Git usan los textos de design-flow § 12 |

Motion: opacidad `--gr-motion-fast`; con reduced motion, sin transición (el retardo de apertura se mantiene). Sin estados propios.

### 2.15 TableHeader y TableRow

Grilla del grafo con columnas configurables. Contenedor `role="grid"` con `aria-rowcount` y `aria-colcount` (filas virtualizadas, 50 visibles más «N commits más»). Plantilla de columnas por CSS grid (`grid-template-columns`) que construye el llamador a partir de la configuración; anchos del mock B1, constantes de la pantalla del grafo y no tokens:

| Columna | Ancho | Ocultable |
|---|---|---|
| Branch / Tag | 195 px | sí |
| Graph | 175 px | sí |
| Commit message | `minmax(220px, 1fr)` | no (siempre visible) |
| Author | 190 px | sí |
| Date / Time | 120 px | sí |
| Sha | 84 px | sí |

**Plegado** (**Enmienda aditiva (2026-11-09), Decisión del orquestador (2026-11-09), validada por Arquitecto**; sustituye a la regla anterior, que plegaba por debajo de 1000 px de ventana en el orden Sha, Date / Time, Author, Branch / Tag; design-flow D-18 y H-17; DS-US-DSK-005 D11 y T004). Se mide el ancho del contenedor de la grilla, no el de la ventana: debe alojar las columnas visibles fijas más 220 px de Commit message. Si no caben, se ocultan, en este orden y sin tocar la preferencia guardada: **Sha, Author, Date / Time, Branch / Tag**; Graph y Commit message nunca se pliegan. Mientras Author diga «Tú u otro (sin atribuir)» en cada fila repite el mismo texto, y Date es el dato útil. Referencias: centro de 788 px (1440 con detalle) → Branch, Graph, Message y Date; 708 px (1360 con detalle) → Branch, Graph y Message; 1152 px (1440 sin detalle) → las seis. La grilla solo se desplaza en horizontal si no cabe Graph más 220 px. Column settings marca cada columna plegada con «No cabe con este ancho» y conserva su interruptor.

| Parte | Especificación |
|---|---|
| TableHeader | alto `size-graph-header` (38 px), `position: sticky` z 10, fondo transparente sobre `panel`, borde inferior 1 px `border-subtle`, celdas `role="columnheader"` con `pad-x` `space-3`, tipo `type-secondary`, `text-muted`. Sin orden ni redimensión en B1 (no son botones). Al final, el IconButton ghost sm del engranaje (Popover de Column settings), absoluto a `space-2` del borde derecho; `aria-label` = `ui.table.settings` |
| TableRow | alto `size-graph-row` (44 px), `role="row"` con celdas `role="gridcell"`, `pad-x` `space-3` por celda |
| Texto de celda | principal `type-body` `text-main`; secundario (subtítulo del autor, fecha, sha) `type-meta` o `type-mono`, `text-muted` (no `text-faint`: C4) |

| Estado de fila | Fondo | Otros |
|---|---|---|
| reposo | transparente | |
| hover | `surface-hover` solo en las celdas de texto (Message, Author, Date, Sha); la celda Graph no se pinta para no ocultar los carriles | |
| seleccionada | `accent-glow` | barra interior izquierda de 2 px `accent`; solo sobre `panel` (C7); texto `text-main` |
| foco | anillo interior (§1.1) | |
| loading | barras `surface-hover`, `radius-sm`, sin animación | `aria-busy` en el grid |
| motor parado o desconectado | todo `text-muted`, sin hover | `aria-disabled` en el grid; design-flow § 2.3 |
| fila WIP | `accent-glow`, borde del mensaje discontinuo | |

Teclado (APG data grid con foco por fila): una sola parada de Tab; Arriba/Abajo mueven de fila; Inicio/Fin y Ctrl+Inicio/Ctrl+Fin primera y última; RePág/AvPág salto de página; Enter selecciona y abre el detalle; Derecha entra en el primer elemento enfocable de la fila (p. ej. la insignia ⚡) y entre celdas; Izquierda o Escape vuelven a la fila; Enter sobre ⚡ abre el conflicto (design-flow § 5). Vista alternativa en tabla (`Cmd+Opción+T`) es otra pantalla, no esta primitiva. Motion: fondo con `--gr-motion-fast`.

### 2.16 CopyCommand

**Enmienda aditiva (2026-11-09), Decisión del orquestador (2026-11-09), validada por Arquitecto** (DS-US-DSK-008 D22, T005; design-flow H-16). Un comando en una caja `<code>` (`surface-control`, `radius-md`, `type-mono`, `text-main`, `break-all`) con un `Button variant="secondary" size="sm"` «Copiar» (`engine.action.copy`) que escribe con `navigator.clipboard.writeText` desde el clic, sin plugin ni permiso de Tauri. El llamador garantiza que el texto es seguro para pegar en una shell (lo construye el backend). Nunca lee el portapapeles.

| Estado | Botón | Anuncio |
|---|---|---|
| reposo | «Copiar» | — |
| copiado | «Copiado» (`ui.common.copied`) con ✔ durante 2 s; luego vuelve a «Copiar» | una vez, en una región `role="status"` `sr-only` presente desde el montaje (WCAG 4.1.3) |
| fallo | «Copiar» | `ui.common.copyFailed` en la misma región |

Prop opcional `onCopied?: (command: string) => void`, llamada solo tras un `writeText` correcto (la usa Guardrails para recordar qué excepción se copió, DS-US-DSK-007 D12). El temporizador se limpia al desmontar. Con reduced motion, sin transición en el ✔.

## 3. Tokens ausentes

Ningún valor se inventa en componentes: cada fila está resuelta con un token existente (interino) salvo la del scrim.

| Falta | Para qué | Propuesta mínima (oscuro / claro) | Interino |
|---|---|---|---|
| `--gr-surface-scrim` | Fondo del Dialog | `rgba(0, 0, 0, 0.55)` / `rgba(20, 24, 32, 0.35)` | **Sin interino: bloquea Dialog.** El mock usaba `rgba(0,0,0,0.75)` con desenfoque |
| `--gr-size-ring` | Grosor y separación del anillo de foco, barra de selección | `2px` | 2 px literal documentado en §1.1 y §2.9 |
| `--gr-accent-hover` (y `--gr-accent-pressed`) | Hover y pressed del primary | oscuro un poco más claro que `#8ab4ff`, claro más oscuro que `#2a6be0`, con `text-on-accent` ≥ 4.5 | `--gr-accent-text` (C1; se mueve en la dirección correcta en ambos temas) |
| `--gr-surface-pressed` | Estado pressed | entre `surface-hover` y `surface-raised` | `surface-hover` sin `--gr-highlight` |
| `--gr-live-bg` | Fondo del Pill «en vivo» | `rgba(245, 165, 36, 0.14)` / `rgba(194, 116, 6, 0.10)` | `--gr-warning-bg` (tono casi igual; lo distinguen el punto y la etiqueta) |
| `--gr-z-sticky/popover/scrim/dialog/toast/tooltip` | Capas | 10 / 40 / 50 / 51 / 60 / 70 (§1.5) | Valores de §1.5 |
| `--gr-size-dialog-width` | Ancho del Dialog | `480px` | `min(32rem, 100% - 2 * var(--gr-window-margin))` |
| Selector de tema acotado | Dos temas a la vez en el catálogo | `[data-theme="light"]` además de `:root[data-theme="light"]` en el generador | Dos iframes (§5) |

Huecos de catálogo (no son tokens): la **toolbar** del mock usa botones verticales icono sobre etiqueta y un botón partido (Fetch). Ninguna es una de las 14 primitivas: se especifican como `Button` con `variant="ghost"` + `stacked` y `SplitButton` = Button + IconButton + Menu cuando se implemente la toolbar. Además, Radix, CVA, `tailwind-merge` y Lucide figuran en ADR-GRP-003 pero **no están** en `apps/desktop/package.json`; añadirlos es decisión de la historia de implementación.

## 4. Claves i18n (`ui.*`, en/es; ambos idiomas en el mismo PR, `catalog.test.ts` exige paridad)

| Clave | es | en | Quién la usa |
|---|---|---|---|
| `ui.common.close` | Cerrar | Close | IconButton de cierre (Dialog, Toast) |
| `ui.common.cancel` | Cancelar | Cancel | Pie del Dialog `confirm` |
| `ui.common.loading` | Cargando… | Loading… | Texto sr-only de cargas |
| `ui.common.copied` | Copiado | Copied | CopyCommand: botón 2 s y región `role="status"` (§ 2.16) |
| `ui.common.copyFailed` | No se pudo copiar. Selecciona el comando y cópialo a mano. | Couldn't copy. Select the command and copy it by hand. | CopyCommand: región `role="status"` (§ 2.16) |
| `ui.button.busy` | En curso | In progress | Button `pending` (sr-only) |
| `ui.chip.remove` | Quitar {{label}} | Remove {{label}} | Cierre del Chip |
| `ui.pill.reversible` | Recuperable | Recoverable | Pill |
| `ui.pill.confirm` | Pide confirmación | Needs confirmation | Pill |
| `ui.pill.irreversible` | No se puede deshacer | Can't be undone | Pill |
| `ui.pill.live` | En vivo | Live | Pill |
| `ui.status.active` | Activo | Active | StatusDot |
| `ui.status.inactive` | Inactivo | Inactive | StatusDot |
| `ui.status.conflict` | Conflicto previsto | Predicted conflict | StatusDot |
| `ui.status.live` | En vivo | Live | StatusDot |
| `ui.tree.loading` | Cargando repositorios… | Loading repositories… | TreeRow loading |
| `ui.toast.region` | Avisos | Notifications | Viewport del Toast |
| `ui.toast.kind.success` | Hecho | Done | Prefijo sr-only |
| `ui.toast.kind.warning` | Aviso | Warning | Prefijo sr-only |
| `ui.toast.kind.error` | Error | Error | Prefijo sr-only |
| `ui.tooltip.shortcut` | Atajo: {{keys}} | Shortcut: {{keys}} | `aria-keyshortcuts` y texto del atajo |
| `ui.table.settings` | Ajustes de columnas | Column settings | Engranaje de la cabecera |
| `ui.table.loading` | Cargando filas… | Loading rows… | TableRow loading |

Las etiquetas de Menu, Popover (títulos, «Restablecer», «Siempre visible») y los textos de Dialog los aportan las pantallas con su propio namespace; las primitivas no llevan texto de dominio. Ningún texto de usuario llega crudo: pasa por `t()` o por el saneado de ADR-GRP-004.

## 5. Catálogo de desarrollo

Página `/dev/ui`, solo con `import.meta.env.DEV`, sin Storybook (ADR-GRP-003, enmienda: Storybook entra con UI-2). Reglas de composición:

1. **Una rejilla por tema, lado a lado.** Como `glass.css` define los temas en `:root`, la página renderiza dos `<iframe>` del mismo origen (`/dev/ui/grid?theme=dark` y `?theme=light`) que fijan `document.documentElement.dataset.theme` antes de pintar. Nunca `prefers-color-scheme`.
2. **Filas = primitivas; columnas = estados.** Orden: reposo, hover, foco, pressed, disabled, error (solo donde aplique, §1.2). Cada primitiva muestra todas sus variantes y tamaños.
3. **Estados forzados.** hover, pressed y foco se fuerzan con `data-force="hover|pressed|focus"` mediante variantes `@custom-variant` de Tailwind definidas una vez y compartidas con producción: el catálogo y la app usan el mismo CSS.
4. **Casos límite por fila:** etiqueta larga con elipsis, solo icono, con contador, 3 niveles de sangría (TreeRow), 6 columnas y 3 columnas (TableRow), toast de cada tipo, Dialog `confirm` e `info` abiertos en un contenedor aislado.
5. **Superficies.** Cada primitiva se muestra sobre las superficies donde está permitida (C4–C13); sobre una prohibida solo en una fila marcada «no usar».
6. **Movimiento.** Un conmutador del catálogo emula `prefers-reduced-motion` y los estados deben cambiar sin transición.
7. **Verificación.** axe (`axe-core`, ya en el repo) corre sobre ambas rejillas en Vitest: roles, nombres, `aria-*`, orden de foco. En jsdom axe no calcula contraste: lo cubren §1.4 y el gate de `crates/theme` extendido a `glass.json`. Los tests de interacción cubren las tablas de teclado de §2.7 a §2.15 y que Enter con el foco inicial en Cancelar no confirma.

## 6. Cobertura frente al design-flow

| Necesidad (design-flow) | Primitiva |
|---|---|
| Botones de toolbar, «Integrar cambios», «Deshacer» (c2, d2) | Button, IconButton |
| Píldora de conflictos previstos, estado del motor (§ 2.3), chips `HEAD ↑4` | Pill, StatusDot, Chip |
| Árbol de repos con insignia, agentes en vivo, ⚡ (§ 2.2) | TreeRow, RepoBadge, StatusDot |
| Menú Fetch, Column settings (§ 4.b) | Menu, Popover, Toggle |
| Diálogo con foco inicial en Cancelar, Esc, vuelta del foco (c2, d2, § 5, § 7) | Dialog |
| Toast de 5 s con «Rehacer» (c4), conflicto nuevo (b) | Toast |
| Glosario de tooltips (§ 12), motivo de un botón desactivado | Tooltip |
| Cabecera y filas del grafo (§ 4.b, § 2.4) | TableHeader, TableRow |
| Comando para copiar en la terminal (d4, base pendiente) | CopyCommand |
| Selector de tema | SegmentedControl (3 opciones: auto, claro, oscuro; hoy `TitleBar` usa un IconButton cíclico) |
