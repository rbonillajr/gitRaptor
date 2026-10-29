---
title: Primeros pasos con GitRaptor (acceso anticipado)
type: guide
status: proposed
created: 2026-10-28
updated: 2026-10-28
domain: GRP
related: [ADR-GRP-014, INF-GRP-004]
---

# Primeros pasos con GitRaptor (acceso anticipado)

> **Alpha, solo macOS, sin firmar.** Los binarios no están firmados ni notarizados. Versión en inglés: [primeros-pasos.en.md](primeros-pasos.en.md).

Esta guía te lleva de cero a tener GitRaptor protegiendo un repo y conectado a Claude Code. Dura unos 10 minutos.

## 1. Instalar

Elige la versión de la [última alpha](https://github.com/rbonillajr/gitRaptor/releases) (`v0.1.0-alpha.N`) y ejecuta:

```sh
curl -fsSL https://github.com/rbonillajr/gitRaptor/releases/download/v0.1.0-alpha.N/install.sh | sh
```

El script descarga el archivo de tu Mac (Apple silicon o Intel), **verifica el SHA256 antes de extraer** e instala `raptor`, `raptor-mcp` y `raptor-hook` en `~/.local/bin`. No toca tu `PATH`: si `~/.local/bin` no está en él, añádelo a tu perfil de shell.

- Para instalar en `/usr/local/bin`: `sh install.sh --system` (usa `sudo` solo para copiar los 3 archivos).
- **Cuarentena de macOS.** Si bajaste el archivo con el navegador, macOS lo marca y Gatekeeper lo bloquearía. Si el script encuentra esa marca, la quita de los 3 binarios y **te lo avisa** (el binario no está notarizado; confías en el SHA256). Si no hay marca, no hace nada.

Comprueba la instalación:

```sh
raptor --version
```

## 2. Observar un repo

```sh
cd ~/ruta/a/tu/repo
raptor repo add
```

Sin argumento usa la carpeta actual; acepta la raíz de cualquiera de sus worktrees. `raptor repo discovered` lista los repos que GitRaptor encontró en tus carpetas de código y que nadie observa todavía.

## 3. Proteger el repo (Guardrails)

```sh
raptor guard install
```

Explica qué instala y **te pide permiso**. Después, el force-push y el borrado de la rama base quedan denegados, incluso con Git a pelo. `raptor guard status` muestra el estado y `raptor guard uninstall` lo quita y deja el repo como estaba.

## 4. Abrir la TUI

```sh
raptor
```

Muestra los agentes y worktrees en vivo, los conflictos previstos y la línea de tiempo. Se cierra con `q`.

## 5. Conectar Claude Code

```sh
raptor mcp install            # registra el servidor MCP en Claude Code (alcance de usuario)
raptor mcp enable             # en la carpeta del repo: permite que los agentes lo usen
raptor mcp list               # repos que los agentes pueden usar
```

Reinicia Claude Code. `raptor mcp enable` es un comando reservado al desarrollador: un agente no puede ejecutarlo por su cuenta.

## 6. Si algo sale mal

GitRaptor guarda una foto antes de cada operación que cambia tu repo. **No intentes arreglarlo a mano antes de mirar esto.**

```sh
raptor undo                   # deshace la última operación del worktree donde estás
raptor timeline               # qué cambió, cuándo y quién (humano o agente)
raptor restore <id>           # devuelve el worktree a un punto de la línea de tiempo
```

El estado justo antes de `undo` o `restore` también se guarda, así que puedes volver atrás. Ojo: `undo` actúa en local; si ya hiciste `push`, el remoto no se toca.

Para reportar un problema, abre un issue con la salida de `raptor --version` y `raptor status`.

## 7. Desinstalar

```sh
raptor guard uninstall        # en cada repo protegido
raptor mcp uninstall          # quita el servidor MCP de Claude Code
raptor daemon disable         # si activaste el arranque con el login
raptor daemon stop            # para el motor
sh install.sh --uninstall     # borra solo raptor, raptor-mcp y raptor-hook
```

`install.sh --uninstall` **no borra tus datos** (en macOS, `~/Library/Application Support/gitraptor`: perfil y línea de tiempo). Bórralos solo si aceptas perder el historial; si no, una reinstalación los recupera. Si vas a desinstalar de una carpeta que no es la de por defecto, añade `--install-dir <ruta>`.
