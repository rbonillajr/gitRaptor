---
id: INF-GRP-003
title: "Pipeline de release: 6 targets, firma, checksums, SBOM y borrador de GitHub Release"
type: inf
status: in-progress
feature: motor-local
domain: GRP
priority: high
complexity: medium
created: 2026-10-05
updated: 2026-10-05
related:
  adrs: [ADR-GRP-014, ADR-GRP-001, ADR-GRP-005]
  stories: [INF-GRP-004, INF-GRP-002, TS-GRP-001, TS-GRP-002]
  specs: []
ado:
  id: null
  url: null
tags: [motor-local, ci, release, firma, notarizacion, checksums, sbom, attestation, nfr-06, sec-07]
---

## INF-GRP-003: Pipeline de release

**Valor**: un tag `v*` produce los binarios de NFR-06 con checksums y SBOM y deja un borrador de release. Nada se publica sin un paso humano.

### Descripción

**Como** Arquitecto
**Quiero** un workflow de release reproducible, disparado por tag, con builds nativos de los 6 targets
**Para** que los canales de INF-GRP-004 consuman artefactos verificables y la release no dependa de la máquina de nadie (NFR-06, ADR-GRP-014)

> Dev Spec: N/A (brief compacto). Este documento más ADR-GRP-014 hacen de Dev Spec (proceso AADD ligero). Decisión del orquestador (2026-10-05), validada por el Arquitecto: el release se divide en dos enablers, este (build y release) e INF-GRP-004 (canales).
>
> **Depende de**: nada para el build. Una release real en Windows depende de los pendientes de TS-GRP-001 (ACL, SEC-06) y TS-GRP-002 (ACE de `git.exe`).

### Alcance Técnico

- **Crear** `.github/workflows/release.yml`. Se dispara con un tag `v*`; `workflow_dispatch` solo reconstruye un tag que ya existe; los PR que tocan el pipeline o `packaging/**` lo ejecutan como prueba, sin release.
- **Validar** en el job `plan` que el tag coincide exactamente con la versión del workspace, y pasar `cargo audit` (SEC-07).
- **Compilar** los 6 targets de ADR-GRP-014 § 2 en runners nativos, con `--locked` y sin restaurar caché. Linux con musl estático.
- **Probar** `raptor --version` en cada target (x86_64 de macOS con Rosetta) y comprobar con `file` que el binario de Linux es estático.
- **Firmar**: en macOS, Developer ID + notarización; en Windows, Azure Artifact Signing por OIDC. Los dos pasos se saltan con aviso cuando falta el secreto.
- **Empaquetar** `raptor-<versión>-<target>.tar.gz`/`.zip` y generar los `.sha256`, `SHA256SUMS` y los SBOM CycloneDX de `apps/cli` y `apps/mcp`.
- **Atestiguar** la procedencia solo con la variable `RELEASE_ATTEST == 'true'`.
- **Crear** el borrador de GitHub Release, con prerelease si la versión lleva `-`. Nunca se reemplazan los assets de una release ya publicada.
- **Fuera de alcance**: publicar en los canales y los scripts de instalación (INF-GRP-004); `cargo-deny` de licencias (SEC-07, NFR-11); las releases inmutables y los secretos (humano).

### Requiere secreto o configuración (preparado, sin ejecutar)

| Paso | Requiere |
|---|---|
| Firma y notarización en macOS | Secretos `APPLE_CERTIFICATE_P12_BASE64`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_API_KEY_P8_BASE64`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER_ID` |
| Firma en Windows | Secretos `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_CERTIFICATE_PROFILE`, y la credencial federada en Azure para este repo |
| Attestation de procedencia | Variable de repo `RELEASE_ATTEST = true` (entrada pública e irreversible en Sigstore) |
| Releases inmutables | Ajuste del repo, antes de la primera release real |

### Plan de Verificación

#### Pruebas Automatizadas

- **Build**: los 6 jobs `build` en verde, y cada uno sube su archivo.
- **Versión**: un tag distinto de la versión del workspace hace fallar `plan`.
- **Integridad**: `sha256sum --check SHA256SUMS` en verde dentro del job `assemble`.
- **Instaladores**: los jobs `test-installers` de INF-GRP-004 en verde antes de crear el borrador.
- **Borrador**: `gh release view` muestra `isDraft: true` y los assets esperados.

#### Verificación Manual / Sandbox

- Un tag de prueba sobre la rama, que se borra después junto con el borrador. La evidencia está en "Evidencia".

### Evidencia

Se completa en el PR de INF-GRP-003 con el enlace al run.

### Pendientes

- Firma en Windows ARM con Artifact Signing, sin verificar. Pendiente: etapa de validación multiplataforma.
- El banco INF-GRP-002 debe medir NFR-04 y NFR-05 sobre el binario musl (ADR-GRP-014 § 2).
- La firma del SBOM con `attest-sbom` se activa junto con `RELEASE_ATTEST`.
