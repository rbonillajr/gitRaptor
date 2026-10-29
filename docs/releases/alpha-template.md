---
title: Plantilla de las notas de una alpha semanal
type: template
status: proposed
created: 2026-10-28
updated: 2026-10-28
domain: GRP
related: [ADR-GRP-014, INF-GRP-003, INF-GRP-004]
---

# Plantilla de las notas de una alpha

Cómo se usa: el coordinador copia el bloque de abajo a `docs/releases/v0.1.0-alpha.N.md` (en el mismo PR `chore(release)` que sube la versión), sustituye lo que está entre `<…>` y borra las líneas que no apliquen. `release.yml` usa ese archivo como el texto del borrador de la GitHub Release. El archivo **va en español primero y en inglés después**, y el aviso de "sin firmar" no se quita nunca.

Antes de publicar, comprueba el [criterio de cada alpha](../requirements/release-plan.md#ea-acceso-anticipado-alpha-semanal): CI en verde, pruebas de humo, ninguna regresión de NFR-01 abierta de severidad alta.

---

```markdown
# GitRaptor v0.1.0-alpha.<N> — <AAAA-MM-DD>

> **Acceso anticipado (alpha), solo macOS (Apple silicon e Intel).** Estos binarios **no están firmados ni notarizados**. El script de instalación verifica el SHA256 y, si macOS marcó los archivos con cuarentena, la quita y te lo avisa. Windows y Linux no se ofrecen todavía.

## Instalar

    curl -fsSL https://github.com/rbonillajr/gitRaptor/releases/download/v0.1.0-alpha.<N>/install.sh | sh

Instala `raptor`, `raptor-mcp` y `raptor-hook` en `~/.local/bin` (con `--system`, en `/usr/local/bin`). Primeros pasos: [`docs/guia/primeros-pasos.md`](https://github.com/rbonillajr/gitRaptor/blob/v0.1.0-alpha.<N>/docs/guia/primeros-pasos.md).
Verificación manual: descarga el `.tar.gz` de tu Mac y `SHA256SUMS`, y comprueba con `shasum -a 256 -c`.
Desinstalar: `sh install.sh --uninstall` (no toca tus datos).

## Qué cambia

- <cambio visible para el usuario, con el id de la historia: US-XXX-NNN>
- …

## Limitaciones conocidas

- Sin firma ni notarización (ver arriba).
- <limitación nueva o vigente: por ejemplo "`undo` deshace en local; el remoto no se toca (US-TMC-014 aún no avisa)">
- El riesgo 12 de NFR-01 afecta solo a Windows, que no se ofrece.
- <regresión de NFR-01 abierta de severidad media o baja, si la hay, con su id>

## Cómo reportar un problema

Abre un issue en el repo con la salida de `raptor --version` y `raptor status`, qué hacías y qué esperabas. Si perdiste trabajo, **no insistas**: ejecuta `raptor timeline` y `raptor restore <id>` antes de seguir.

---

# GitRaptor v0.1.0-alpha.<N> — <YYYY-MM-DD>

> **Early access (alpha), macOS only (Apple silicon and Intel).** These binaries are **not signed nor notarized**. The install script verifies the SHA256 and, if macOS flagged the files with the quarantine attribute, removes it and tells you. Windows and Linux are not offered yet.

## Install

    curl -fsSL https://github.com/rbonillajr/gitRaptor/releases/download/v0.1.0-alpha.<N>/install.sh | sh

Installs `raptor`, `raptor-mcp` and `raptor-hook` into `~/.local/bin` (`/usr/local/bin` with `--system`). Getting started: [`docs/guia/primeros-pasos.en.md`](https://github.com/rbonillajr/gitRaptor/blob/v0.1.0-alpha.<N>/docs/guia/primeros-pasos.en.md).
Manual check: download the `.tar.gz` for your Mac and `SHA256SUMS`, then run `shasum -a 256 -c`.
Uninstall: `sh install.sh --uninstall` (it does not touch your data).

## What changed

- <user-visible change, with the story id: US-XXX-NNN>
- …

## Known limitations

- Not signed nor notarized (see above).
- <new or standing limitation, for example "`undo` rolls back locally; the remote is not touched (US-TMC-014 does not warn yet)">
- Risk 12 of NFR-01 affects Windows only, which is not offered.
- <open NFR-01 regression of medium or low severity, if any, with its id>

## Reporting a problem

Open an issue in the repo with the output of `raptor --version` and `raptor status`, what you were doing and what you expected. If you lost work, **do not keep going**: run `raptor timeline` and `raptor restore <id>` first.
```
