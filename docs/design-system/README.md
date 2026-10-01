---
id: DS-GRP-001
title: GitRaptor Design System
type: design-system
status: draft
version: 0.2
date: 2026-10-01
owner: Rene Bonilla
related: [BRD-GRP-001, ADR-GRP-002, ADR-GRP-003, ADR-GRP-004]
tags: [design-system, design-tokens, tui, cli, ratatui, accessibility, theming, mvp]
changelog:
  - 0.1 (2026-10-01): Design system completo (tokens, UI kit React, Storybook, Figma, temas web).
  - 0.2 (2026-10-01): Acotado al alcance inicial (MVP = CLI/TUI + MCP). Lo de la UI web/React queda diferido a la Fase 3.
---

# GitRaptor Design System

> La fuente única de verdad sobre cómo se ve, se siente y se comporta GitRaptor.

## 0. Alcance de esta versión

El MVP de GitRaptor (BRD-GRP-001, Fase 1) tiene **tres superficies: CLI, TUI y servidor MCP**. Este design system cubre **solo lo que esas superficies necesitan**:

| ✅ Incluido (v0, MVP) | ⏸️ Diferido (Fase 3, app de escritorio y extensión) |
|---|---|
| Principios de diseño | UI kit React (`@gitraptor/ui`) |
| Design tokens: **color**, símbolos y estados | Tipografía web, radios, sombras y escalas de espaciado en px |
| Tema de la TUI (truecolor, 256 colores, sin color) | Temas web (`light`, `high-contrast`, `vscode`) |
| Componentes de la TUI (widgets ratatui) | Storybook, regresión visual, librería de Figma |
| Convenciones de salida de la CLI | Sistema de movimiento web (Motion) |
| Contenido: voz, tono, mensajes de error y de política (humanos y agentes) | Patrones de UX y estado de React (ADR-GRP-004) |
| Accesibilidad en terminal | Gobierno completo con Changesets y niveles de madurez |

**Regla para crecer:** los tokens se definen desde ya en un formato neutral (DTCG JSON), así la Fase 3 los reutiliza sin rehacerlos. Todo lo demás se agrega cuando una fase lo requiera.

---

## 1. Principios de diseño

| Principio | En la TUI y la CLI |
|---|---|
| **Claridad antes que decoración** | El color solo tiene significado (estado, agente, tipo de cambio). Nada de bordes ni adornos de más. |
| **Velocidad percibida** | La TUI refleja cambios en menos de 500 ms (NFR-04). Nunca se bloquea la entrada del usuario; las operaciones largas muestran progreso. |
| **Seguridad visible** | Siempre se ve qué es reversible. Las acciones destructivas se distinguen visualmente y se ofrece `undo` tras cada operación. |
| **Densidad con calma** | Mucha información por pantalla, agrupada, con detalle bajo demanda (expandir o panel lateral). |
| **Teclado primero** | Todo con teclado, atajos siempre visibles en la barra de ayuda y un modo de ayuda (`?`). |
| **Respeta la terminal** | Hereda el fondo de la terminal, respeta `NO_COLOR` y funciona en tamaños pequeños (mínimo 80×24). |

---

## 2. Design tokens (v0)

