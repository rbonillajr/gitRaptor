---
id: ADR-GRP-014
title: Pipeline de release y canales de distribución
type: adr
status: accepted
date: 2026-10-05
created: 2026-10-05
updated: 2026-10-05
deciders: [Rene Bonilla]
domain: GRP
feature: motor-local
related: [BRD-GRP-001, ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, INF-GRP-003, INF-GRP-004, INF-GRP-002, TS-GRP-001, TS-GRP-002]
tags: [release, licencia, fsl, open-core, distribucion, ci, firma, notarizacion, checksums, sbom, attestation, homebrew, winget, npm, instalador, nfr-06, nfr-03, nfr-11, sec-07, sec-14]
---

# ADR-GRP-014 — Pipeline de release y canales de distribución

> **Estado**: aceptado (2026-10-05). Decisión del orquestador (2026-10-05), validada por el Arquitecto y el PO. El nombre y la licencia (§ 6) son decisión de Rene Bonilla (2026-10-05, D4 y D5 del documento de negocio); el paso a `accepted` es decisión del orquestador, validada por el Arquitecto.

## Contexto

NFR-06 pide un **binario único** para Windows, macOS y Linux, en x64 y arm64, que se instale con `winget`, `brew`, `npm`/`npx` y un script. Ninguna historia ni enabler cubría el release: no había forma reproducible de producir esos binarios ni de demostrar su integridad.

Hay cinco restricciones:

- **NFR-03 (100 % local)**: el motor no abre la red. La descarga es del instalador, nunca del binario.
- **Política de CI**: las acciones de GitHub van fijadas por SHA, igual que en `repo-intact.yml`.
- **Decisión v0.3 del negocio**: GitRaptor arranca como herramienta interna. Por eso ahora **no se publica nada**: publicar es un paso humano.
- **Licencia**: Homebrew, winget y npm piden una licencia (NFR-11). Al proponer este ADR el workspace declaraba `license = "UNLICENSED"` y el nombre y la licencia seguían abiertos (pregunta abierta 2). Los cierra el § 6.
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

`packaging/render-channels.mjs` genera los canales de una release a partir de los archivos y de `SHA256SUMS`, en cada release y como artefacto. Su **publicación** vive en `release-channels.yml`, que solo corre cuando un humano publica el borrador, la variable `RELEASE_PUBLISH_CHANNELS` vale `true`, la release no es prerelease y todos los manifiestos declaran la licencia (§ 6).

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

### 6. Nombre y licencia

Decisión de Rene Bonilla (2026-10-05), D4 y D5 del documento de negocio:

- **Nombre en los canales: `gitraptor`.** Fórmula `gitraptor` en el tap propio, `GitRaptor.GitRaptor` en winget y `gitraptor` más `@gitraptor/cli-<os>-<cpu>` en npm. **El comando sigue siendo `raptor`** (y `raptor-mcp`).
- **Licencia: FSL-1.1-ALv2** (Functional Source License 1.1, con licencia futura Apache-2.0). El núcleo (motor, CLI/TUI, MCP, Time Machine y Guardrails individuales) es gratis para cualquier usuario. La edición de equipo de pago, con licencia comercial, llegará más adelante (BR-23 y BR-25) y no se construye ahora.
- **Dónde se declara:**
  - `LICENSE` en la raíz, con el texto oficial de [getsentry/fsl.software](https://github.com/getsentry/fsl.software/blob/85f3fc7ed6d487a49b70dc2d02e2790fd6242467/FSL-1.1-ALv2.template.md). Solo se rellena el aviso: año 2026, licenciante Rene Bonilla. La plantilla no tiene campo para el nombre del software: "el Software" es lo que se distribuye junto a este texto.
  - `license = "FSL-1.1-ALv2"` en `[workspace.package]`, que heredan todos los crates y apps, y en los dos spikes, que son workspaces aparte.
  - `"license": "FSL-1.1-ALv2"` en cada `package.json`.
  - En los canales que genera `render-channels.mjs`: `license` en la fórmula, `License` y `LicenseUrl` (el `LICENSE` del tag de esa versión) en winget y `license` en los siete paquetes npm. Los archivos de la release y los paquetes npm llevan el `LICENSE`. Si el workspace no declara licencia, el render falla.
- **Identificador SPDX.** `FSL-1.1-ALv2` está en la SPDX License List (comprobado en la 3.29) y no es OSI. Lo reconoce Homebrew, cuya copia de la lista SPDX es la 3.29. En npm está en `spdx-license-ids` 3.0.24, la versión actual; npm 10.9.3 trae la 3.0.21, que no lo tiene, y aun así `npm pack` no avisa. winget acepta texto libre. Cargo no valida la licencia mientras `publish = false`. No hace falta un identificador alternativo; si una herramienta antigua no lo reconociera, la alternativa es `license-file = "LICENSE"` en Cargo y `"license": "SEE LICENSE IN LICENSE"` en npm.
- **Homebrew.** homebrew-core solo acepta licencias libres, así que la FSL refuerza el tap propio: `gitraptor` no puede ir a homebrew-core.
- **Control.** `tools/check-license.sh` comprueba con `cargo metadata` y `jq` que todos los crates y `package.json` declaran FSL-1.1-ALv2 y que `LICENSE` es el texto oficial (SHA-256 fijado). Falla si `cargo metadata` falla o si no encuentra ningún crate o paquete. Lo ejecuta `license.yml` en cada PR y `release-channels.yml` antes de publicar.

## Alternativas consideradas

| Alternativa | Por qué no |
|---|---|
| cargo-dist | Ver § 1: regenera su workflow, no cubre winget ni la notarización y choca con la fijación por SHA |
| Linux con glibc (`*-linux-gnu`) | Depende de la versión de glibc del sistema; no es "binario único" |
| Compilación cruzada con `cross` o `cargo-zigbuild` | Los runners arm64 nativos son gratuitos en repos públicos y evitan una capa más |
| npm con `postinstall` o shim que descarga | Ver § 5 |
| Publicar los canales desde `release.yml` | Publicaría con la release aún en borrador. Separarlo deja la publicación detrás de un paso humano |
| Licencia permisiva (MIT o Apache-2.0) desde el día uno | Descartada por D4: no protege el núcleo frente a un servicio competidor mientras la edición de equipo no exista. La FSL pasa a Apache-2.0 a los dos años |
| Attestations siempre activas | Cada una es una entrada pública e irreversible en Sigstore, incluso para un tag de prueba |

## Consecuencias

- ✅ Un tag produce los 6 binarios con checksums, SBOM, scripts probados, canales generados y un borrador. Nada sale al público sin un paso humano.
- ✅ Los secretos que faltan no rompen el pipeline: el paso se salta y avisa con "requiere secreto: X".
- ⚠️ El canal npm instala el binario en `node_modules` o en la caché de `npx`, que no es una ruta canónica: `raptor daemon enable` debe rechazarlo (SEC-14) y orientar a otro canal. Lo cierra la Dev Spec de TS-GRP-003.
- ✅ El nombre y la licencia están decididos (§ 6): ya no bloquean los canales.
- ⚠️ Antes de publicar en cualquier canal quedan tres pasos humanos: reservar `gitraptor` y `@gitraptor` en npm, crear el tap y configurar los secretos. Antes del lanzamiento comercial se recomienda una revisión legal de la licencia.
- ⚠️ Windows sigue con pendientes que bloquean una release real en ese SO: la ACL del perfil (TS-GRP-001, SEC-06) y las ACE de `git.exe` (TS-GRP-002).
- ⚠️ El pipeline no aplica `cargo-deny` (licencias, NFR-11). Queda como pendiente de SEC-07, fuera de este enabler. Cuando se configure, revisará también los crates propios y fallaría con la FSL: se eximen con `[licenses.private] ignore = true` (todos son `publish = false`) y FSL-1.1-ALv2 no entra en la lista permitida para dependencias de terceros.
- ⚠️ Con un licenciante persona física y una edición comercial prevista, conviene un CLA o DCO antes de aceptar contribuciones externas. Entra en la revisión legal recomendada, junto con quién es el titular de la propiedad intelectual.

## Validación

1. Un tag de prueba ejecuta `release.yml` y produce los 6 archivos con `SHA256SUMS`, los SBOM, los canales generados y un borrador, que se borra después junto con el tag. La evidencia queda enlazada en el PR de INF-GRP-003.
2. `install.sh` (Linux y macOS) e `install.ps1` (Windows) instalan desde los archivos del run y rechazan un checksum manipulado sin instalar nada.
3. En macOS, `raptor --version` corre en arm64 y, con Rosetta, en x86_64. En Linux, `file` confirma que el binario es estático.
4. `release-channels.yml` no corre mientras `RELEASE_PUBLISH_CHANNELS` no valga `true`.
5. `license.yml` está en verde: todos los crates y paquetes declaran FSL-1.1-ALv2 y `LICENSE` coincide con el SHA-256 del texto oficial con el aviso rellenado. Los canales generados declaran la misma licencia.

## Referencias

- NFR-03, NFR-06, NFR-11, SEC-07 y SEC-14 en [non-functional.md](../non-functional.md).
- [ADR-GRP-001](./ADR-GRP-001-stack-tecnologico.md) (stack), [ADR-GRP-005](./ADR-GRP-005-forma-motor-proceso-segundo-plano.md) § 4 (binario instalado y actualización) y [ADR-GRP-006](./ADR-GRP-006-perfil-ubicacion-almacenamiento.md) (PQ-7).
- Enablers: [INF-GRP-003](../../requirements/features/motor-local/technical-stories/INF-GRP-003-pipeline-release.md) y [INF-GRP-004](../../requirements/features/motor-local/technical-stories/INF-GRP-004-canales-distribucion.md).
- Documento de negocio: decisión v0.3 (herramienta interna), D4 (open core y FSL-1.1-ALv2) y D5 (nombre), que cierran la pregunta abierta 2.
- Licencia: [LICENSE](../../../LICENSE) y [fsl.software](https://fsl.software).
