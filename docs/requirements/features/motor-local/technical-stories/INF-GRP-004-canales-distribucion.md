---
id: INF-GRP-004
title: "Canales de distribución: Homebrew, winget, npm y scripts de instalación verificados"
type: inf
status: in-progress
feature: motor-local
domain: GRP
priority: high
complexity: medium
created: 2026-10-05
updated: 2026-10-05
related:
  adrs: [ADR-GRP-014, ADR-GRP-005, ADR-GRP-006]
  stories: [INF-GRP-003, TS-GRP-003]
  depends_on: [INF-GRP-003]
  specs: []
ado:
  id: null
  url: null
tags: [motor-local, release, distribucion, homebrew, winget, npm, instalador, checksums, nfr-06, nfr-03, nfr-11, sec-14]
---

## INF-GRP-004: Canales de distribución

**Valor**: cada release trae listos la fórmula de Homebrew, los manifiestos de winget, los paquetes npm y los scripts de instalación, verificados contra sus checksums. Publicarlos solo depende de los secretos y de un paso humano; la licencia ya está decidida (FSL-1.1-ALv2, ADR-GRP-014 § 6).

### Descripción

**Como** Arquitecto
**Quiero** generar y probar los cuatro canales de NFR-06 a partir de los artefactos de INF-GRP-003
**Para** que la primera publicación sea activar una variable, sin escribir nada nuevo (ADR-GRP-014 § 5)

> Dev Spec: N/A (brief compacto). Este documento más ADR-GRP-014 § 5 hacen de Dev Spec. Decisión del orquestador (2026-10-05), validada por el Arquitecto y el PO.
>
> **Depende de**: INF-GRP-003 (archivos y `SHA256SUMS`).

### Alcance Técnico

- **Crear** `packaging/install/install.sh` (POSIX) y `packaging/install/install.ps1`. Detectan el target, descargan el archivo y `SHA256SUMS`, verifican el checksum y solo entonces instalan `raptor` y `raptor-mcp` en `~/.local/bin` o en `%LOCALAPPDATA%\Programs\GitRaptor\bin`. Nunca tocan el PATH, el autoarranque ni los datos.
- **Crear** `packaging/render-channels.mjs`, que genera desde `SHA256SUMS`:
  - la fórmula `gitraptor` del tap propio, sin bloque `service`;
  - los manifiestos de winget (zip + portable, dos alias);
  - el paquete npm `gitraptor`, con el *launcher* `packaging/npm/launcher.cjs`, y los seis `@gitraptor/cli-<os>-<cpu>`.
- **Probar** en `release.yml` (job `test-installers`) los dos scripts contra los archivos del run, en Linux, macOS y Windows, incluido un `SHA256SUMS` manipulado que debe abortar sin instalar nada.
- **Crear** `.github/workflows/release-channels.yml`. Publica solo cuando un humano publica el borrador, `RELEASE_PUBLISH_CHANNELS == 'true'`, la release no es prerelease y todos los manifiestos declaran la licencia (`tools/check-license.sh`, ADR-GRP-014 § 6).
- **Fuera de alcance**: el registro del autoarranque (PQ-1, TS-GRP-003) y la US de instalación (ver "Pendientes").

### Requiere secreto o configuración (preparado, sin ejecutar)

| Canal | Requiere |
|---|---|
| Homebrew | Secreto `HOMEBREW_TAP_TOKEN` (contents:write en `rbonillajr/homebrew-tap`); crear el tap |
| winget | Secreto `WINGET_TOKEN` (public_repo, para el PR a `microsoft/winget-pkgs`) |
| npm | Trusted publisher configurado en npmjs.com para `gitraptor` y cada `@gitraptor/cli-*`; reservar el nombre y el scope (⚠️ **ASSUMPTION**: no se comprobó que estén libres) |
| Todos | Variable de repo `RELEASE_PUBLISH_CHANNELS = true`; licencia FSL-1.1-ALv2 declarada en todos los manifiestos (resuelto el 2026-10-05, ADR-GRP-014 § 6) |

### Decisiones de producto (PO, 2026-10-05)

Decisión del orquestador (2026-10-05), validada por el PO:

- En el MVP interno (decisión v0.3) bastan el script verificado y el tap propio. winget y npm quedan generados pero sin publicar, porque son registros públicos.
- El comando instalado es `raptor` en todos los canales, y se actualiza por el mismo canal con el que se instaló.
- Desinstalar borra solo los binarios, nunca el perfil ni los datos de la Time Machine (NFR-01).
- La licencia está resuelta (FSL-1.1-ALv2, ADR-GRP-014 § 6) y todos los canales la declaran; `license.yml` lo comprueba en cada PR.

### Plan de Verificación

#### Pruebas Automatizadas

- `install.sh` instala en Linux y macOS, y `install.ps1` en Windows, desde los archivos del run. `raptor --version` devuelve la versión.
- Con un `SHA256SUMS` manipulado, los dos scripts fallan y el directorio de destino no contiene `raptor`.
- `render-channels.mjs` falla si falta un target en `SHA256SUMS`.

#### Verificación Manual / Sandbox

- En macOS (2026-10-05): render completo con los archivos locales, `install.sh` desde `file://`, rechazo del checksum manipulado, y `npm install` de los tarballs `gitraptor` y `@gitraptor/cli-darwin-arm64`, con `raptor --version` por el *launcher*.

### Pendientes

- **US de instalación (PO)**: "El desarrollador instala `raptor` en un paso por el canal de su sistema". Se escribe cuando se decida activar la publicación.
- **SEC-14**: el canal npm no deja el binario en una ruta canónica. `raptor daemon enable` debe rechazarlo (Dev Spec de TS-GRP-003).
- **`raptor --version` con el canal de origen** (PO): queda para cuando exista la US de instalación.
- Actualización con winget mientras corre el daemon. Pendiente: etapa de validación multiplataforma.
- La validación de los manifiestos de winget con `winget validate` y la de la fórmula con `brew audit` se hacen antes de la primera publicación.
