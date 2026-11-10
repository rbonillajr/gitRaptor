---
schema: nassa.approval-request.v1
plan: "docs/dev-briefs/ts-dsk-002-autostart-dmg.prep.md"
---

Lee este fichero y luego `ts-dsk-002-autostart-dmg.prep.md`; `ts-dsk-002-autostart-dmg.prep.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: **sí** — el crítico agotó sus rondas en la ronda 2 sin un plan aprobable; ver «Diferidos»
- Generada: `2026-11-10T11:16:46.802Z`
- Sha de aprobación: `9f514fa5a82b9c8f0fc4486872398cecdc07f13818158aa1f1235789adf3a540:077e4fb4156419a128535737d428f04f5cf36cb3255c91a9cf634846a6851467:6052e5eff0ed6fccec42ee71b7c53337a87e7d164766f60ba151fb49c962e5be`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES ce62f6af8793>>>
```text
intent: Que el humano pueda pedir desde la app de escritorio que el motor arranque al iniciar sesión, solo con un Sí en un diálogo nativo que el webview no controla, y que la alpha semanal publique la app como .dmg sin firmar con raptor y raptor-mcp dentro (ADR-GRP-014 Enmienda DEP-DSK-5, ADR-GRP-017, NFR-02, SEC-13).
endState: gitraptor-desktop registra tauri-plugin-dialog =2.8.3 (único plugin, sin ningún permiso dialog:* en la capability); engine/autostart.rs decide la oferta (offer_with_home) y ejecuta daemon enable/disable con argv fijo y entorno vacío más PATH solo tras un Sí (run_choice); los comandos engine_autostart_offer y engine_autostart están en commands.json, el handler y la capability, y CommandError gana not-offered, cancelled y failed con su catálogo en/es; un componente AutostartOffer (primitivas y tokens, en/es, axe, claro y oscuro) deja pedirlo desde la app; tauri.release.conf.json, tools/desktop/stage-sidecars.sh, el job desktop-app de release.yml (sin secretos, signingIdentity -) y select-assets.sh publican GitRaptor-<versión>-aarch64-apple-darwin.dmg y su .sha256 en las alphas; un guion de verificación manual T009 queda listo para el coordinador. V1, V2 y V7 siguen en verde.
nonGoals:
  - firma Developer ID, notarización, cask y updater (v0.1.0)
  - Intel, Windows y Linux (etapa de validación multiplataforma)
  - ejecutar la verificación manual T009 (la ejecuta el coordinador) y marcar la ficha implemented antes de que se verifique
  - supervisor, EngineHub y pantallas del motor de US-DSK-003, más allá de montar AutostartOffer
  - audit token / LOCAL_PEERTOKEN (SPIKE-DSK-001) y clase de cliente desktop (DEP-DSK-2)
  - lanzar workflow_dispatch en macOS o cambiar variables RUST_* o CARGO_* de los workflows
```
<<<FIN-DATOS-NO-CONFIABLES ce62f6af8793>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 48417b83af9c>>>
```text
[1] id: DSK002-OFFER
statement: T007: offer_with_home da Offered para /Applications/GitRaptor.app/... y para <home>/Applications/..., y NotOffered Location para /Volumes/..., para una ruta con /AppTranslocation/ y para una ruta fuera de /Applications; con autostart registrado con el raptor del bundle da Registered; registrado con otro binario da NotOffered OtherBinary
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test autostart_offer
tests:
  - apps/desktop/src-tauri/tests/autostart_offer.rs

[2] id: DSK002-FLOW
statement: T007: run_choice con la confirmación en false devuelve Cancelled y el raptor falso no se ejecuta (registro vacío); con true ejecuta exactamente daemon enable (o daemon disable) con entorno = solo PATH y devuelve registered; una salida distinta de 0 es Failed; enable sin oferta Offered o disable sin Registered es NotOffered sin abrir la confirmación ni ejecutar nada; los textos del diálogo son los de la DS en español e inglés; lang_from_apple_languages elige Es para una salida cuyo primer idioma empieza por es y En para el resto
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test autostart
tests:
  - apps/desktop/src-tauri/tests/autostart.rs

[3] id: DSK002-CMDS
statement: T007: engine_autostart_offer y engine_autostart están en commands.json (lista exacta de siete), en el handler y con su allow-<comando> en la capability, y esta no contiene ningún dialog:*; EngineAutostartParams exige enable y rechaza campos desconocidos; engine_autostart_offer solo acepta params vacíos; CommandError serializa not-offered, cancelled y failed; lib.rs registra .plugin(tauri_plugin_dialog::init()); Cargo.toml fija tauri-plugin-dialog =2.8.3; commands.ts y su test de igualdad listan los siete comandos
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test autostart_commands --test commands && pnpm -C apps/desktop exec vitest run src/client/commands.test.ts
tests:
  - apps/desktop/src-tauri/tests/autostart_commands.rs
  - apps/desktop/src/client/commands.test.ts

