---
id: DS-TS-CKP-004
title: "Dev Spec — Tokens semánticos y símbolos con fallback en el tema de la TUI"
type: dev-spec
status: approved
feature: cockpit
domain: GRP
story: TS-CKP-004
created: 2026-10-05
updated: 2026-10-06
related:
  adrs: [ADR-GRP-003, ADR-CKP-003, ADR-GRP-002, ADR-GRP-001]
  nfrs: [NFR-09]
  rules: [BR-CKP-EDGE-006]
tags: [cockpit, design-tokens, tema, simbolos, accesibilidad, nfr-09, dtcg, style-dictionary, contraste, no-color, paleta-a, terminal-clara, osc-11, colorfgbg]
---

# Dev Spec — TS-CKP-004: tokens semánticos y símbolos

Blueprint compacto de [TS-CKP-004](../technical-stories/TS-CKP-004-tokens-semanticos-simbolos.md). Fuentes: ADR-GRP-003 (tokens DTCG compilados a `crates/theme`), ADR-CKP-003 § 7 (anchura por símbolo) y § 10 (tema agnóstico de la biblioteca de TUI, resolución del tema), DSYS-GRP-001 § 2 y § 6. Solo tokens y tema: sin pantallas ni detección del modo (eso es de INF-CKP-001 y de las US-CKP).

## 1. Ubicación en el código

| Archivo | Responsabilidad |
|---|---|
| `packages/design-tokens/tokens/color.json` | Primitivos: hex + `ansi16` (y `ansi256` opcional) en `$extensions["dev.gitraptor"]` |
| `packages/design-tokens/tokens/semantic.json` | Semánticos: referencia a un primitivo, `highContrast`, `inherit`, `noColor`, `role`; o alias de otro semántico |
| `packages/design-tokens/tokens/symbol.json` | Símbolos: `{ glyph, ascii, width }` |
| `packages/design-tokens/scripts/rust-format.mjs` | Formato propio de Style Dictionary: 256 derivado (OKLab, 16..255), alias, salida Rust determinista |
| `packages/design-tokens/scripts/build.mjs` | `build` (salida JSON + Rust) y `--check` (regenera en un temporal y compara byte a byte) |
| `crates/theme/src/generated.rs` | **Generado y commiteado**: `ColorToken`, `SymbolToken` y sus tablas. `#[rustfmt::skip]`; clippy lo revisa |
| `crates/theme/src/lib.rs` | Tipos y resolución: `Theme`, `Style`, `Color`, `Glyph`, `Attrs`, `agent_color`, `contrast_ratio` |
| `.github/workflows/tokens.yml` | Job `tokens up to date`: `check` + tests del paquete |

## 2. Dependencias nuevas

| Dependencia | Dónde | Por qué | Licencia |
|---|---|---|---|
| `unicode-width` 0.2 | **dev-dependency** de `crates/theme` | Test: la anchura declarada ≥ la que mide `unicode-width` (la que usa ratatui) | MIT / Apache-2.0 |

`crates/theme` no tiene dependencias normales: ni `ratatui` ni `crossterm` (test de independencia).

## 3. Tokens

**Semánticos** (DSYS-GRP-001 § 2.1 y adiciones de esta TS): `text.{default,muted,inverse}`, `bg.{default,selected,highlight}`, `accent.default`, **`focus.default`**, `status.{success,warning,danger,info}` (severidades), `git.{added,removed,modified,conflict,branch.base}`, `agent.1` … `agent.8` y **`agent.state.{active,idle,done}`** (alias: `status.success`, `text.muted`, `text.muted`). El estado de un worktree no tiene tokens propios: se pinta con `git.modified`, `git.conflict` o `text.muted` más su símbolo.

Cada semántico tiene **tres profundidades en dos juegos** (normal y alto contraste): truecolor, índice de 256 (derivado, siempre 16..255: los 16 primeros los remapea el tema de la terminal) con su RGB xterm, y fallback de 16.

