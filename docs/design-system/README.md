---
id: DSYS-GRP-001
title: GitRaptor Design System
type: design-system
status: draft
version: 0.4
date: 2026-10-01
updated: 2026-10-05
owner: Rene Bonilla
related: [BRD-GRP-001, ADR-GRP-002, ADR-GRP-003, ADR-GRP-004, ADR-CKP-003]
tags: [design-system, design-tokens, tui, cli, ratatui, accessibility, theming, mvp]
changelog:
  - 0.1 (2026-10-01): Design system completo (tokens, UI kit React, Storybook, Figma, temas web).
  - 0.2 (2026-10-01): Acotado al alcance inicial (MVP = CLI/TUI + MCP). Lo de la UI web/React queda diferido a la Fase 3.
  - 0.3 (2026-10-04): Enmienda del Cockpit (E7 de ADR-CKP-003): símbolos con anchura, `crates/theme` agnóstico de ratatui, alcance de `--plain`, versiones de la TUI e i18n con catálogo tipado.
  - 0.4 (2026-10-05): Enmienda de TS-CKP-004: `focus.default`, `agent.state.*`, símbolo `info`, gate de contraste del tema de alto contraste y tokens implementados.
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
- **Salida en el MVP:** Style Dictionary genera un módulo Rust (`crates/theme`) con la paleta para ratatui y la CLI. En la Fase 3, los mismos tokens generarán variables CSS. (Enmienda 2026-10-04, Cockpit: `crates/theme` no depende de ratatui; ver la sección final.)
- **Dos niveles:** **primitivos** (`color.green.500`) y **semánticos** (`color.status.success`). Los componentes de la TUI solo usan los semánticos.

### 2.1 Color semántico

| Grupo | Tokens |
|---|---|
| Texto | `text.default`, `text.muted`, `text.inverse` |
| Fondo | `bg.default` (= fondo de la terminal), `bg.selected`, `bg.highlight` |
| Marca | `accent.default` (color de acento, **por definir**) |
| Foco | `focus.default`: panel o fila con el foco del teclado (enmienda 2026-10-05) |
| Estado | `status.success`, `status.warning`, `status.danger`, `status.info` |
| Git | `git.added`, `git.removed`, `git.modified`, `git.conflict`, `git.branch.base` |
| Agentes | `agent.1` … `agent.8`: colores categóricos para distinguir agentes y carriles del grafo |
| Estado de agente | `agent.state.active`, `agent.state.idle`, `agent.state.done`: alias de `status.success` y `text.muted`; el símbolo es la señal principal (enmienda 2026-10-05) |

Cada token semántico define **tres valores**: *truecolor* (hex), *256 colores* (índice ANSI) y *fallback de 16 colores*, para que la TUI se vea bien en cualquier terminal.

### 2.2 Símbolos

El color nunca va solo: cada estado lleva además un símbolo y, si se puede, texto.

| Significado | Símbolo | Fallback ASCII |
|---|---|---|
| Éxito | `✔` | `[ok]` |
| Error | `✖` | `[x]` |
| Advertencia | `⚠` | `[!]` |
| Información | `ℹ` | `[i]` |
| Agente activo / inactivo / terminado | `●` / `◐` / `○` | `*` / `~` / `o` |
| Conflicto previsto | `⚡` | `[c]` |
| Bloqueado por política | `⛔` | `[blocked]` |
| Snapshot / undo disponible | `⟲` | `[undo]` |

`--ascii` (o detectar una terminal sin Unicode) activa el fallback. (Enmienda 2026-10-04, Cockpit: cada símbolo es un token con glifo, fallback ASCII y anchura; ver la sección final.)

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
- **i18n:** inglés y español (NFR-10). Los mensajes viven en archivos de recursos y no se concatenan strings. (Enmienda 2026-10-04, Cockpit: en el MVP, catálogo tipado en el código; ver la sección final.)
- **Mensajes para agentes (MCP):** cuando una política bloquea o una herramienta falla, la respuesta al agente es **estructurada y accionable**, con código de error, la regla violada y una alternativa permitida. Ejemplo: `{ "error": "POLICY_BLOCKED", "rule": "no-force-push", "suggestion": "usa safe_push con force_with_lease" }`. Así el agente puede corregirse solo, sin inventar un workaround.

---

## 6. Accesibilidad en terminal

- Se respeta **`NO_COLOR`** y hay un flag `--no-color`. Sin color, todo sigue entendiéndose gracias a los símbolos y el texto.
- La información nunca depende solo del color.
- Hay un **tema de alto contraste** para la TUI (`--theme high-contrast`). Pinta su propio fondo y su texto cumple **WCAG AA (≥ 4.5:1)**, comprobado en CI (enmienda 2026-10-05).
- Navegación completa con teclado, sin depender del mouse (aunque se soporta).
- Funciona con lectores de pantalla en el modo CLI y con `--json`. La TUI ofrece un modo `--plain` sin redibujado continuo. (Enmienda 2026-10-04, Cockpit: alcance de `--plain` y del ratón en el MVP; ver la sección final.)

---

## 7. Herramientas (v0)

