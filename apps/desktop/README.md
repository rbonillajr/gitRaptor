# GitRaptor: app de escritorio

App de escritorio (Tauri + React) de GitRaptor. Es un cliente más del daemon por el mismo canal que la TUI y la CLI (ADR-GRP-017).

## Perfil que recibe la app del daemon

El daemon decide qué ve la app por la ascendencia de su proceso y por la ruta de su ejecutable, nunca por lo que la app declare (TS-GRP-009, [DS-TS-GRP-009](../../docs/requirements/features/motor-local/dev-specs/TS-GRP-009-dev-spec.md)):

- Instalada en `/Applications` o `~/Applications` y abierta desde el Finder: vista completa.
- Lanzada por un agente (por ejemplo, Claude Code ejecuta el binario de la app): perfil `mcp`, vista restringida.
- Con un intérprete, un multiplexor de terminal o una ascendencia que no se puede leer en la cadena: perfil `mcp` (ante la duda, el más restrictivo).

## Desarrollo

`pnpm tauri dev` arranca la app bajo `node`, que es un intérprete: **el daemon le da el perfil `mcp`**. Es lo esperado, no un fallo.

Para ver la vista completa en desarrollo, construye la app y lanza el binario directamente desde tu terminal (no desde una sesión de un agente):

```sh
pnpm --filter @gitraptor/desktop build
cargo build -p gitraptor-desktop
./target/debug/gitraptor-desktop
```

Lanzado así, sin identidad por ruta, el daemon aplica las reglas de un cliente con terminal (controles 1 a 3 de ADR-GRP-005 § 6). Con la alpha instalada en la carpeta de aplicaciones, ábrela desde el Finder.
