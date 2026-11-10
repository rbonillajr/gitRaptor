---
id: SPIKE-GRD-002-GUION
title: "Guion manual de SPIKE-GRD-002 (macOS): pasos físicos para Rene"
type: research
status: ready
feature: guardrails
domain: GRP
spike: SPIKE-GRD-002
created: 2026-11-10
updated: 2026-11-10
related:
  adrs: [ADR-GRD-008, ADR-GRP-017]
  stories: [US-GRD-013, US-GRD-015, US-DSK-001, US-DSK-002, US-DSK-006]
  spikes: [SPIKE-GRD-002]
tags: [guardrails, spike, guion-manual, touch-id, localauthentication, presencia-humana]
---

# Guion manual de SPIKE-GRD-002 (macOS)

Pasos que necesitan una persona: poner el dedo, escribir la contraseña, aprobar o cancelar un diálogo. Todo lo demás (plazo de 60 s, cancelación, concurrencia, empaquetado, binario dentro del bundle y entrada sintética) lo ejecuta el worker solo, en la **sesión desatendida**, antes de este guion.

- **Duración**: unos 12 minutos para los pasos 1 a 11. Los opcionales no cuentan.
- **Qué registra**: cada paso añade una línea JSON a `spikes/os-presence-factor/results/manual.jsonl` con el resultado del diálogo (sí o no y un código de error) y tus respuestas a una o dos preguntas. **Nunca registra tu contraseña ni tu huella**: el SO solo devuelve sí o no.
- **No toca GitRaptor** ni tu perfil: es un binario aislado (`presence-probe`) con repos y sockets en un directorio temporal.

## Antes de empezar (30 s)

1. Abre una terminal en el worktree del spike:

   ```sh
   cd ~/orca/workspaces/gitRaptor/osfactor/spikes/os-presence-factor
   ```

2. Ten el Mac **desbloqueado**, con la tapa abierta (Touch ID disponible) y sin otras ventanas importantes en primer plano.

Cada paso es **un comando**. Al final te hace una o dos preguntas; responde con texto breve y Enter.

## Pasos

| # | Comando | Qué haces | Qué observas (y respondes) |
|---|---|---|---|
| 1 | `./scripts/manual-step.sh 1` | Aparece el diálogo del sistema: apruébalo con **Touch ID** | El **nombre de app** que muestra el diálogo y si se lee el texto completo con el código `T1T1` |
| 2 | `./scripts/manual-step.sh 2` | Pulsa **«Usar contraseña…»** y aprueba con **tu contraseña** (va al diálogo del SO, el probe no la ve) | Si pudiste aprobar con la contraseña |
| 3 | `./scripts/manual-step.sh 3` | Pulsa **Cancelar** | Nada (el resultado lo registra el probe) |
| 4 | `./scripts/manual-step.sh 4` | Pon un dedo **no registrado** (o el nudillo) hasta que el diálogo cambie; después **cancela** | Qué hizo el diálogo tras los intentos fallidos |
| 5 | `./scripts/manual-step.sh 5` | Salen **dos diálogos seguidos**: aprueba cada uno con Touch ID | Si tuviste que poner el dedo **dos veces** (uso único, H6) |
| 6 | `./scripts/manual-step.sh 6` | Bloquea el Mac (**Ctrl+Cmd+Q**) y desbloquéalo con **Touch ID**; el diálogo sale solo en cuanto vuelves. Apruébalo con Touch ID | Si el diálogo te pidió el dedo otra vez o aprobó solo (H6 tras un desbloqueo reciente) |
| 7 | `./scripts/manual-step.sh 7` | El daemon de prueba corre como **LaunchAgent**. Aprueba con Touch ID | El nombre de app que muestra el diálogo |
| 8 | `./scripts/manual-step.sh 8` | El daemon arranca **bajo demanda** (entorno vaciado + `setsid`, como el producto). Aprueba con Touch ID | Nada |
| 9 | `./scripts/manual-step.sh 9` | Un binario sin interfaz **dentro de una `.app` de prueba** pide el factor. Aprueba con Touch ID | El nombre y el icono que muestra el diálogo (E15) |
| 10 | `./scripts/manual-step.sh 10` | **Robo de foco.** Sale un prompt: escribe **sin mirar** la frase `hola mundo` y pulsa Enter. A mitad de la frase aparece el diálogo. **No lo apruebes**: si te lo encuentras, cancélalo | Qué pasó con lo que escribías (¿fue al diálogo o a la terminal?) |
| 11 | `./scripts/manual-step.sh 11` | Se activa el **salvapantallas** y, 3 s después, el probe pide el diálogo. No toques nada durante 10 s; luego vuelve (si pide desbloqueo, desbloquea) | Si al volver había un diálogo esperando |

## Opcionales (solo si tienes el medio a mano)

| Paso | Requisito | Comando | Qué observas |
|---|---|---|---|
| opt-watch | Apple Watch emparejado y desbloqueado | `./scripts/manual-step.sh opt-watch` | Si aprueba con doble clic del botón lateral |
| opt-lid | Monitor externo; **tapa cerrada** | `./scripts/manual-step.sh opt-lid` | Qué ofrece el diálogo (Touch ID, contraseña o reloj) |
| opt-ssh | Remote Login activado (Ajustes > General > Compartir). Desde **otra** máquina o desde una terminal: `ssh <tu-usuario>@localhost`, y dentro de la sesión SSH, `cd` al worktree | `./scripts/manual-step.sh opt-ssh` | Si aparece un diálogo en la pantalla del Mac (lo esperado: no, H3). **Desactiva Remote Login al terminar** |

Lo que no se haga queda como **"no verificado"** en el informe, con su motivo (R7): no se inventa ningún resultado.

## Fuera de este guion (a propósito)

- **E11, el secreto conocido con una cuenta de prueba local** (H10): exige crear una cuenta de usuario y cambiar de sesión, más de 15 minutos. Se responde con la evidencia de la sesión desatendida: si las pulsaciones sintéticas llegan al campo de contraseña con un valor **erróneo** fijo, un valor correcto también llegaría. Si el informe lo deja inferido, una sesión aparte con la cuenta de prueba lo cierra.
- **E5, cambio rápido de usuario y Mac sin Touch ID**: necesitan otra cuenta u otro equipo. Quedan "no verificado".
- **E6, primera llamada tras iniciar sesión** (`-1000`): exige cerrar sesión. Queda para una sesión aparte si el informe lo pide.

## Al terminar

Avisa al coordinador con **"guion hecho"** y la lista de pasos saltados. El worker lee `results/manual.jsonl`; no hace falta copiar nada.