- **Formato:** [W3C Design Tokens (DTCG)](https://www.designtokens.org/) en JSON, en `packages/design-tokens/tokens/*.json`.
- **Salida en el MVP:** Style Dictionary genera un módulo Rust (`crates/theme`) con la paleta para ratatui y la CLI. En la Fase 3, los mismos tokens generarán variables CSS.
- **Dos niveles:** **primitivos** (`color.green.500`) y **semánticos** (`color.status.success`). Los componentes de la TUI solo usan los semánticos.

### 2.1 Color semántico

| Grupo | Tokens |
|---|---|
| Texto | `text.default`, `text.muted`, `text.inverse` |
| Fondo | `bg.default` (= fondo de la terminal), `bg.selected`, `bg.highlight` |
| Marca | `accent.default` (color de acento, **por definir**) |
| Estado | `status.success`, `status.warning`, `status.danger`, `status.info` |
| Git | `git.added`, `git.removed`, `git.modified`, `git.conflict`, `git.branch.base` |
| Agentes | `agent.1` … `agent.8`: colores categóricos para distinguir agentes y carriles del grafo |

Cada token semántico define **tres valores**: *truecolor* (hex), *256 colores* (índice ANSI) y *fallback de 16 colores*, para que la TUI se vea bien en cualquier terminal.

### 2.2 Símbolos

El color nunca va solo: cada estado lleva además un símbolo y, si se puede, texto.

| Significado | Símbolo | Fallback ASCII |
|---|---|---|
| Éxito | `✔` | `[ok]` |
| Error | `✖` | `[x]` |
| Advertencia | `⚠` | `[!]` |
| Agente activo / inactivo / terminado | `●` / `◐` / `○` | `*` / `~` / `o` |
| Conflicto previsto | `⚡` | `[c]` |
| Bloqueado por política | `⛔` | `[blocked]` |
| Snapshot / undo disponible | `⟲` | `[undo]` |

`--ascii` (o detectar una terminal sin Unicode) activa el fallback.

---

## 3. Componentes de la TUI (v0)

Widgets ratatui en `apps/cli`, todos con los tokens de `crates/theme`:

| Componente | Uso | Notas |
|---|---|---|
| **Layout** (header, paneles, StatusBar) | Estructura del cockpit | Se adapta desde 80×24; los paneles colapsan en pantallas chicas |
| **AgentList / AgentRow** | Lista de agentes y worktrees: estado, rama, archivos, ahead/behind, última actividad | Color `agent.n` + símbolo de estado |
| **GraphLanes** | Grafo en vivo de las ramas de agentes sobre la base | Carriles con `agent.n`, caracteres de dibujo de caja |
| **DiffView** | Diff de un agente o commit | `git.added`/`git.removed`, números de línea en `text.muted` |
| **TimelineList** | Time Machine: snapshots con quién, cuándo y qué | Acción `undo` / `restore` visible |
| **ConflictAlert** | Aviso de conflicto previsto entre agentes | `status.warning` + `⚡`, lista de archivos |
| **PolicyBanner** | Acción bloqueada o pendiente de aprobación | `status.danger` + `⛔`, con la regla que la bloqueó |
| **ConfirmPrompt** | Confirmación de acciones irreversibles | Describe qué se pierde; por defecto queda en **No** |
| **Notification (toast)** | Feedback de acciones con "u: deshacer" | Desaparece a los 5 s y queda en el historial |
| **KeyHints / Help (`?`)** | Atajos de teclado visibles | Siempre en la parte inferior |

---

## 4. Convenciones de la CLI

| Aspecto | Regla |
|---|---|
| **Salida humana** | Una línea de resultado con un símbolo de estado, más detalles indentados. Color solo si la salida es una TTY. |
| **Salida para máquinas** | `--json` en todos los comandos, con un esquema estable y versionado. Es la base para scripts y agentes. |
| **Errores** | Formato *qué pasó → por qué → qué hacer*: `✖ Push bloqueado: la política "protect-main" prohíbe push a main → crea una rama: raptor branch new <nombre>` |
| **Códigos de salida** | `0` ok, `1` error, `2` uso incorrecto, `3` bloqueado por política, `4` conflicto. Documentados y estables. |
| **Progreso** | Un spinner o barra solo en una TTY; en CI o con pipes, líneas simples. |
| **Ayuda** | `raptor <cmd> --help` con ejemplos reales. |

---

## 5. Contenido: voz y tono

- **Voz:** directa, técnica y tranquila. Somos un copiloto confiable, no una alarma.
- **Verbos concretos:** "Restaurar snapshot", "Descartar worktree", no "Aceptar" ni "OK".
- **Términos de Git** en su forma estándar (commit, rebase, merge, worktree).
- **i18n:** inglés y español (NFR-10). Los mensajes viven en archivos de recursos y no se concatenan strings.
- **Mensajes para agentes (MCP):** cuando una política bloquea o una herramienta falla, la respuesta al agente es **estructurada y accionable**, con código de error, la regla violada y una alternativa permitida. Ejemplo: `{ "error": "POLICY_BLOCKED", "rule": "no-force-push", "suggestion": "usa safe_push con force_with_lease" }`. Así el agente puede corregirse solo, sin inventar un workaround.

---

## 6. Accesibilidad en terminal

- Se respeta **`NO_COLOR`** y hay un flag `--no-color`. Sin color, todo sigue entendiéndose gracias a los símbolos y el texto.
- La información nunca depende solo del color.
- Hay un **tema de alto contraste** para la TUI (`--theme high-contrast`).
- Navegación completa con teclado, sin depender del mouse (aunque se soporta).
- Funciona con lectores de pantalla en el modo CLI y con `--json`. La TUI ofrece un modo `--plain` sin redibujado continuo.

---

## 7. Herramientas (v0)

| Necesidad | Herramienta |
|---|---|
| Tokens | DTCG JSON + Style Dictionary → `crates/theme` (Rust) |
| TUI | ratatui + crossterm |
| CLI | clap (ayuda, autocompletado) |
| Pruebas visuales de la TUI | Snapshot tests de buffers ratatui con `insta` |

---

## 8. Decisiones pendientes (v0)

1. **Color de acento de marca** (`accent.default`) y paleta final de agentes, validada para daltonismo.
2. **Tema por defecto:** detectar si la terminal es clara u oscura, o asumir oscura.
3. **Responsable** del design system.

---

## 9. Qué se agrega en la Fase 3 (referencia)

Cuando arranque la app de escritorio o la extensión, se amplía este documento según [ADR-GRP-003](../architecture/decisions/ADR-GRP-003-design-system.md) y [ADR-GRP-004](../architecture/decisions/ADR-GRP-004-estado-frontend-ux.md):
- salida CSS de los tokens;
- tipografía, espaciado, radios y elevación web;
- temas `light` y `vscode`;
- UI kit React (`@gitraptor/ui`) con Radix, Tailwind y CVA;
- Storybook y regresión visual;
- Motion;
- librería de Figma;
- gobierno con versionado.
