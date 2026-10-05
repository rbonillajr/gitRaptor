# @gitraptor/tokens

Design tokens de GitRaptor en formato [DTCG](https://www.designtokens.org/), según el [design system v0](../../docs/design-system/README.md) (DSYS-GRP-001 § 2) y [TS-CKP-004](../../docs/requirements/features/cockpit/technical-stories/TS-CKP-004-tokens-semanticos-simbolos.md).

| Archivo | Contenido |
|---|---|
| `tokens/color.json` | Primitivos: hex y fallback de 16 colores (`$extensions["dev.gitraptor"].ansi16`). El índice de 256 colores se deriva en el build (OKLab, solo 16..255) salvo que el primitivo declare `ansi256`. |
| `tokens/semantic.json` | Semánticos (los únicos que usan los widgets): referencia al primitivo, `highContrast`, `inherit` (hereda el color de la terminal en el juego normal), `noColor` (atributos sin color) y `role`. Un semántico que solo referencia a otro es un alias. |
| `tokens/symbol.json` | Símbolos: glifo, fallback ASCII y anchura en columnas. |

## Comandos

- `pnpm nx build @gitraptor/tokens`: genera `build/tokens.json` y **`crates/theme/src/generated.rs`** (se commitea).
- `pnpm nx check @gitraptor/tokens`: falla si `generated.rs` no coincide con los tokens (lo corre el CI, workflow `tokens`).
- `pnpm nx test @gitraptor/tokens`: completitud de tokens y símbolos, fallbacks ASCII y el propio control.

Los valores de color son provisionales hasta que se cierre DSYS-GRP-001 § 8 (⚠️ **ASSUMPTION**).
