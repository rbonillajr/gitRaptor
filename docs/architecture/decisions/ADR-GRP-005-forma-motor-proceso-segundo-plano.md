---
id: ADR-GRP-005
title: Forma del motor — proceso en segundo plano por usuario y canal local
type: adr
status: proposed
date: 2026-10-03
created: 2026-10-03
updated: 2026-10-03
deciders: [Rene Bonilla]
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-006, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRP-012, ADR-GRP-013, CTX-GRP-001, BR-GRP-001]
tags: [motor-local, daemon, ipc, json-rpc, autoarranque, unix-socket, named-pipe, seguridad, continuidad]
---

# ADR-GRP-005 — Forma del motor: proceso en segundo plano por usuario y canal local

## Contexto

El motor tiene que capturar la actividad de los agentes **aunque no haya ninguna superficie de GitRaptor abierta** (Q1, BR-CONS-005) y sin huecos mientras la máquina está encendida (KPI § 3 del context). Lo consumen tres clientes: la CLI/TUI `raptor` (`apps/cli`), el servidor MCP `raptor-mcp` (`apps/mcp`) y, más adelante, el Cockpit (F-001-02). Restricciones que acotan la forma:

- El motor no escribe nada en el repo observado (Q21, BR-CONS-001) y fuera del repo solo escribe su perfil (Q17); la ubicación del perfil la fija ADR-GRP-006.
- Sin hooks propios (Q22), 100% local y sin puertos de red (NFR-03).
- Solo el desarrollador añade o retira repos y corrige atribuciones; un agente no (Q40, BR-AUTH-001, BR-CONS-002).
- Seguridad del MCP (NFR-02): sin shell, entradas validadas y allowlist.
- Stack fijado por ADR-GRP-001 y ADR-GRP-002: Rust, motor en `crates/core`, contrato en `crates/api`, binarios `raptor` y `raptor-mcp`.

## Decisión

**Un proceso del motor por usuario del SO, en segundo plano, que arranca al iniciar sesión y, como red de seguridad, bajo demanda desde cualquier cliente. Los clientes hablan con él por un canal local accesible solo por ese usuario.**

### 1. Proceso y empaquetado

- El daemon es el subcomando `raptor daemon` del binario `raptor` (`apps/cli`). No hay una app `raptord` separada y **no se enmienda ADR-GRP-002** (decisión de Rene Bonilla, 2026-10-03, PQ-5).
- La lógica del motor vive en `crates/core`, la lectura de Git en `crates/git` (ADR-GRP-009) y el contrato del canal en `crates/api`. `apps/cli` solo aporta el punto de entrada del daemon y los clientes.
- `raptor-mcp` y la CLI/TUI son **clientes** del daemon: ninguno embebe el motor ni abre el almacén del perfil. El daemon es el **único escritor** del perfil (ADR-GRP-006).
- Corre con los privilegios del usuario, nunca como root ni LocalSystem.

### 2. Instancia única

- Un archivo de bloqueo exclusivo en el directorio de estado del perfil (bloqueo consultivo del SO, que se libera solo si el proceso muere).
- Un segundo `raptor daemon` que no obtiene el bloqueo termina sin observar nada y sin tocar el almacén.

### 3. Arranque

- **Autoarranque al iniciar sesión** como **excepción explícita y acotada a Q17** (decisión de Rene Bonilla, 2026-10-03, PQ-1). Lo registra **el instalador o un comando que lanza el desarrollador** (nombre provisional `raptor daemon enable`, con su inverso `raptor daemon disable`), y es reversible al desinstalar. El motor nunca lo registra por su cuenta.

  | SO | Mecanismo | Qué se escribe fuera del perfil |
  |----|-----------|---------------------------------|
  | macOS | LaunchAgent de launchd con arranque al cargar y relanzamiento si termina con error | Un `.plist` en `~/Library/LaunchAgents` |
  | Linux | Unidad `systemd --user` habilitada en el target de sesión, con reinicio ante fallo | Un `.service` en `~/.config/systemd/user` y su enlace de habilitación |
  | Windows | Valor en la clave `Run` de HKCU (la tarea programada de inicio de sesión queda como alternativa si hace falta relanzar ante fallo) | Un valor en HKCU |

- **Arranque bajo demanda**: si un cliente no puede conectar, lanza `raptor daemon` desacoplado (el mismo ejecutable que lo invoca), espera el handshake con un tiempo máximo y reintenta. Si dos clientes lo lanzan a la vez, el bloqueo de instancia única resuelve la carrera.
- **Sin autoarranque** (el desarrollador no lo activó, o falta systemd en Linux): el arranque bajo demanda sigue funcionando y el tiempo desde el inicio de sesión hasta el primer cliente se trata como hueco "sin atribuir" (BR-EDGE-005, ADR-GRP-013). Los clientes avisan de que el autoarranque está desactivado, sin activarlo.
- **Entorno heredado**: launchd y systemd arrancan con un PATH mínimo. El daemon no depende del PATH de la shell para encontrar Git; la resolución la define ADR-GRP-009.

