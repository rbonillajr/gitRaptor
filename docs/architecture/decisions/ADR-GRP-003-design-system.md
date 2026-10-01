---
id: ADR-GRP-003
title: Design system de GitRaptor — tokens, UI kit (@gitraptor/ui), patrones, movimiento y documentación
type: adr
status: accepted
date: 2026-10-01
deciders: [Rene Bonilla]
related: [BRD-GRP-001, ADR-GRP-001, ADR-GRP-002, ADR-GRP-004]
tags: [design-system, ui-kit, react, radix, tailwind, cva, design-tokens, motion, animations, storybook, vscode-webview]
---

# ADR-GRP-003 — Design system de GitRaptor

## Contexto

La UI de GitRaptor vive en dos superficies que deben verse y comportarse igual:
- la app de escritorio (Tauri + React, ADR-GRP-001);
- los webviews de la extensión de VS Code/Cursor.

Para lograr un nivel profesional y consistente no basta una librería de componentes: hace falta un **design system** completo (principios, tokens, componentes, patrones, movimiento, accesibilidad, documentación y gobierno), del que el UI kit es solo una pieza.

Queremos:
- reutilizar componentes en vez de duplicarlos;
- una identidad visual "llamativa" y consistente;
- accesibilidad (NFR-09);
- y que dentro del editor la UI **respete el tema del usuario** (variables `--vscode-*`).

## Alcance por fase

- **MVP (Fase 1, CLI/TUI + MCP):** solo los **fundamentos** (principios, tokens de color y símbolos en DTCG JSON), el **tema y los componentes de la TUI**, las convenciones de la CLI, el contenido y la accesibilidad en terminal. El detalle está en [`docs/design-system/README.md`](../../design-system/README.md) v0.2. Los tokens se compilan a Rust (`crates/theme`).
- **Fase 3 (app de escritorio y extensión):** todo lo que describe este ADR sobre la UI web (UI kit React, temas web, Storybook, Motion, Figma). **No se implementa antes** de que esa fase arranque.

## Decisión

Adoptar un **design system propio de GitRaptor**, documentado en [`docs/design-system/`](../../design-system/README.md), compuesto por:

| Pieza | Dónde vive |
|---|---|
| Principios de diseño, identidad visual, contenido y gobierno | `docs/design-system/README.md` |
| Design tokens | `@gitraptor/tokens` (`packages/design-tokens`) |
| UI kit (componentes React) | `@gitraptor/ui` (`packages/ui-kit`) |
| Reglas de diseño de componentes y sistema de movimiento | Este ADR |
| Patrones de UX y estado | ADR-GRP-004 |
| Documentación viva | Storybook (en `packages/ui-kit`) |
| Librería de diseño | Figma, espejo de tokens y componentes |

Su implementación técnica son dos paquetes del monorepo (ADR-GRP-002):

1. **`@gitraptor/tokens`** (`packages/design-tokens`): **design tokens** de color, tipografía, espaciado, radios, sombras y motion, como fuente única (JSON) que se compila a **variables CSS** y a tipos TS. Incluye temas `dark`, `light` y `high-contrast`, más un tema **`vscode`** que mapea los tokens a las variables `--vscode-*` del editor.
2. **`@gitraptor/ui`** (`packages/ui-kit`): una librería de componentes **React + TypeScript**:
   - **Primitivas:** **Radix UI** (accesibilidad, foco y teclado resueltos), con el estilo propio encima, al estilo shadcn: los componentes son nuestros y no una dependencia opaca.
   - **Estilos:** **Tailwind CSS v4** que consume los tokens como variables CSS. Nada de colores hardcodeados.
   - **Variantes tipadas:** **CVA** (class-variance-authority) + `tailwind-merge`. Cada componente expone `variant`, `size` y `tone`, y TypeScript impide combinaciones inválidas.
   - **Animación:** **Motion** (antes Framer Motion) con `LazyMotion` y componentes `m` para lo que tiene estado; transiciones CSS para lo simple (ver "Sistema de movimiento").
   - **Iconografía y tipografía:** **Lucide** como set único de íconos. Fuente UI **Inter** o **Geist**, y monoespaciada **JetBrains Mono** o **Geist Mono** para código y diffs. Las fuentes van embebidas en el build.
   - **Build:** **Vite en modo librería** (ESM + `.d.ts`), con tree-shaking y `react` y `react-dom` como peer dependencies.
   - **Catálogo y pruebas:** **Storybook**, que documenta cada componente, permite revisarlo visualmente por tema y corre tests de interacción y de accesibilidad (addon a11y).
   - **Tests:** Vitest + Testing Library.
   - **Distribución:** consumo interno vía `workspace:*`. Publicarlo en npm es opcional más adelante.

### Catálogo inicial de componentes

| Categoría | Componentes |
|---|---|
| Base | Button, IconButton, Input, Select, Checkbox, Switch, Tooltip, Badge, Kbd, Spinner |
| Layout | Panel, SplitPane (redimensionable), Tabs, Toolbar, Sidebar, StatusBar |
| Overlays | Dialog, ConfirmDialog (acciones destructivas, NFR-01), DropdownMenu, ContextMenu, CommandPalette, Toast |
| Datos | VirtualList y VirtualTable (con `@tanstack/react-virtual`), Tree, EmptyState |
| Dominio Git/agentes | BranchPill, CommitRow, AgentCard (estado, rama, actividad), ConflictBadge, DiffViewer (unificado y split), TimelineItem (Time Machine), PolicyViolationBanner |