[4] id: DSK002-UI
statement: T007 (D-A): AutostartOffer con offer offered muestra la acción de activar y con registered la de dejar de arrancar, llama a onChange(true) o onChange(false) al pulsarla, queda aria-disabled con pending, y no tiene violaciones de axe en claro y oscuro, en español e inglés; AutostartSection (montada por AppShell bajo EngineBand) solo muestra AutostartOffer con el motor conectado y la oferta offered o registered, y al pulsar invoca engine_autostart con { enable }; los errores not-offered, cancelled y failed tienen texto en en.json y es.json
verify: pnpm -C apps/desktop exec vitest run src/engine/AutostartOffer.test.tsx src/engine/AutostartSection.test.tsx
tests:
  - apps/desktop/src/engine/AutostartOffer.test.tsx
  - apps/desktop/src/engine/AutostartSection.test.tsx

[5] id: DSK002-DMG-ASSETS
statement: T008: stage-sidecars.sh copia raptor y raptor-mcp a binarios/raptor-<triple> y raptor-mcp-<triple> con modo 755 y falla si falta alguno; select-assets.sh adjunta en las alphas el .dmg GitRaptor-<versión>-aarch64-apple-darwin.dmg y su .sha256 además de lo que ya adjuntaba, y en una versión estable adjunta todo; en la alpha el SHA256SUMS incluye la línea del .dmg y existe su .sha256
verify: bash tools/desktop/stage-sidecars.test.sh && bash tools/release/select-assets.test.sh
tests:
  - tools/desktop/stage-sidecars.test.sh
  - tools/release/select-assets.test.sh

[6] id: DSK002-RELEASE-JOB
statement: T008: release.yml tiene el job desktop-app (name 'desktop app (aarch64-apple-darwin)', needs plan y build, runs-on macos-latest, permissions contents: read, sin secrets.*, con tauri build --bundles dmg --config src-tauri/tauri.release.conf.json, hdiutil attach -nobrowse -readonly y la comprobación de --version de raptor y raptor-mcp dentro de GitRaptor.app); el .dmg se renombra a GitRaptor-<versión>-aarch64-apple-darwin.dmg y se sube con upload-artifact archive-desktop-aarch64-apple-darwin; assemble lo lista en needs y calcula el .sha256 de GitRaptor-*.dmg; tauri.release.conf.json es exactamente externalBin raptor y raptor-mcp, targets dmg y signingIdentity -; .gitignore ignora apps/desktop/src-tauri/binaries/
verify: bash tools/desktop/release-job.test.sh && pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test tauri_surface
tests:
  - tools/desktop/release-job.test.sh
  - apps/desktop/src-tauri/tests/tauri_surface.rs

[7] id: DSK002-T009-SCRIPT
statement: T009: el guion docs/dev-briefs/ts-dsk-002-t009-verificacion-macos.md existe, recoge los cuatro pasos de la DS con comandos y lo que hay que observar, y el paso 3 usa la fila de autoarranque de la app y cita ~/Library/LaunchAgents/dev.gitraptor.plist
verify: bash tools/desktop/t009-script.test.sh
tests:
  - tools/desktop/t009-script.test.sh

[8] id: DSK002-BOUNDARY
statement: V1, V2 y V7 siguen en verde con el plugin de diálogo: gitraptor-desktop no alcanza gitraptor-core, -git ni -policy y no gana unsafe; el único plugin es tauri-plugin-dialog sin permisos para el webview; src/ no lee el entorno ni los argumentos
verify: pnpm -C apps/desktop run build && cargo test -p gitraptor-desktop --test boundaries --test environment --test tauri_surface
tests:
  - apps/desktop/src-tauri/tests/boundaries.rs
  - apps/desktop/src-tauri/tests/environment.rs