### 4. Ciclo de vida

- Al arrancar: adquiere el bloqueo, abre el perfil, resuelve Git y entra en el estado que corresponda de BR-WF-002 ("Esperando Git", "Sin repos" u "Observando"). Si hay repos, reconcilia cada uno (ADR-GRP-010) y registra el hueco desde la última marca de observación (ADR-GRP-013).
- Parada ordenada (cierre de sesión, `raptor daemon stop`, señal de terminación): vacía lo pendiente, persiste la marca "observado hasta" y libera el bloqueo.
- **Actualización del binario**: si un cliente encuentra un daemon con una versión de protocolo incompatible, le pide parar y lanza el nuevo. En Windows el instalador para el daemon antes de reemplazar el ejecutable.
- Logs en el directorio de estado del perfil, con rotación y sin contenido de archivos del usuario.

### 5. Canal local

- **macOS y Linux**: socket Unix dentro del directorio de ejecución del perfil (ADR-GRP-006), directorio 0700 y socket 0600. El daemon comprueba con las credenciales del par (peer credentials) que el cliente es el mismo usuario y rechaza cualquier otro.
- **Windows**: named pipe con nombre derivado del SID del usuario, DACL limitada a ese SID, creación como primera instancia (evita que otro proceso ocupe el nombre antes) y rechazo de clientes remotos. El cliente comprueba que el servidor del pipe corre con su mismo SID.
- **Sin puertos TCP** ni ninguna escucha de red (NFR-03).
- **Contrato**: JSON-RPC 2.0 con mensajes delimitados y tamaño máximo por mensaje, definido en `crates/api`. Tiene tres partes:
  - **Handshake** con versión de protocolo y versión del binario.
  - **Consultas y comandos** (estado del motor, repos, worktrees, sesiones, eventos, registro y corrección).
  - **Stream de eventos por suscripción**, que publica los eventos del motor en orden de secuencia (ADR-GRP-011 mide su latencia).
- **Validación de entradas**: tipos estrictos con rechazo de campos desconocidos en los comandos; rutas canonicalizadas y comprobadas contra los repos observados (BR-VAL-002); ningún parámetro llega a un shell (argv fijo, ADR-GRP-009).

### 6. Comandos reservados al desarrollador

Añadir o retirar repos y corregir atribuciones **se rechazan si el proceso que llama desciende de un agente detectado**, y además exigen **confirmación interactiva en terminal** (decisión de Rene Bonilla, 2026-10-03, PQ-6):

1. **Ascendencia (en el daemon)**: obtiene el PID del cliente del canal (credenciales del socket o del named pipe), recorre sus procesos antecesores y rechaza el comando si alguno es un proceso de agente detectado según ADR-GRP-012. Es de mejor esfuerzo.
2. **Confirmación interactiva (en el cliente)**: la CLI/TUI solo envía el comando si la entrada y la salida estándar son una terminal y el desarrollador confirma de forma explícita. Sin terminal, el comando no se envía.
3. **MCP**: `raptor-mcp` no expone estas operaciones (allowlist de NFR-02). Un agente solo puede registrarse en su worktree (BR-AUTH-001).

## Alternativas consideradas

| Alternativa | Por qué no |
|-------------|------------|
| **Librería embebida en cada cliente** | Hay huecos cuando no hay ningún cliente abierto (incumple Q1 y BR-CONS-005), los watchers se duplican por cliente y varios procesos escriben a la vez en el perfil. |
| **Daemon solo bajo demanda** | Deja un hueco desde el inicio de sesión hasta el primer cliente, justo cuando un agente puede estar trabajando. Se conserva como red de seguridad, no como mecanismo principal. |
| **Servicio del sistema (root o LocalSystem)** | Privilegios excesivos para leer repos del usuario, un proceso compartido entre usuarios y escrituras fuera del perfil del usuario. Se descarta. |
| **App `raptord` separada** | Exige enmendar ADR-GRP-002 y un segundo binario que distribuir por `winget`, `brew`, `npm` y script. Rechazada por PQ-5. |
| **Canal por TCP en localhost** | Abre un puerto accesible por cualquier proceso de la máquina y obliga a autenticar con tokens. Choca con NFR-03. |
| **Distinguir al desarrollador solo por confirmación interactiva** | Un agente con una pseudo-terminal la superaría. Se combina con la ascendencia de procesos (PQ-6). |

## Consecuencias

