---
id: SPIKE-GRD-002-RES
title: "Resultados de SPIKE-GRD-002: factor de autenticación del SO invocado desde el daemon (macOS)"
type: research
status: draft
feature: guardrails
domain: GRP
spike: SPIKE-GRD-002
created: 2026-11-10
updated: 2026-11-10
related:
  adrs: [ADR-GRD-008, ADR-GRP-002, ADR-GRP-014, ADR-GRP-017, ADR-GRD-004]
  stories: [US-GRD-013, US-GRD-015, US-DSK-001, US-DSK-002, US-DSK-006]
  spikes: [SPIKE-GRD-002, SPIKE-DSK-001, SPIKE-DSK-002]
tags: [guardrails, spike, localauthentication, touch-id, presencia-humana, entrada-sintetica, macsys, objc2-local-authentication, fail-closed]
---

# Resultados de SPIKE-GRD-002 (macOS)

> **Borrador (2026-11-10)**: contiene lo medido sin diálogo (pantalla bloqueada) y el análisis del binding. Falta la sesión desatendida con diálogo y el guion manual de Rene.

## 0. Entorno

- macOS **26.6.2** (25G83), arm64, Touch ID activo (`bioutil`: biometría para desbloquear 1). Una identidad "Apple Development" local; sin Developer ID.
- Prototipo: [`spikes/os-presence-factor/`](../../../../../spikes/os-presence-factor/) (`presence-probe`), workspace propio. `objc2 =0.6.5`, `block2 =0.6.2`, `objc2-foundation =0.3.2`, `objc2-local-authentication =0.3.2`, `objc2-core-graphics =0.3.2`.
- Evidencia: `results/*.jsonl` (una línea JSON por celda; el campo `cell` la identifica).

## 1. Medido con la pantalla bloqueada (2026-11-10)

`results/launch-matrix-locked.jsonl`, con `scripts/launch-matrix.sh`:

| Forma de arranque | audit session | `launchctl managername` | gráfico | `sid` | Accesibilidad heredada | `canEvaluatePolicy` | Resultado | ms |
|---|---|---|---|---|---|---|---|---|
| Directo, hijo de Claude Code (terminal de Orca) | 100024 | Aqua | sí | del shell | **sí** | sí | `-4` systemCancel | 46 |
| Bajo demanda (`env_clear()` + `setsid()`), lanzado desde Orca | 100024 | Aqua | sí | propio | **sí** | sí | `-4` | 32 |
| LaunchAgent sin `LimitLoadToSessionType` | 100024 | Aqua | sí | 1 | no | sí | `-4` | 27 |
| LaunchAgent con `LimitLoadToSessionType=Aqua` | 100024 | Aqua | sí | 1 | no | sí | `-4` | 26 |
| Terminal.app, directo | 100024 | Aqua | sí | propio | no | sí | `-4` | 26 |
| Terminal.app, bajo demanda | 100024 | Aqua | sí | propio | no | sí | `-4` | 44 |

Lecturas:
- **`setsid()` conserva el audit session y el bootstrap Aqua** (premisa de H2): son atributos de Mach/BSM, no de la sesión POSIX.
- **Pantalla bloqueada**: en macOS 26 el fallo es `-4` (`systemCancel`) en < 50 ms y sin diálogo, no el `-1004` de F6. `canEvaluatePolicy` dice "sí" en todos los casos (H4 confirmada). El daemon decide la disponibilidad con `CGSessionCopyCurrentDictionary` (`CGSSessionScreenIsLocked`) más el resultado real.
- **TCC atribuye el permiso de Accesibilidad al proceso responsable**: el daemon lanzado desde Orca lo hereda aunque vacíe el entorno y llame a `setsid()`. Un agente hijo de Claude Code en una terminal con Accesibilidad puede publicar eventos sintéticos (E14 es realista).
- `CGSSessionSecureInputPID` existe en el diccionario de la sesión: permite ver qué proceso tiene la entrada segura activa.

## 2. Binding (E12)

| | `objc2-local-authentication` (en `macsys`) | `robius-authentication` 0.3.1 |
|---|---|---|
| Crates en darwin | **6** (`objc2`, `objc2-encode`, `block2`, `objc2-foundation`, `objc2-local-authentication`, `bitflags`) | 8 (los mismos 6 más `robius-authentication` y `cfg-if`) |
| Crates en todos los targets (lo que entra en `Cargo.lock`) | los mismos | **162** (`zbus`, polkit, `windows`) |
| `invalidate()` | sí | **no** (comentario en su código: "we never invalidate the context") |
| Reutilización | fijada a 0 explícitamente | no la toca (vale 0 por defecto) |
| `unsafe` propio | sí, en `ffi_local_auth` | no |

- `LAContext` **no** es `Send` ni `Sync` en el binding: el contexto vive en el hilo que lo crea; solo cruza al otro hilo un `Sender` de un canal.
- **Sin firmar no arranca** en arm64: `codesign --remove-signature` y el proceso muere con SIGKILL (exit 137) antes de `main`. El mínimo es la firma ad hoc del enlazador (`flags=adhoc,linker-signed`).

## Pendiente

Sesión desatendida (E7, E8, E9, E14/E10, E15) y guion manual (E1, H6, E2/E3 con aprobación, E13, E5 salvapantallas).
