---
id: ADR-GRP-014
title: Pipeline de release y canales de distribución
type: adr
status: proposed
date: 2026-10-05
created: 2026-10-05
updated: 2026-10-05
deciders: [Rene Bonilla]
domain: GRP
feature: motor-local
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, INF-GRP-003, INF-GRP-004, INF-GRP-002, TS-GRP-001, TS-GRP-002]
tags: [release, distribucion, ci, firma, notarizacion, checksums, sbom, attestation, homebrew, winget, npm, instalador, nfr-06, nfr-03, nfr-11, sec-07, sec-14]
---

# ADR-GRP-014 — Pipeline de release y canales de distribución

> **Estado**: propuesto. Decisión del orquestador (2026-10-05), validada por el Arquitecto y el PO. Falta que Rene Bonilla la acepte.

## Contexto

NFR-06 pide un **binario único** para Windows, macOS y Linux, en x64 y arm64, que se instale con `winget`, `brew`, `npm`/`npx` y un script. Ninguna historia ni enabler cubría el release: no había forma reproducible de producir esos binarios ni de demostrar su integridad.

Hay cinco restricciones:

- **NFR-03 (100 % local)**: el motor no abre la red. La descarga es del instalador, nunca del binario.
- **Política de CI**: las acciones de GitHub van fijadas por SHA, igual que en `repo-intact.yml`.
- **Decisión v0.3 del negocio**: GitRaptor arranca como herramienta interna, y el nombre y la licencia siguen abiertos (pregunta abierta 2). Por eso ahora **no se publica nada**.
- **Licencia**: el workspace declara `license = "UNLICENSED"`. Homebrew, winget y npm piden licencia (NFR-11).
- **SEC-14 y ADR-GRP-005 § 4**: el autoarranque y la petición de "versión incompatible" solo se aceptan del **binario instalado**, así que cada canal tiene que fijar una ruta canónica.

## Decisión

### 1. Workflow propio en lugar de cargo-dist

`release.yml` es un workflow escrito a mano. Se descarta cargo-dist por tres razones:

- cargo-dist genera y regenera su propio workflow, lo que choca con la fijación por SHA y con cualquier edición a mano.
- No cubre winget ni la notarización completa de macOS.
- El toolchain fijado en `rust-toolchain.toml` (1.99) ya lo gobierna rustup.

**Coste**: los instaladores y el render de los canales los mantenemos nosotros (`packaging/`).

### 2. Disparo, versión y targets

- Se dispara con un tag `v*`. `workflow_dispatch` solo reconstruye un tag que ya existe. Los PR que tocan el pipeline ejecutan el mismo build como prueba, sin release.
- El tag tiene que coincidir **exactamente** con la versión del workspace, sufijo de prerelease incluido. Así `raptor --version`, los archivos y los manifiestos nombran la misma versión.
- Antes de compilar, `cargo audit` bloquea la release si hay una vulnerabilidad conocida en `Cargo.lock` (SEC-07).
- La matriz es nativa, en runners gratuitos para repos públicos:

| Target | Runner | Nota |
|---|---|---|
| `x86_64-unknown-linux-musl` | `ubuntu-latest` | Estático con `musl-gcc`, porque `rusqlite` compila SQLite con `cc` |
| `aarch64-unknown-linux-musl` | `ubuntu-24.04-arm` | Ídem |
| `x86_64-apple-darwin` | `macos-latest` | Compilado cruzado desde Apple silicon y probado con Rosetta |
| `aarch64-apple-darwin` | `macos-latest` | — |
| `x86_64-pc-windows-msvc` | `windows-latest` | — |
| `aarch64-pc-windows-msvc` | `windows-11-arm` | — |

- Linux usa **musl estático**: un binario que corre igual en glibc y en musl. **Riesgo**: el allocator de musl es más lento. El banco INF-GRP-002 debe medir NFR-04 y NFR-05 sobre el binario musl y, si no llega, se cambia el allocator (por ejemplo, mimalloc) sin cambiar de target.
- Los builds de release **nunca restauran caché**, para que un PR no pueda envenenarlos.
- La prueba de humo es solo `raptor --version`. `raptor-mcp` arrancaría el motor y tocaría el perfil (NFR-01).

### 3. Artefactos, integridad y borrador