**Símbolos** (DSYS-GRP-001 § 2.2 + `info`):

| Token | Glifo | ASCII | Anchura |
|---|---|---|---|
| `success` / `error` | ✔ / ✖ | `[ok]` / `[x]` | 1 |
| `warning` / `info` | ⚠ / ℹ | `[!]` / `[i]` | 2 |
| `agent.{active,idle,done}` | ● / ◐ / ○ | `*` / `~` / `o` | 1 |
| `conflict` / `blocked` | ⚡ / ⛔ | `[c]` / `[blocked]` | 2 |
| `undo` | ⟲ | `[undo]` | 1 |

La anchura declarada es la **máxima** vista en terminales comunes. La TUI reserva esa anchura y rellena el glifo hasta ella. En ASCII la anchura es la longitud del fallback.

## 4. Resolución (`crates/theme`)

```rust
Theme::new(ColorMode::{TrueColor|Ansi256|Ansi16|NoColor}, Contrast::{Normal|High}, SymbolSet::{Unicode|Ascii})
theme.style(ColorToken) -> Style { color: Option<Color::{Rgb|Indexed|Ansi16}>, attrs: Attrs }
theme.symbol(SymbolToken) -> Glyph { text, width }
agent_color(index0) -> ColorToken      // agent.(index0 % 8 + 1)
is_agent_color_reused(index0) -> bool  // index0 >= 8
contrast_ratio(Rgb, Rgb) -> f64        // WCAG 2.1
```

- **Precedencia**: `NoColor` gana sobre `High` (ADR-CKP-003 § 10): sin color nunca sale un color, ni de texto ni de fondo; solo los atributos `noColor`.
- **Atributos sin color**: negrita e inverso cargan el significado (`status.danger`, `status.warning`, `git.conflict`, `accent` → negrita; `bg.selected` → inverso; `focus` → negrita + inverso). `dim` solo en `text.muted` (y sus alias), donde perderlo no quita información. `bg.selected` y `focus` se distinguen solo por la negrita: basta porque en una lista el foco y la selección coinciden.
- **Herencia**: en el juego normal, `text.default` y `bg.default` devuelven `None` (color de la terminal). En alto contraste el tema **pinta su propio fondo** (negro) y texto (blanco).
- `Style` no tiene rol: el widget aplica el color como texto o fondo. `ColorToken::role()` es orientativo.

## 5. Decisiones

**Decisión del orquestador (2026-10-05), validada por Arquitecto y PO.** Ajustes incorporados: del Arquitecto, OKLab, override de 256, alias `agent.state.*`, azul aclarado, test de `unicode-width` y pares de contraste; del PO, `agent.8` distinto de `text.muted`, el gate registrado en DSYS § 6 y en la TS, y `ℹ` en la verificación de anchura pendiente.

| # | Decisión | Motivo |
|---|---|---|
| D1 | Formato propio de Style Dictionary 5 (configuración en `.mjs`), `generated.rs` commiteado y control byte a byte en CI | Un formato propio no cabe en el JSON de configuración; commitear permite compilar sin Node |
| D2 | Índice de 256 derivado por distancia OKLab en 16..255, con override `ansi256` opcional; se emite su RGB xterm | Menos datos a mano; la distancia RGB falla en grises y azules; el RGB permite medir contraste en 256 sin otra tabla |
| D3 | Alto contraste en `$extensions` con el mismo esquema; pinta su propio fondo | Un tema que el usuario elige; así el contraste es calculable |
| D4 | Atributos sin color: negrita e inverso para lo importante; `dim` solo donde no hay pérdida | `dim` no se soporta igual en todas las terminales |
| D5 | `focus.default` y `agent.state.*` como alias; sin tokens de worktree | Lo pide el brief; alias para no abrir decisiones de paleta mientras § 8 sigue abierta |
| D6 | Símbolo `info` (`ℹ`/`[i]`); anchura máxima plausible + test contra `unicode-width` | `status.info` sin símbolo violaría "el color nunca va solo" |
| D7 | Paleta provisional | Ver ⚠️ ASSUMPTION abajo |
| D8 | **Gate de contraste en alto contraste ≥ 4.5:1** (WCAG AA, truecolor y 256); normal y 16 colores solo informe | Fondo propio → cálculo determinista; AA ya citado en ADR-GRP-003. 16 colores no es calculable (paleta del usuario). Cambia la línea "informe sin gate" de la TS |
| D9 | `crates/theme` sin dependencias normales; enums generados con `ALL` y `name()` | ADR-CKP-003 § 10 |

