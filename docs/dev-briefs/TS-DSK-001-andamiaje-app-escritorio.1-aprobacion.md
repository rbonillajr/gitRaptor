---
schema: nassa.approval-request.v1
plan: "docs/requirements/features/desktop/dev-specs/TS-DSK-001-andamiaje-app-escritorio.md"
---

Lee este fichero y luego `TS-DSK-001-andamiaje-app-escritorio.md`; `TS-DSK-001-andamiaje-app-escritorio.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: no
- Generada: `2026-11-09T02:47:55.030Z`
- Sha de aprobación: `5d38fb3c5a2b14fbb17f4f472a6949f4cba8d5acd98c990f47629f559a6319c7:e21105b4116e4d2cc2954c7b935a14c4cf28f2df1ccf75152d481dad74bf959b:f8a197b92b92c37cb5f8c4c91773a0e9bc1f8d79898a05fdc8f5610cc5499a8a`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES 39e787f45cce>>>
```text
intent: Dar a la app de escritorio (D7, BR-27, hito M2-UI) una base sobre la que cada historia añada su vista sin reabrir la frontera de seguridad de ADR-GRP-017: backend Tauri que solo enlaza el cliente del canal, lista cerrada de comandos IPC, CSP estricta sin red y sin plugins shell, fs ni http.
endState: apps/desktop existe: el crate gitraptor-desktop compila con un único comando app_info, la ventana B1 vacía (grafito glass, claro y oscuro, atajos ⌘B y ⌥⌘B, cajón de detalle) se construye con Vite, las fitness functions V1, V2, V5 y V7 son tests verdes, glass.css se genera desde los tokens, el job de CI desktop corre la app y los jobs generales excluyen el crate.
nonGoals:
  - Conectar con el daemon o arrancarlo (TS-DSK-002, US-DSK-003)
  - externalBin, .dmg, firma o updater
  - Datos del motor, grafo, buscador ⌘K o píldora de estado
  - Tipos generados desde crates/api y TanStack Router
  - Marcar el check desktop como obligatorio en la protección de main (G4, tras el primer verde)
```
<<<FIN-DATOS-NO-CONFIABLES 39e787f45cce>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 0e21ce528be1>>>
```text
[1] id: IPC1
statement: ipc::parse acepta solo {"params": {…}} sin campos desconocidos en ningún nivel y rechaza cuerpos que no son objeto (invalid-params); app_info::info() devuelve CARGO_PKG_VERSION y PROTOCOL_VERSION; los comandos #[tauri::command] de src/commands coinciden con commands.json y con los permisos allow-<comando> de la capability
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test commands
tests:
  - apps/desktop/src-tauri/tests/commands.rs