| Necesidad | Herramienta |
|---|---|
| Tokens | DTCG JSON + Style Dictionary → `crates/theme` (Rust) |
| TUI | ratatui + crossterm (Enmienda 2026-10-04, Cockpit: `ratatui` 0.30.x con `crossterm` 0.29) |
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

---

## Enmienda (2026-10-04, Cockpit)

Aplicada desde la enmienda E7 de [ADR-CKP-003](../architecture/decisions/ADR-CKP-003-arquitectura-tui.md) (§ 7 y § 10; accepted 2026-10-04). **Decisión del orquestador (2026-10-04), validada por Arquitecto**; el PO valida el alcance después. No cambia los principios, la paleta, los componentes ni las convenciones de la CLI.

| Cambio | Dónde | Fuente |
|---|---|---|
| Cada símbolo es un token con glifo, fallback ASCII y **anchura en columnas** | § 2.2 | ADR-CKP-003 § 7 y § 10 |
| `crates/theme` es agnóstico de ratatui | § 2 | ADR-CKP-003 § 10 |
| Alcance de `--plain` en el MVP; ratón desactivado por defecto | § 6 | ADR-CKP-003 § 10 |
| `ratatui` 0.30.x con el backend `crossterm` 0.29 | § 7 | ADR-CKP-003 § 1 |
| i18n del MVP con catálogo tipado | § 5 | ADR-CKP-003 § 10 (ajuste de coherencia) |

- **Símbolos como tokens**: cada símbolo de § 2.2 se define en `packages/design-tokens` con su glifo, su fallback ASCII y su **anchura de visualización**, porque `⚡`, `⛔` y `⚠` ocupan dos columnas en muchas terminales y sus fallbacks ASCII ocupan más. El layout toma la anchura del tema activo, no del texto. El fallback ASCII se activa con `--ascii` o con una locale que no sea UTF-8. Es un **requisito previo** de cualquier pantalla: hoy los tokens solo tienen dos primitivos.
- **`crates/theme` agnóstico de ratatui**: expone por token semántico truecolor, índice de 256 colores y fallback de 16 colores, y por símbolo glifo, fallback y anchura. `apps/cli` lo mapea a los estilos de ratatui. Ningún widget usa colores ni glifos literales.
- **`--plain` en el MVP**: mismo modelo, otro renderer. Sin pantalla alternativa ni movimiento del cursor: escribe la vista inicial como texto y después solo líneas nuevas con los cambios y las alertas. ⚠️ **ASSUMPTION** a validar por el PO: en el MVP, `--plain` es de **lectura y alertas**; las acciones de BR-07 requieren la TUI completa.
- **Ratón**: ⚠️ **ASSUMPTION**: la captura del ratón está desactivada por defecto, para no romper la selección de texto de la terminal. La navegación completa con teclado no cambia.
- **i18n**: en el MVP los mensajes viven en un **catálogo tipado** en el código (una enumeración con un `match` exhaustivo por idioma), no en archivos de recursos: una traducción ausente es un error de compilación. Sigue sin concatenar cadenas. Se revisa si hace falta pluralización compleja.
- **Pruebas visuales**: snapshots con `insta` en 80×24, 100×30, 120×40 y 79×24, con truecolor, 256 colores, 16 colores, `NO_COLOR` y alto contraste, y con los dos juegos de símbolos (ADR-CKP-003, Validación V1).

Linux y Windows (anchura de símbolos y consola de Windows): **Pendiente: etapa de validación multiplataforma**.

---

## Enmienda (2026-10-05, TS-CKP-004)

Aplicada al implementar los tokens ([TS-CKP-004](../requirements/features/cockpit/technical-stories/TS-CKP-004-tokens-semanticos-simbolos.md), [Dev Spec](../requirements/features/cockpit/dev-specs/TS-CKP-004-tokens-semanticos-simbolos.md)). **Decisión del orquestador (2026-10-05), validada por Arquitecto y PO.** No cierra ninguna decisión de § 8.

| Cambio | Dónde |
|---|---|
| `focus.default` para el foco del teclado; sin color, negrita + inverso | § 2.1 |
| `agent.state.{active,idle,done}` como alias de semánticos existentes | § 2.1 |
| Símbolo `info` (`ℹ` / `[i]`, anchura 2), porque `status.info` no tenía símbolo | § 2.2 |
| El tema de alto contraste pinta su propio fondo y tiene un gate WCAG AA (4.5:1) en CI; el juego normal hereda texto y fondo de la terminal y su contraste solo se informa | § 6 |
| Sin color, el significado lo cargan la negrita y el inverso; `dim` solo en `text.muted` | § 6 |

- **Implementación**: `packages/design-tokens/tokens/{color,semantic,symbol}.json` → Style Dictionary → `crates/theme/src/generated.rs` (commiteado). El workflow `tokens` falla si el código generado no coincide con los tokens. El índice de 256 colores se deriva de cada hex (solo 16..255).
- ⚠️ **ASSUMPTION** (§ 8 sigue abierta): acento teal, paleta de agentes Okabe-Ito (apta para daltonismo) con `agent.8` en blanco y el azul aclarado, y fondo oscuro supuesto para el informe del juego normal.

Anchura real de `⚡`, `⛔`, `⚠` y `ℹ` en Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

<!-- ci speed probe: mixed -->
