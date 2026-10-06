# @gitraptor/tokens

Design tokens de GitRaptor en formato [DTCG](https://www.designtokens.org/), según el [design system v0](../../docs/design-system/README.md) (DSYS-GRP-001 § 2) y [TS-CKP-004](../../docs/requirements/features/cockpit/technical-stories/TS-CKP-004-tokens-semanticos-simbolos.md).

| Archivo | Contenido |
|---|---|
| `tokens/color.json` | Primitivos: hex y fallback de 16 colores (`$extensions["dev.gitraptor"].ansi16`). El índice de 256 colores se deriva en el build (OKLab, solo 16..255) salvo que el primitivo declare `ansi256`. |
| `tokens/semantic.json` | Semánticos (los únicos que usan los widgets): referencia al primitivo (terminal oscura), `light` (terminal clara), `highContrast`, `inherit` (hereda el color de la terminal en el juego normal), `noColor` (atributos sin color) y `role`. Un semántico que solo referencia a otro es un alias. |
| `tokens/symbol.json` | Símbolos: glifo, fallback ASCII y anchura en columnas. |

## Comandos

- `pnpm nx build @gitraptor/tokens`: genera `build/tokens.json` y **`crates/theme/src/generated.rs`** (se commitea).
- `pnpm nx check @gitraptor/tokens`: falla si `generated.rs` no coincide con los tokens (lo corre el CI, workflow `tokens`).
- `pnpm nx test @gitraptor/tokens`: completitud de tokens y símbolos, fallbacks ASCII y el propio control.

La paleta es la **opción A, «Grafito y teal»**, decidida por Rene Bonilla el 2026-10-05 (DSYS-GRP-001, enmienda 2026-10-05 de la paleta), con una variante para terminal oscura y otra para terminal clara. El informe de contraste de cada token, con el gate AA, lo da `cargo test -p gitraptor-theme contrast_report -- --nocapture`.