- ✅ Cumple "0 huecos mientras la máquina está encendida": la observación no depende de que haya un cliente abierto.
- ✅ Un único escritor del perfil: sin escrituras concurrentes desde varios procesos (ADR-GRP-006).
- ✅ Un único binario que distribuir; ADR-GRP-002 no cambia.
- ✅ El canal no es accesible por otros usuarios ni por la red.
- ⚠️ **El autoarranque escribe fuera del perfil**, en contra de la letra de Q17 y de la verificación 2 de BR-CONS-001 ("fuera del repo, lo único que cambia son los datos del motor en el perfil"). **Mitigación**: solo lo escribe el instalador o un comando del desarrollador, nunca el motor por su cuenta, y se revierte al desinstalar. **Pendiente para el PO**: actualizar Q17 (y la verificación de BR-CONS-001 y el NFR "fuera del repo solo cambia el perfil") con esta excepción. No se edita el requerimiento desde este ADR.
- ⚠️ La distinción entre desarrollador y agente es de mejor esfuerzo: un proceso del mismo usuario puede saltársela (por ejemplo, editando el perfil a mano). **Mitigación**: el modelo de amenaza es el error o la iniciativa de un agente, no un proceso malicioso del propio usuario; se documenta así en el contrato. ⚠️ **ASSUMPTION**: la herramienta de shell de Claude Code no tiene terminal interactiva; lo comprueba SPIKE-GRP-001.
- ⚠️ La confirmación interactiva añade un paso para añadir repos o corregir. **Mitigación**: un solo paso de confirmación, que la TUI integra en su propio flujo.
- ⚠️ En macOS, la ruta de un socket Unix tiene un límite de unos 104 bytes y el perfil vive bajo `~/Library/Application Support`. **Mitigación**: el identificador de la carpeta del perfil es corto (ADR-GRP-006) y el cliente conecta con una ruta relativa al directorio de ejecución si la absoluta excede el límite.
- ⚠️ Un proceso residente consume memoria y CPU de forma continua. **Mitigación**: presupuestos de huella medidos en INF-GRP-002 junto con la frescura.
- ⚠️ En Windows, sin supervisor que relance el daemon tras un fallo. **Mitigación**: el arranque bajo demanda lo relanza en el siguiente uso, y el tiempo caído se reconcilia como hueco.

## Validación

Las pruebas usan repos y perfiles temporales (variable de sobreescritura del perfil de ADR-GRP-006), nunca el repo de GitRaptor.

1. **Instancia única**: dos `raptor daemon` simultáneos; uno termina sin observar y el almacén solo recibe escrituras del otro.
2. **Bajo demanda**: con el daemon parado, `raptor` y `raptor-mcp` lo arrancan y completan el handshake; con dos clientes a la vez queda un solo daemon.
3. **Continuidad**: con el daemon autoarrancado y sin clientes, un commit en un worktree temporal aparece después en el historial (US-GRP-004).
4. **Caída**: matar el daemon a la fuerza; al relanzarlo, el intervalo caído queda registrado como hueco y sus cambios "sin atribuir" (US-GRP-005).
5. **Canal restringido**: el socket es 0600 en un directorio 0700; un cliente de otro usuario es rechazado; en Windows, la DACL del pipe solo contiene el SID del usuario y un cliente remoto es rechazado. No hay ningún puerto en escucha.
6. **Comandos reservados**: añadir o retirar un repo y corregir una atribución lanzados como descendientes de un proceso de agente simulado se rechazan; sin terminal, la CLI no los envía; `raptor-mcp` no los ofrece (US-GRP-001, US-GRP-006, US-GRP-010).
7. **Autoarranque**: `raptor daemon enable` y `disable` crean y eliminan exactamente los artefactos de la tabla del apartado 3, y nada más fuera del perfil, en los tres SO.
8. **Repo intacto**: todos los escenarios anteriores pasan por el arnés de INF-GRP-001 (BR-CONS-001).

## Referencias

- **Reglas**: BR-CONS-001, BR-CONS-005, BR-WF-002, BR-AUTH-001, BR-CONS-002, BR-VAL-002, BR-EDGE-005.
- **Historias**: US-GRP-001, US-GRP-002, US-GRP-004, US-GRP-005, US-GRP-014, US-GRP-015 y, por el canal, todas las demás.
- **Decisiones del context**: Q1, Q6, Q17, Q21, Q22, Q28, Q40.
- **Decisiones de producto**: PQ-1, PQ-5 y PQ-6 (Rene Bonilla, 2026-10-03).
- **NFR**: NFR-01, NFR-02, NFR-03, NFR-04, NFR-06.
- **ADRs**: ADR-GRP-001, ADR-GRP-002, ADR-GRP-006 (perfil), ADR-GRP-009 (resolución de Git), ADR-GRP-010 (reconciliación), ADR-GRP-011 (latencia del stream), ADR-GRP-012 (procesos de agente), ADR-GRP-013 (huecos).
- **Enablers**: TS-GRP-003, TS-GRP-004, INF-GRP-001, INF-GRP-002.