```
<<<FIN-DATOS-NO-CONFIABLES 48417b83af9c>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES 17b8e1b385ff>>>
```text
- El build de `tauri build --bundles dmg` no corre en local en esta corrida (sin firmar y lento); lo prueba el job `desktop-app` del CI del PR y T009.
- `tauri-plugin-dialog` añade crates al `Cargo.lock`: se deja que cargo añada lo mínimo.
- Windows y Linux: fuera (etapa de validación multiplataforma). Solo macOS aarch64.
```
<<<FIN-DATOS-NO-CONFIABLES 17b8e1b385ff>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES ede099eebd62>>>
```text
docs/dev-briefs/ts-dsk-002-autostart-dmg.prep.md
```
<<<FIN-DATOS-NO-CONFIABLES ede099eebd62>>>

planSha: `9f514fa5a82b9c8f0fc4486872398cecdc07f13818158aa1f1235789adf3a540` (base de la cadena de re-planes)

Léelo en `ts-dsk-002-autostart-dmg.prep.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES b188b2b9231b>>>
```text
docs/dev-briefs/ts-dsk-002-autostart-dmg.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES b188b2b9231b>>>

contractSha: `077e4fb4156419a128535737d428f04f5cf36cb3255c91a9cf634846a6851467`

## Tests de criterio

testsSha: `6052e5eff0ed6fccec42ee71b7c53337a87e7d164766f60ba151fb49c962e5be`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES e158620f4988>>>
```text
3ebb76d5fdfa3def8a629744f986450587090b665840b6504d48e0f3e75ba851  apps/desktop/src-tauri/tests/autostart.rs
a2122c0c9f0e2c6b557e93d0ce201dfb72d38243e7a91738936ad1d40d5c4126  apps/desktop/src-tauri/tests/autostart_commands.rs
60ce34a2eeacc2464f4d945511e669a764bcc0afdb17d4ff254c5977f8f6252c  apps/desktop/src-tauri/tests/autostart_offer.rs
094369187aa7f32421a4246c7041d98195b0749c822a6a48392e3ce585e071c8  apps/desktop/src-tauri/tests/boundaries.rs
8046e35a65526a9f73ce47139e019dcf7d780d2788e905e71fd3beadd31e1b39  apps/desktop/src-tauri/tests/environment.rs
18e667946a5206a78946d89d405870fd6f8a8556a2d3fbc2e27f65febf06e1dd  apps/desktop/src-tauri/tests/tauri_surface.rs
639092980753593c8827ab596533995a89c7ed34f745e91b534b8d489379393a  apps/desktop/src/client/commands.test.ts
892dfa21683b1280590c57bbdc88087e383c51feacc14d844a40583b7c2f2639  apps/desktop/src/engine/AutostartOffer.test.tsx
8338d68090555a25d0e5821b759fd21fea1352d16d8edd7501f15dd6211603a8  apps/desktop/src/engine/AutostartSection.test.tsx
429165165f722b0b7f1821791f42d474431afa8bbe6be7a3858eff820f9afecc  tools/desktop/release-job.test.sh
e33b395c6bc8731e8a3feec81b29624c1f5919429e69c7d18c1ca01870674df9  tools/desktop/stage-sidecars.test.sh
de7e9694947fcb199e2f4f6ad06d04483c5d1e883a65a6a853c73cf341bde3f5  tools/desktop/t009-script.test.sh
ef9dba24d5ad8422512a5ab465e9bca8c192623ec80aef44713c120deca31b8e  tools/release/select-assets.test.sh
```
<<<FIN-DATOS-NO-CONFIABLES e158620f4988>>>

## Crítico

- Rondas (plan): 2
  - ronda 1: AJUSTES · 4 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: AJUSTES · 1 bloqueante(s) · siguiente: excepcion
- Último veredicto: AJUSTES
- Excepción: sí — se aprueba con los hallazgos abiertos como diferidos, o se para el paquete

## Diferidos

### Diferidos propuestos

1 hallazgo(s) bloqueante(s) que el crítico dejó abiertos, copiados tal cual:

<<<DATOS-NO-CONFIABLES 6a5a83335d88>>>
```text
id: PC-9
kind: test
toHuman: false
evidence: docs/dev-briefs/ts-dsk-002-autostart-dmg.contract.json:41
ask: Añadir a autostart_commands.rs (DSK002-CMDS) una comprobacion de la orquestacion de engine_autostart: recalcula la oferta con ensure --no-start mas offer, usa blocking_show dentro de spawn_blocking como confirm de run_choice, sin confirm constante; y fijar la serializacion de la respuesta de engine_autostart_offer.
```
<<<FIN-DATOS-NO-CONFIABLES 6a5a83335d88>>>

## Firmas

- planSha (contexto, no firmado): `9f514fa5a82b9c8f0fc4486872398cecdc07f13818158aa1f1235789adf3a540`
- contractSha: `077e4fb4156419a128535737d428f04f5cf36cb3255c91a9cf634846a6851467`
- testsSha: `6052e5eff0ed6fccec42ee71b7c53337a87e7d164766f60ba151fb49c962e5be`
- approvalSha: `9f514fa5a82b9c8f0fc4486872398cecdc07f13818158aa1f1235789adf3a540:077e4fb4156419a128535737d428f04f5cf36cb3255c91a9cf634846a6851467:6052e5eff0ed6fccec42ee71b7c53337a87e7d164766f60ba151fb49c962e5be`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