[2] id: WIN1
statement: La ventana solo navega a los orígenes de frontendDist (tauri://localhost, http://tauri.localhost) y, en debug, a http://localhost:1420; cualquier otro esquema, host o puerto se rechaza
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test window
tests:
  - apps/desktop/src-tauri/tests/window.rs

[3] id: V1
statement: Las dependencias normales de gitraptor-desktop no alcanzan gitraptor-core, gitraptor-git ni gitraptor-policy, sí alcanzan gitraptor-api con la feature client, y el crate no está en default-members
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test boundaries
tests:
  - apps/desktop/src-tauri/tests/boundaries.rs

[4] id: V2
statement: La superficie de Tauri es la exacta de la Dev Spec: CSP y devCsp exactas, sin red ni unsafe-eval, flags endurecidos, una sola capability cerrada a main con permisos de commands.json, sin plugins (shell, fs, http ni otros), sin devtools y con los manejadores de navegación en window.rs
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test tauri_surface
tests:
  - apps/desktop/src-tauri/tests/tauri_surface.rs

[5] id: V7
statement: El backend no lee el entorno ni los argumentos en tiempo de ejecución y no depende de inicializadores de logging que lean variables
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test environment
tests:
  - apps/desktop/src-tauri/tests/environment.rs

[6] id: TOK1
statement: glass.css se genera desde tokens/glass.json con tema oscuro y claro idénticos en sus dos bloques, sin variables vacías y con el contraste WCAG de DSYS-GRP-002, y check detecta un glass.css desactualizado; construir no cambia la salida rust (crates/theme/src/generated.rs)
verify: pnpm --filter @gitraptor/tokens run build && git diff --exit-code crates/theme/src/generated.rs packages/design-tokens/css/glass.css && pnpm --filter @gitraptor/tokens run check && node --test packages/design-tokens/test/glass.test.mjs
tests:
  - packages/design-tokens/test/glass.test.mjs

[7] id: V5
statement: ESLint prohíbe HTML de texto no confiable (dangerouslySetInnerHTML, innerHTML, insertAdjacentHTML, document.write, librerías de markdown), los plugins de Tauri en todo el frontend y @tauri-apps/api fuera de src/client
verify: pnpm -C apps/desktop exec vitest run tests/eslint-v5.test.ts
tests:
  - apps/desktop/tests/eslint-v5.test.ts

[8] id: FE1
statement: La lista de comandos del frontend es igual a commands.json y call("app_info", {}) envía { params: {} } por invoke
verify: pnpm -C apps/desktop exec vitest run src/client/commands.test.ts
tests:
  - apps/desktop/src/client/commands.test.ts

[9] id: FE2
statement: El almacén de la ventana cicla el tema auto → light → dark → auto, persiste solo theme, language, sidebarOpen y detailOpen y se rehidrata
verify: pnpm -C apps/desktop exec vitest run src/state/ui-store.test.ts
tests:
  - apps/desktop/src/state/ui-store.test.ts

[10] id: FE3
statement: Los catálogos en y es tienen el mismo conjunto de claves y ningún valor vacío
verify: pnpm -C apps/desktop exec vitest run src/i18n/catalog.test.ts
tests:
  - apps/desktop/src/i18n/catalog.test.ts

[11] id: B1
statement: La ventana B1 vacía muestra barra de título, barra lateral, centro y detalle con nombres accesibles en en y es; ⌘B y ⌥⌘B pliegan los paneles; el tema cicla y fija data-theme; a 1200 px el detalle es un cajón con foco atrapado que Esc cierra; axe sin violaciones; marca gr:shell-painted
verify: pnpm -C apps/desktop exec vitest run src/shell/AppShell.test.tsx
tests:
  - apps/desktop/src/shell/AppShell.test.tsx

[12] id: CI1
statement: Los jobs generales excluyen gitraptor-desktop (cargo-test-selection, nextest-junit, affected-tests, repo-intact) y el workflow desktop existe con su filtro y sus pasos
verify: node --test tools/ci/desktop-exclusion.test.mjs
tests:
  - tools/ci/desktop-exclusion.test.mjs

[13] id: STA1
statement: El lock de pnpm está congelado, el frontend pasa lint y typecheck y el crate gitraptor-desktop pasa clippy sin avisos
verify: node --test tools/ci/desktop-static.test.mjs
tests:
  - tools/ci/desktop-static.test.mjs

[14] id: DOC1
statement: AGENTS.md dice que fuera del job desktop la suite general es cargo test --workspace --exclude gitraptor-desktop (G3)
verify: grep -q 'exclude gitraptor-desktop' AGENTS.md
tests:
  (ninguno)
```
<<<FIN-DATOS-NO-CONFIABLES 0e21ce528be1>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES ded780d84597>>>
```text
(el plan no declara gaps ni supuestos)
```
<<<FIN-DATOS-NO-CONFIABLES ded780d84597>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES 68a7bc3cd0a5>>>
```text
docs/requirements/features/desktop/dev-specs/TS-DSK-001-andamiaje-app-escritorio.md
```
<<<FIN-DATOS-NO-CONFIABLES 68a7bc3cd0a5>>>

planSha: `5d38fb3c5a2b14fbb17f4f472a6949f4cba8d5acd98c990f47629f559a6319c7` (base de la cadena de re-planes)

Léelo en `TS-DSK-001-andamiaje-app-escritorio.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES f9b65a81a386>>>
```text
docs/dev-briefs/ts-dsk-001-desktop-scaffold.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES f9b65a81a386>>>

contractSha: `e21105b4116e4d2cc2954c7b935a14c4cf28f2df1ccf75152d481dad74bf959b`

## Tests de criterio

testsSha: `f8a197b92b92c37cb5f8c4c91773a0e9bc1f8d79898a05fdc8f5610cc5499a8a`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES 70f1965812b4>>>
```text
325351007d3cc5b64d345089f897ddba90f5c7660d48c040539c055055e89cef  apps/desktop/src-tauri/tests/boundaries.rs
af3e70ad641863c79024c1d030ab67c45ff7c17f805639a3152eb6bac6d304aa  apps/desktop/src-tauri/tests/commands.rs
8046e35a65526a9f73ce47139e019dcf7d780d2788e905e71fd3beadd31e1b39  apps/desktop/src-tauri/tests/environment.rs
18e667946a5206a78946d89d405870fd6f8a8556a2d3fbc2e27f65febf06e1dd  apps/desktop/src-tauri/tests/tauri_surface.rs
663e685a826e8eb1926f2c1ea232710eedfd9d0cca1f5b81b70523c30d8ef98d  apps/desktop/src-tauri/tests/window.rs
5d2b01b3a31c1e18e88c809314b484062822099dc8784033b6636a85c490f458  apps/desktop/src/client/commands.test.ts
95f2ccdd1809755a39d687c32684712e5cee4361895b136758820ce1c2b44df1  apps/desktop/src/i18n/catalog.test.ts
9a15452298e5a2ffb4b6beb6ae339027a7c44347737023abb7dca88adcd24b3b  apps/desktop/src/shell/AppShell.test.tsx
3875a847038111513c56688d723c22163d6a200d883cf66bb3eda991e46eac0e  apps/desktop/src/state/ui-store.test.ts
deec9b7fbaaf6121785254a6e5824e34bc3c875c2af12dece5f2b9a82f763dc2  apps/desktop/tests/eslint-v5.test.ts
417f700f9f5b1eee3164e7b83368949fdc0a8d78575e1139fe20fb14cda9725c  packages/design-tokens/test/glass.test.mjs
3fe3247a6be879e50237c31e7cb5377ecca7d5de5fa4dd499d6ebabeed4712de  tools/ci/desktop-exclusion.test.mjs
aea1846ce859b25eb680d94e38724a3a0c6a99fd55974cf260ab952662b42410  tools/ci/desktop-static.test.mjs
```
<<<FIN-DATOS-NO-CONFIABLES 70f1965812b4>>>

## Crítico

- Rondas (plan): 3
  - ronda 1: AJUSTES · 2 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
  - ronda 3: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
- Último veredicto: APROBABLE
- Excepción: no

## Diferidos

Ninguno: el crítico dio el plan por aprobable.

## Firmas

- planSha (contexto, no firmado): `5d38fb3c5a2b14fbb17f4f472a6949f4cba8d5acd98c990f47629f559a6319c7`
- contractSha: `e21105b4116e4d2cc2954c7b935a14c4cf28f2df1ccf75152d481dad74bf959b`
- testsSha: `f8a197b92b92c37cb5f8c4c91773a0e9bc1f8d79898a05fdc8f5610cc5499a8a`
- approvalSha: `5d38fb3c5a2b14fbb17f4f472a6949f4cba8d5acd98c990f47629f559a6319c7:e21105b4116e4d2cc2954c7b935a14c4cf28f2df1ccf75152d481dad74bf959b:f8a197b92b92c37cb5f8c4c91773a0e9bc1f8d79898a05fdc8f5610cc5499a8a`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