**Pares medidos** (D8): cada token de primer plano sobre `bg.default`; `text.default` y `text.muted` sobre `bg.selected` y `bg.highlight`; `text.inverse` sobre `accent`, `focus` y `status.*` (los colores sobre los que se imprime invertido).

~~⚠️ **ASSUMPTION**~~ **Retirada el 2026-10-05** (paleta A decidida; ver § 8). Texto original (DSYS-GRP-001 § 8.1 y § 8.2 entonces abiertas): acento teal; paleta de agentes Okabe-Ito (apta para daltonismo) con el negro sustituido por blanco en `agent.8` (un gris se confundía con `text.muted`, es decir, con un agente inactivo) y el azul aclarado (`#4fa3e0`; el original daba 3.3:1 sobre fondo oscuro); fondo oscuro supuesto para el informe del juego normal. Choques conocidos y aceptados porque el símbolo y el nombre distinguen siempre: el acento teal frente a `agent.2` (azul cielo), y bermellón, amarillo y verde azulado de agentes frente a `status.danger`, `warning` y `success`. En el juego normal, el informe marca por debajo de 4.5:1 `agent.6` (4.4), `text.muted` sobre `bg.selected` (2.4) y sobre `bg.highlight` (3.8): sin gate hasta que § 8 fije la paleta y el umbral.

## 6. Plan de tests

| Criterio de la TS | Test |
|---|---|
| Generación: el control falla con un token editado sin regenerar | `test/tokens.test.mjs`: `control fails when a token/symbol is edited…` (copia en temporal); `committed module is up to date`; `generation is deterministic`; CI `tokens up to date` |
| Completitud: tres profundidades + alto contraste; símbolo con glifo, fallback y anchura; fallback ASCII puro | Node: `every semantic token references primitives…`, `every symbol has glyph, pure-ASCII fallback and width`. Rust: `every_color_token_has_three_depths_in_both_sets`, `the_token_set_covers_the_design_system`, `every_symbol_has_glyph_ascii_fallback_and_width`, `declared_symbol_width_is_at_least_the_unicode_width`, `wide_symbols_reserve_two_columns` |
| Modos: truecolor, 256, 16, sin color (`NO_COLOR`), alto contraste, ASCII | `each_mode_yields_its_depth`, `no_color_never_yields_a_color_and_keeps_the_meaning_in_attributes`, `dim_and_underline_only_where_losing_them_loses_no_information`, `normal_set_inherits_…_high_contrast_paints_them`, `ascii_set_returns_the_fallback_and_its_length` |
| Independencia de la biblioteca de TUI | `crate_has_no_normal_dependencies` |
| Agentes: el noveno recibe el color del primero | `ninth_agent_reuses_the_first_color` |
| Contraste | `high_contrast_meets_wcag_aa_in_truecolor_and_256` (gate), `report_normal_contrast_and_agent_collisions` (informe, `--nocapture`), `contrast_ratio_matches_wcag_reference_values` |

## 7. Fuera de alcance (y a quién pertenece)

