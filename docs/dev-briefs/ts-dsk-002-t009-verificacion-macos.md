# Guion T009 · Verificación de punta a punta en macOS (TS-DSK-002)

Lo ejecuta el coordinador, no Rene, salvo donde un paso lo exige (un clic en el diálogo del sistema es humano: ver el paso 3). Resultado de cada paso, en la descripción del PR. Máquina: macOS Apple silicon. Dev Spec: [DS-TS-DSK-002](../requirements/features/desktop/dev-specs/TS-DSK-002-arranque-motor-binarios-bundle.md), T009.

> **Perfil real.** La app limpia el entorno al ejecutar `raptor`, así que `GITRAPTOR_PROFILE_DIR` no llega al motor: estos pasos usan el perfil real del usuario que los ejecuta (daemon en su carpeta de usuario y, en el paso 3, el LaunchAgent). Se limpia al final. No registres repos de prueba: la app no pide ninguno y no hace falta abrir ninguno.

## Preparación · construir el `.dmg` en local

El CI del PR no construye el `.dmg` (el job `desktop-app` corre en la alpha, con el tag). Se construye aquí con los mismos pasos que el job. Desde la raíz del worktree:

```sh
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)   # versión del workspace
T=aarch64-apple-darwin
cargo build --release --locked --target $T -p gitraptor-cli -p gitraptor-mcp
sh tools/desktop/stage-sidecars.sh $T target/$T/release
pnpm install --frozen-lockfile --ignore-scripts
pnpm --filter @gitraptor/desktop exec tauri build --target $T --bundles dmg --config src-tauri/tauri.release.conf.json
ls target/$T/release/bundle/dmg/
```

Observar: aparece un `.dmg` sin firmar. Montarlo y comprobar los binarios del motor dentro:

```sh
hdiutil attach -nobrowse -readonly -mountpoint /tmp/gr-dmg target/$T/release/bundle/dmg/*.dmg
/tmp/gr-dmg/GitRaptor.app/Contents/MacOS/raptor --version        # raptor <versión>
# raptor-mcp no tiene --version y arrancaría el motor: no se ejecuta; se comprueba que está y su versión
ls -l /tmp/gr-dmg/GitRaptor.app/Contents/MacOS/raptor-mcp
LC_ALL=C grep -a -c -F "$VERSION" /tmp/gr-dmg/GitRaptor.app/Contents/MacOS/raptor-mcp   # > 0
hdiutil detach /tmp/gr-dmg
```

Antes de empezar, sin otro `raptor`: `pgrep -fl "raptor daemon"` no devuelve nada y `ls ~/Library/LaunchAgents/dev.gitraptor.plist` no existe. Si hay un motor o un plist previos, anota su estado y retíralos al final (limpieza).

Instala la app: montar el `.dmg`, arrastrar `GitRaptor.app` a `/Applications` y expulsar la imagen. Al ser un `.dmg` sin firmar, la primera vez: clic derecho, Abrir (o `xattr -dr com.apple.quarantine /Applications/GitRaptor.app`).

## Paso 1 · "Arrancar motor" arranca el motor del bundle

Con el `.dmg` instalado en `/Applications` y sin otro `raptor` instalado (`ls ~/.local/bin/raptor` no existe; `command -v raptor` vacío):

```sh
open /Applications/GitRaptor.app
```

En la app: pulsar **Arrancar motor** (banda de estado del motor). Observar:

- la banda pasa a motor conectado;
- `ps -axo pid,command | grep "[r]aptor"` muestra el daemon con la ruta `/Applications/GitRaptor.app/Contents/MacOS/raptor`, no otra;
- sigue sin existir `~/Library/LaunchAgents/dev.gitraptor.plist`.

## Paso 2 · un motor de otra instalación no se reemplaza

Parar el motor del paso 1 (`/Applications/GitRaptor.app/Contents/MacOS/raptor daemon stop`) y cerrar la app. Instalar un `raptor` de una versión anterior con `install.sh` (alpha anterior) y arrancar su daemon:

```sh
curl -fsSL https://github.com/<org>/gitRaptor/releases/download/<tag-anterior>/install.sh | sh
~/.local/bin/raptor daemon status       # arranca el motor de esa versión si no está en marcha
pgrep -fl "raptor daemon"               # la ruta es ~/.local/bin/raptor
open /Applications/GitRaptor.app
```

Observar: la app muestra **Motor incompatible** con la ruta del daemon (`~/.local/bin/raptor`) y la versión, y **no lo reemplaza**: `pgrep -fl "raptor daemon"` sigue mostrando el mismo pid y la misma ruta antes y después de abrir la app (anota el pid). Pulsar el botón de la banda no debe sustituirlo. Limpieza: `~/.local/bin/raptor daemon stop` y `rm ~/.local/bin/raptor ~/.local/bin/raptor-mcp`.

## Paso 3 · autoarranque desde la app (diálogo del sistema)

Con el motor del bundle en marcha (paso 1) y la app abierta desde `/Applications`: debe verse la fila de autoarranque ("Arrancar GitRaptor al iniciar sesión" / "Start GitRaptor at login"). **Este paso lo hace una persona**: el diálogo es del sistema y el webview no puede pulsarlo.

1. Pulsar la fila de autoarranque. Observar el diálogo nativo con título "Arrancar GitRaptor al iniciar sesión", el texto "GitRaptor arrancará su motor cada vez que inicies sesión. Si no lo pediste tú, cancela." y los botones **Activar** y **Cancelar** (en inglés con el SO en inglés).
2. Pulsar **Cancelar**: `ls ~/Library/LaunchAgents/dev.gitraptor.plist` falla (no existe) y la fila sigue ofreciendo activar.
3. Pulsar la fila otra vez y **Activar**: `cat ~/Library/LaunchAgents/dev.gitraptor.plist` existe y su ruta de programa es `/Applications/GitRaptor.app/Contents/MacOS/raptor`; la fila ahora ofrece dejar de arrancar.
4. Pulsar la fila, **Cancelar**: el plist sigue como estaba. Pulsar otra vez y confirmar el diálogo de "Dejar de arrancar…": `ls ~/Library/LaunchAgents/dev.gitraptor.plist` falla.

## Paso 4 · app abierta desde la imagen montada: sin oferta

Dejar el motor del bundle en marcha, expulsar y volver a montar el `.dmg` y abrir la app **desde la imagen** (`open "/Volumes/GitRaptor/GitRaptor.app"`), con la copia de `/Applications` cerrada.

Observar: no aparece la fila de autoarranque. En el backend esto es `engine_autostart_offer` respondiendo `not-offered` con motivo `location` (una ruta bajo `/Volumes/`); desde la UI solo se ve como ausencia de la fila, y la regla la fija el test `tests/autostart_offer.rs`. Tampoco aparece con la app en `/Volumes/` si el motor ya tiene el plist (no debe crearse nada). Comprobar que `ls ~/Library/LaunchAgents/dev.gitraptor.plist` sigue sin existir.

## Limpieza

```sh
/Applications/GitRaptor.app/Contents/MacOS/raptor daemon disable 2>/dev/null
/Applications/GitRaptor.app/Contents/MacOS/raptor daemon stop 2>/dev/null
rm -f ~/Library/LaunchAgents/dev.gitraptor.plist
rm -rf /Applications/GitRaptor.app
hdiutil detach /Volumes/GitRaptor 2>/dev/null
```

Si había un motor o un plist previos, restaurarlos como estaban.

## Resultados (a rellenar en el PR)

| Paso | Resultado | Observación |
|---|---|---|
| Preparación (`.dmg`) | | |
| 1 · Arrancar motor | | |
| 2 · No reemplaza otro motor | | |
| 3 · Diálogo, Cancelar y Activar | | |
| 4 · Desde la imagen montada | | |
