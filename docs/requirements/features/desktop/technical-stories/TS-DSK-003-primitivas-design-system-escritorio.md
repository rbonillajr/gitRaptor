---
id: TS-DSK-003
title: "Primitivas del design system de la app de escritorio y catálogo de desarrollo"
type: ts
status: ready
feature: desktop
domain: GRP
priority: high
complexity: medium
created: 2026-11-09
updated: 2026-11-09
related:
  context: [CTX-DSK-001]
  adrs: [ADR-GRP-003, ADR-GRP-004, ADR-GRP-017, ADR-GRP-002]
  nfrs: [NFR-09, NFR-13]
  stories: [US-DSK-003, US-DSK-004, US-DSK-005, US-DSK-006, US-DSK-007, US-DSK-008, TS-DSK-001]
  specs: [DS-TS-DSK-003]
ado:
  id: null
  url: null
tags: [desktop, design-system, primitivas, grafito-glass, tokens-web, a11y, axe, i18n, reduced-motion, catalogo-dev, sin-dependencias-ui, ui-1]
---

## TS-DSK-003 — Primitivas del design system de la app de escritorio y catálogo de desarrollo

**Valor**: cada historia de UI-1 compone sus pantallas con los mismos botones, filas, menús, avisos y diálogos, accesibles por teclado y lector de pantalla, en claro y oscuro y en en/es, sin decidir de nuevo cómo se ve ni cómo responde cada pieza.

### Descripción

**Como** Arquitecto
**Quiero** las 14 primitivas de la [spec de primitivas](../design/primitives-spec.md) en `apps/desktop/src/ui`, construidas solo con los tokens de `glass.css`, con sus pruebas de roles, teclado y axe, y un catálogo que solo existe en desarrollo
**Para** que US-DSK-003 a US-DSK-008 solo monten pantallas, y que la [Definición de diseño de componentes de ADR-GRP-003](../../../../architecture/decisions/ADR-GRP-003-design-system.md#definición-de-diseño-de-componentes) se cumpla una vez y la vigilen los tests

> **Depende de**: TS-DSK-001 (proyecto `apps/desktop`, `glass.css`, i18n y job `desktop`). **Habilita**: US-DSK-003 a US-DSK-008 (UI-1). **Decisión del orquestador (2026-11-09), validada por Arquitecto**: va como enabler y no dentro de una US porque las seis historias de UI-1 comparten las primitivas y ninguna tiene un resultado observable propio que las contenga todas. Las decisiones que la spec UX dejaba abiertas (dependencias, tokens ausentes, catálogo con los dos temas) están cerradas en la [Dev Spec](../dev-specs/TS-DSK-003-primitivas-design-system-escritorio.md) y en la [Enmienda 2026-11-09 de ADR-GRP-003](../../../../architecture/decisions/ADR-GRP-003-design-system.md#enmienda-2026-11-09-primitivas-de-ui-1-sin-dependencias-de-ui).

### Alcance Técnico

- **Implementar** en `apps/desktop/src/ui` las primitivas Button, IconButton, Chip, Pill, StatusDot, RepoBadge, SegmentedControl, Toggle, Tree/TreeRow, Menu, Popover, Toast, Dialog, Tooltip y Table (TableHeader, TableRow), con un archivo por primitiva y su `*.test.tsx`.
  - sin dependencias de UI nuevas (ni Radix, ni CVA, ni `tailwind-merge`, ni Lucide, ni Motion): hooks propios de foco, foco itinerante, cierre y posición, e iconos SVG en línea propios
  - solo tokens `--gr-*` de `glass.css`; ninguna rama por tema ni `dark:` de Tailwind
- **Añadir** a `packages/design-tokens` los tokens que faltan (`surface.scrim`, `accent.hover`, `live.bg`, `size.ring`, `size.dialog`, `z.*`) y los selectores de tema acotados (`[data-theme="dark"]`, `[data-theme="light"]`), y regenerar `glass.css`.
- **Añadir** las claves `ui.*` en en y es.
- **Crear** el catálogo de desarrollo (`?catalog`), con las primitivas en claro y oscuro lado a lado, que no llega al build de release.
- **Migrar** los tres botones de `TitleBar` a `IconButton` sin cambiar su aspecto.
- **Fuera de alcance**: `packages/ui` y Storybook (UI-2, ADR-GRP-003 Enmienda 2026-10-28); la toolbar y su botón partido (historia que monte la toolbar); la virtualización de árbol y grilla (US-DSK-004 y US-DSK-005); el modo de alto contraste (UI-2); la densidad `compact`; las pantallas de cada historia.

### Plan de Verificación

#### Pruebas Automatizadas

- **Por primitiva**: roles y nombres accesibles, tablas de teclado de la spec de primitivas (§ 2.7 a § 2.15), estados (`aria-disabled`, `aria-pressed`, `aria-checked`, `aria-selected`, `aria-busy`) y axe sin violaciones en en y es.
- **Dialog**: con el foco inicial en Cancelar, Enter no confirma; Escape cierra y devuelve el foco al disparador; el resto queda `inert`.
- **Lint de tokens**: ningún archivo de `src/ui` contiene colores hex, `rgb()`, `hsl()`, medidas en `px`, radios fuera de la escala ni `dark:`.
- **Movimiento**: las transiciones salen solo de las utilidades `gr-motion*`, que leen `--gr-motion-fast`, y `prefers-reduced-motion: reduce` lo pone a `0s`.
- **Contraste**: los pares token/token de la spec de primitivas (§ 1.4) calculados desde `glass.css`, con los tintes compuestos sobre su superficie.
- **Catálogo**: el build de release (`vite build`) no contiene el centinela del catálogo ni su chunk; en jsdom, axe sobre las dos rejillas sin violaciones.
- **i18n**: paridad en/es (test de catálogo existente).

#### Verificación Manual / Sandbox

- `pnpm --filter @gitraptor/desktop dev` y abrir `http://localhost:1420/?catalog`: las 14 primitivas en oscuro y claro lado a lado, con los estados forzados; el conmutador de movimiento reducido quita las transiciones.
- En la ventana B1, la barra de título se ve igual que antes de la migración a `IconButton`.
- VoiceOver en macOS recorre un Menu, un Dialog `confirm` y el árbol de ejemplo del catálogo y anuncia rol, nombre y estado.