- Detección del modo desde `--no-color`, `NO_COLOR`, `COLORTERM`/`TERM`, `--theme`, `--ascii` y locale: INF-CKP-001 / historia de accesibilidad.
- Mapeo de `Style` a `ratatui::style` y relleno de glifos en el layout: `apps/cli` (INF-CKP-001).
- Paleta y acento definitivos, tema por defecto: DSYS-GRP-001 § 8.
- Variables CSS: Fase 3.
- Verificación manual de la paleta en emuladores de macOS (claro/oscuro, truecolor, 256, 16): se hace cuando exista la primera pantalla (INF-CKP-001), porque esta historia no pinta nada.
- Anchura real de `⚡`, `⛔`, `⚠` y `ℹ` en terminales de Linux y Windows: **Pendiente: etapa de validación multiplataforma**.
- El job `tokens up to date` bloquea el merge solo si se añade a la protección de rama (hoy solo se exigen los jobs `repo-intact`): anotado en el PR.

## 8. Enmienda (2026-10-05): paleta A, terminal clara y detección del fondo

**Decisión de Rene Bonilla (2026-10-05)**: paleta **A, «Grafito y teal»**. Retira la ⚠️ ASSUMPTION de § 5 (D7). El resto: **Decisión del orquestador (2026-10-05), validada por Arquitecto**. Valores en DSYS-GRP-001, enmienda de la paleta.

### 8.1 Ubicación

| Archivo | Cambio |
|---|---|
| `tokens/color.json` | Primitivos claros (`gray.{100,150,600,950}`, `*.700`, `yellow.600`, `okabeIto.*OnLight`) y `gray.{400,750}` para la oscura; overrides `ansi256` donde hace falta |
| `tokens/semantic.json` | `$extensions["dev.gitraptor"].light` en cada semántico no alias (obligatorio: el build falla sin él) |
| `scripts/rust-format.mjs` | Emite `light: Values` en `ColorSpec` |
| `crates/theme/src/lib.rs` | `Background`, `Theme::with_background`, `ColorToken::values(contrast, background)` |
| `crates/theme/src/detect.rs` | Lógica pura: `ThemeChoice`, `resolve`, `parse_osc11_reply`, `Background::{from_rgb, from_colorfgbg}`, `OSC11_QUERY`, `reply_complete` |
| `apps/cli/src/term.rs` | E/S (solo Unix, `rustix` `termios`): `detect_theme`, `query_background`. Lo cablea INF-CKP-001 |
| `crates/theme/examples/palette.rs` | Galería de tokens (ANSI o `--html`) |

### 8.2 API

```rust
Theme::new(mode, contrast, symbols)              // terminal oscura, como antes
    .with_background(Background::{Dark|Light})   // de resolve()
token.values(Contrast, Background) -> Values     // el alto contraste ignora el fondo
resolve(flag: Option<ThemeChoice>, no_color: bool, env, query) -> Detection { contrast, background, source, invalid_env }
// apps/cli: term::detect_theme(flag, no_color) = resolve(flag, no_color, env::var, || query_background(QUERY_TIMEOUT))
```

Precedencia: `--theme` > `GITRAPTOR_THEME` > OSC 11 > `COLORFGBG` > oscura. `auto` cede al siguiente. Sin color (`no_color`, `NO_COLOR` no vacío, `TERM=dumb`) no se consulta la terminal. `Source` dice de dónde salió (para diagnóstico).

### 8.3 Decisiones