- Cada target produce `raptor-<versión>-<target>.tar.gz` (`.zip` en Windows), con una carpeta raíz que contiene `raptor`, `raptor-mcp` y el README.
- Integridad: un `.sha256` por archivo y un `SHA256SUMS` agregado que se comprueba en el propio job.
- SBOM: uno en CycloneDX por crate entregado (`apps/cli` y `apps/mcp`), generado con `cargo-cyclonedx` desde `Cargo.lock`.
- Attestation de procedencia (`actions/attest-build-provenance`, OIDC y sin secreto) **solo si la variable de repo `RELEASE_ATTEST` vale `true`**. Para un repo público, cada attestation es una entrada pública e irreversible en Sigstore, así que no se activa hasta la primera release real. La firma del SBOM con `attest-sbom` queda **pendiente** para ese momento, con la misma variable.
- `install.sh` e `install.ps1` se adjuntan a la release y **se prueban en CI** contra los archivos recién construidos: instalan, verifican y rechazan un `SHA256SUMS` manipulado sin instalar nada.
- El resultado es un **borrador** de GitHub Release (`--draft`, y `--prerelease` si la versión lleva `-`). El workflow nunca lo publica y nunca reemplaza los assets de una release ya publicada: **publicar es un paso humano**.
- Permisos por job: `contents: write` solo donde se crea el borrador; `id-token` y `attestations: write` solo donde se atestigua o se firma.
- ⚠️ **Pendiente (humano)**: activar las releases inmutables del repo antes de la primera release real.

### 4. Firma

- **macOS**: Developer ID con *hardened runtime* y `--timestamp`, y notarización con `notarytool` y una clave de App Store Connect. Se notariza un zip, porque a un binario suelto no se le puede grapar el ticket: Gatekeeper lo consulta en línea. Esa consulta la hace el SO, no el motor, así que NFR-03 se cumple. Sin secretos, el binario solo lleva la firma ad hoc del linker y el job avisa.
  - Requiere secreto: `APPLE_CERTIFICATE_P12_BASE64`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_API_KEY_P8_BASE64`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER_ID`.
- **Windows**: Azure Artifact Signing (antes Trusted Signing) con credencial federada por OIDC (`azure/login`), sin *client secret*. Sin secretos, el binario queda sin firmar y el job avisa.
  - Requiere secreto: `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_CERTIFICATE_PROFILE`.
  - ⚠️ **ASSUMPTION**: Artifact Signing para personas solo existe en EE. UU. y Canadá; si no aplica, se usa `signtool` con un certificado OV/EV en la misma posición del job. Tampoco está verificado que la acción corra en `windows-11-arm`. Pendiente: etapa de validación multiplataforma.

### 5. Canales (INF-GRP-004)

`packaging/render-channels.mjs` genera los canales de una release a partir de los archivos y de `SHA256SUMS`, en cada release y como artefacto. Su **publicación** vive en `release-channels.yml`, que solo corre cuando un humano publica el borrador, la variable `RELEASE_PUBLISH_CHANNELS` vale `true`, la release no es prerelease y la licencia ya no es `UNLICENSED`.

| Canal | Forma | Ruta instalada | Requiere |
|---|---|---|---|
| Homebrew | Fórmula `gitraptor` en el tap propio `rbonillajr/homebrew-tap`, con URL y sha256 por SO y CPU. Se llama `gitraptor` porque homebrew-core ya tiene `raptor` (la biblioteca RDF). Sin bloque `service`: el autoarranque es solo `raptor daemon enable` (PQ-1) | `$(brew --prefix)/bin` | Secreto `HOMEBREW_TAP_TOKEN` |
| winget | Manifiesto multiarchivo (1.9.0), `InstallerType: zip` + `NestedInstallerType: portable`, con dos `NestedInstallerFiles` y su `PortableCommandAlias` (`raptor` y `raptor-mcp`); PR a `microsoft/winget-pkgs` con `wingetcreate` | Carpeta de portables de winget | Secreto `WINGET_TOKEN` |
| npm | Paquete `gitraptor` con un *launcher* Node sin dependencias, más un paquete por plataforma, `@gitraptor/cli-<os>-<cpu>`, en `optionalDependencies` (el patrón de esbuild y Biome). Sin `postinstall` y sin descargas en tiempo de ejecución. Se publica con *trusted publishing* (OIDC y provenance), sin `NPM_TOKEN` | `node_modules` o la caché de `npx` | Trusted publisher configurado en npmjs.com |
| Script | `install.sh` (POSIX) e `install.ps1`: descargan el archivo y `SHA256SUMS`, verifican el checksum y solo entonces instalan. Fallan si no hay herramienta de hash. Nunca tocan el PATH ni el autoarranque | `~/.local/bin`; en Windows, `%LOCALAPPDATA%\Programs\GitRaptor\bin` (PQ-7) | — |