El **grafo de commits** no forma parte del UI kit. Vive en `@gitraptor/graph` (Canvas/WebGL) y el UI kit solo lo envuelve con un componente React.

## Definición de diseño de componentes

Todo componente del UI kit cumple este contrato antes de considerarse terminado:

| Aspecto | Regla |
|---|---|
| **Tokens** | Solo usa tokens semánticos (`surface`, `surface-raised`, `accent`, `danger`, `warning`, `success`, `agent-1..n`, `text-muted`…), nunca valores literales. Espaciado en escala de 4 px. |
| **Estados** | Define y documenta: default, hover, focus-visible, active, selected, disabled, loading, error y empty (cuando aplique). |
| **Variantes** | Declaradas con CVA y tipadas. Sin props booleanas que se contradigan (`primary` + `danger`). |
| **API** | `forwardRef`, `className` extensible, `asChild` cuando aplique, *compound components* para piezas compuestas (`<Panel.Header>`, `<Panel.Body>`). |
| **Accesibilidad** | Navegable por teclado, foco visible, roles y ARIA correctos (Radix), contraste WCAG 2.1 AA y estados que no dependen solo del color. |
| **Densidad** | Soporta `comfortable` y `compact` (los devs prefieren UIs densas). |
| **Documentación** | Una story de Storybook por variante y estado, con controles y notas de uso (cuándo sí y cuándo no). |
| **Calidad** | Tests unitarios y de interacción (Vitest + Testing Library), addon a11y sin violaciones y **regresión visual** en CI (Chromatic o Playwright con screenshots) en todos los temas. |
| **Diseño ↔ código** | Los tokens se sincronizan con las variables de Figma (Tokens Studio o Figma Variables). Figma y código usan los mismos nombres. |

## Sistema de movimiento (animaciones)

Objetivo: que la UI se sienta fluida y profesional sin costo de rendimiento.

| Regla | Detalle |
|---|---|
| **Tokens de movimiento** | `motion.duration.fast = 120ms`, `base = 200ms`, `slow = 320ms`. Curvas: `ease-out` estándar para entradas, `ease-in` para salidas y un spring definido (`stiffness`/`damping`) para layout. Todo el producto se mueve igual. |
| **Solo propiedades de GPU** | Se animan únicamente `transform` y `opacity`. Prohibido animar `width`, `height`, `top`, `left` o `margin`. |
| **CSS primero** | Hover, focus, press y tooltips con transiciones CSS, sin JS. |
| **Motion para lo que tiene estado** | Entrada y salida (`AnimatePresence`), reordenamiento de listas y layout (`layout`), feedback de acciones. Carga diferida con `LazyMotion` + `domAnimation` para minimizar el bundle. |
| **Con propósito** | Se anima para dar feedback (un agente terminó, apareció un conflicto, se restauró un undo, una política bloqueó algo), no por decoración. Microinteracciones de 150 a 250 ms. |
| **Accesibilidad** | Se respeta `prefers-reduced-motion` globalmente con `<MotionConfig reducedMotion="user">`. |
| **Grafo fuera de React** | La animación del grafo en vivo (ramas que crecen) va en Canvas/WebGL con `requestAnimationFrame` dentro de `@gitraptor/graph`. |
| **Presupuesto** | 60 fps sostenidos con 10 agentes activos. Ninguna animación bloquea la interacción. |

## Reglas

- El UI kit **no conoce al motor**: sin llamadas a Tauri ni JSON-RPC. Recibe datos por props, y las apps lo conectan con `@gitraptor/client`. Esto se hace cumplir con los tags de Nx (`type:ui` solo puede depender de `type:ui` y `type:tokens`).
- Cada componente nuevo entra con su story, sus tests y su revisión de a11y.
- Rendimiento: los componentes de listas son virtualizados por defecto y se memoizan donde reciben actualizaciones en vivo (cockpit).

## Alternativas consideradas

- **Librería completa de terceros (MUI, Ant Design, Chakra):** acelera al inicio, pero da una identidad genérica, bundles más pesados y es difícil mapearla al tema de VS Code.
- **VS Code Webview UI Toolkit (Microsoft):** **deprecado** (2025) y solo sirve dentro del editor, no en la app de escritorio.
- **CSS Modules sin Tailwind:** viable, pero con más trabajo para mantener la consistencia con los tokens. Tailwind v4 con variables CSS da utilidades sin acoplarse a los valores.

## Consecuencias

- ✅ Una sola fuente de componentes y estilos para el escritorio y la extensión.
- ✅ Accesibilidad de base gracias a Radix y temas intercambiables, incluido el tema del editor.
- ✅ Storybook sirve como documentación viva y como punto de entrada para diseño (Figma ↔ tokens).
- ⚠️ Mantener un design system cuesta. **Mitigación:** un catálogo inicial acotado, que crezca según lo pida cada fase.
- ⚠️ Los webviews de VS Code tienen una CSP restrictiva. **Mitigación:** el build del UI kit no carga recursos externos y las fuentes van embebidas o usan las del sistema.