| # | Decisión | Motivo |
|---|---|---|
| D10 | Variante `light` por token semántico, mismo esquema y tres profundidades; obligatoria salvo en alias | Una caída silenciosa a la oscura es el fallo que se corrige |
| D11 | Lógica en `crates/theme` sin dependencias; E/S de la terminal en `apps/cli` (`term`) | Mantiene D9 y el dueño único del estado de termios (ADR-CKP-003 § 12). Alternativa descartada: feature opcional con `rustix` en `crates/theme`, y el crate `terminal-colorsaurus` (más dependencias) |
| D12 | OSC 11 seguido de DA1; modo sin eco ni canónico (se mantiene `ISIG`), `VMIN=0`/`VTIME=1`, tope de 200 ms y 1 KiB, `tcflush` de la entrada y restauración con guardia | DA1 acaba la espera al instante en terminales sin OSC 11; `poll(2)` no sirve con `/dev/tty` en macOS. Una respuesta posterior al timeout puede llegar al lector de la TUI, que ignora secuencias desconocidas |
| D13 | Claro si la luminancia relativa > 0.179 (contrasta más con negro que con blanco); `COLORFGBG`: último campo, 7 y 9–15 claro | Umbral WCAG simétrico; convención de rxvt y vim |
| D14 | `GITRAPTOR_THEME` inválido: se ignora y se informa sin repetir el valor | Entrada sin controlar (SEC-12) |
| D15 | Tenue oscuro `#949494` en vez de `#8a8a8a`; `bg.highlight` oscuro `#3a3a3a` | Ajuste a la decisión de Rene: 4.35 → 4.95 sobre Solarized oscuro; `#262626` no se distinguía del fondo |
| D16 | Gate AA (truecolor y 256): texto y tenue de cada variante sobre sus fondos típicos y `text.default` sobre selección y barra; en la clara, todo primer plano sobre blanco y `text.inverse` sobre acento, foco y estados; en 256, `status.*`/`git.*` distintos no comparten índice. Informe del resto | Amplía D8. `text.default` se hereda: el gate mide su referencia |

**Por debajo de AA en el informe (sin gate, validado por Arquitecto)**: `status.danger` y `git.removed` oscuros sobre Solarized oscuro (4.48 en truecolor); `agent.3` (4.4) y `agent.6` (3.9) oscuros sobre Solarized oscuro, y `agent.6` sobre `#1e1e1e` (4.3); el tenue sobre la selección (4.4 en las dos variantes); en la clara, `accent`, `status.success`, `status.warning`, `git.*` y algún agente sobre `bg.selected` (3.6–4.1 en 256) y varios colores sobre Solarized claro (4.2–4.4). La fila seleccionada lleva además una marca que no es color (NFR-09).

### 8.4 Plan de tests (añadidos)

| Criterio | Test |
|---|---|
| Variante clara completa | Node: `every semantic token references primitives for every set…`, `the build fails when a semantic token has no light-terminal value`, `the control fails when a light-terminal value is edited…`. Rust: `every_color_token_has_three_depths_in_every_set` |
| Tema claro y alto contraste | `a_light_theme_paints_the_light_values_and_high_contrast_ignores_the_background`, `light_values_differ_from_dark_where_the_brief_found_them_unreadable` |
| Contraste | `normal_sets_meet_wcag_aa_on_typical_grounds` (gate), `status_and_git_colors_keep_distinct_256_indices_in_each_variant` (gate), `contrast_report` (informe) |
| Detección | `detect::tests::*`: respuestas OSC 11 simuladas, basura, luminancia, `COLORFGBG`, `ThemeChoice`, precedencia, sin respuesta, solo DA1, `NO_COLOR`/`TERM=dumb`/`--no-color`, variable inválida |
| E/S | `term::unix::tests::*` contra una pseudoterminal: responde, sin OSC 11 (vuelve al instante), muda (no pasa del timeout y restaura el modo), fichero que no es terminal |

### 8.5 Fuera de alcance

- ~~Cablear `--theme` en clap y llamar a `term::detect_theme` al arrancar la TUI~~: hecho en US-CKP-001 (2026-10-06), antes del lector de eventos de crossterm, junto con `--no-color`, `--ascii` y la profundidad de color ([DS-US-CKP-001](./US-CKP-001-flota-en-vivo.md) D3). Snapshots de widgets en las dos variantes: TS-CKP-005.
- Detección en Linux (probada solo en macOS) y en Windows (stub): **Pendiente: etapa de validación multiplataforma**.