- **npm: por qué paquetes por plataforma y no un shim que descarga.** Con el shim, `raptor-mcp` lanzado por Claude Code descargaría en su primer uso: un proceso lanzado por un agente abriría la red y arriesgaría el *timeout* del stdio del MCP. Además, pnpm 10 bloquea los `postinstall` y `--ignore-scripts` es habitual. Publicar 7 paquetes es solo un bucle en el mismo job, y npm verifica la integridad de cada uno.
- **Checksum frente a autenticidad.** Un `SHA256SUMS` del mismo origen que el archivo prueba integridad, no autenticidad. Los scripts recomiendan `gh attestation verify` cuando las attestations estén activas.
- **Actualización en Windows.** Con el daemon en marcha, `raptor.exe` está bloqueado. `install.ps1` renombra el ejecutable en uso a `.old` (Windows lo permite) antes de copiar el nuevo, y el daemon antiguo se para por "versión incompatible" (ADR-GRP-005 § 4). Para winget queda pendiente comprobarlo. Pendiente: etapa de validación multiplataforma.
- **El comando instalado es `raptor` en todos los canales**, y se actualiza por el mismo canal con el que se instaló. Desinstalar con cualquier canal borra solo los binarios, nunca el perfil ni la Time Machine (NFR-01).

## Alternativas consideradas

| Alternativa | Por qué no |
|---|---|
| cargo-dist | Ver § 1: regenera su workflow, no cubre winget ni la notarización y choca con la fijación por SHA |
| Linux con glibc (`*-linux-gnu`) | Depende de la versión de glibc del sistema; no es "binario único" |
| Compilación cruzada con `cross` o `cargo-zigbuild` | Los runners arm64 nativos son gratuitos en repos públicos y evitan una capa más |
| npm con `postinstall` o shim que descarga | Ver § 5 |
| Publicar los canales desde `release.yml` | Publicaría con la release aún en borrador. Separarlo deja la publicación detrás de un paso humano |
| Attestations siempre activas | Cada una es una entrada pública e irreversible en Sigstore, incluso para un tag de prueba |

## Consecuencias

- ✅ Un tag produce los 6 binarios con checksums, SBOM, scripts probados, canales generados y un borrador. Nada sale al público sin un paso humano.
- ✅ Los secretos que faltan no rompen el pipeline: el paso se salta y avisa con "requiere secreto: X".
- ⚠️ El canal npm instala el binario en `node_modules` o en la caché de `npx`, que no es una ruta canónica: `raptor daemon enable` debe rechazarlo (SEC-14) y orientar a otro canal. Lo cierra la Dev Spec de TS-GRP-003.
- ⚠️ Antes de publicar en cualquier canal quedan cuatro decisiones humanas: la licencia y el nombre (pregunta abierta 2, NFR-11), reservar `gitraptor` y `@gitraptor` en npm, crear el tap y configurar los secretos.
- ⚠️ Windows sigue con pendientes que bloquean una release real en ese SO: la ACL del perfil (TS-GRP-001, SEC-06) y las ACE de `git.exe` (TS-GRP-002).
- ⚠️ El pipeline no aplica `cargo-deny` (licencias, NFR-11). Queda como pendiente de SEC-07, fuera de este enabler.

## Validación

1. Un tag de prueba ejecuta `release.yml` y produce los 6 archivos con `SHA256SUMS`, los SBOM, los canales generados y un borrador, que se borra después junto con el tag. La evidencia queda enlazada en el PR de INF-GRP-003.
2. `install.sh` (Linux y macOS) e `install.ps1` (Windows) instalan desde los archivos del run y rechazan un checksum manipulado sin instalar nada.
3. En macOS, `raptor --version` corre en arm64 y, con Rosetta, en x86_64. En Linux, `file` confirma que el binario es estático.
4. `release-channels.yml` no corre mientras `RELEASE_PUBLISH_CHANNELS` no valga `true`.

## Referencias

- NFR-03, NFR-06, NFR-11, SEC-07 y SEC-14 en [non-functional.md](../non-functional.md).
- [ADR-GRP-001](./ADR-GRP-001-stack-tecnologico.md) (stack), [ADR-GRP-005](./ADR-GRP-005-forma-motor-proceso-segundo-plano.md) § 4 (binario instalado y actualización) y [ADR-GRP-006](./ADR-GRP-006-perfil-ubicacion-almacenamiento.md) (PQ-7).
- Enablers: [INF-GRP-003](../../requirements/features/motor-local/technical-stories/INF-GRP-003-pipeline-release.md) y [INF-GRP-004](../../requirements/features/motor-local/technical-stories/INF-GRP-004-canales-distribucion.md).
- Documento de negocio: decisión v0.3 (herramienta interna) y pregunta abierta 2 (nombre y licencia).
