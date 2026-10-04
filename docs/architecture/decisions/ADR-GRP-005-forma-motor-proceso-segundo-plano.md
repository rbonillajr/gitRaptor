---
id: ADR-GRP-005
title: Forma del motor — proceso en segundo plano por usuario y canal local
type: adr
status: accepted
accepted: 2026-10-04
date: 2026-10-03
created: 2026-10-03
updated: 2026-10-04
deciders: [Rene Bonilla]
domain: GRP
feature: motor-local
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-006, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRP-012, ADR-GRP-013, ADR-GRD-003, ADR-GRD-004, ADR-GRD-006, ADR-GRD-007, ADR-CKP-001, ADR-CKP-002, ADR-CKP-003, CTX-GRP-001, BR-GRP-001]
tags: [motor-local, daemon, ipc, json-rpc, autoarranque, unix-socket, named-pipe, seguridad, continuidad, comandos-reservados, prompt-injection]
---

# ADR-GRP-005 — Forma del motor: proceso en segundo plano por usuario y canal local

> **Estado**: aceptado por Rene Bonilla el 2026-10-04.

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
- `raptor-mcp` y la CLI/TUI son **clientes** del daemon: ninguno embebe el motor ni abre el almacén del perfil. El daemon es el **único escritor** del perfil (ADR-GRP-006). (Enmienda 2026-10-04, Cockpit: dentro del binario `raptor`, la frontera es de módulo; ver la sección final.)
- **Excepción acotada (Enmienda 2026-10-04, Guardrails; ADR-GRD-003 § 4, ADR-GRD-006 § 5)**: el cliente del hook de Guardrails (`raptor hook`), cuando el daemon no es alcanzable o es de otra instancia, hace dos accesos al **directorio de estado** del perfil, cuya ruta es una constante de su dispatcher:
  - **Escribe** una entrada en el **spool** append-only del modo degradado: un archivo por entrada, creado en exclusiva y sin seguir enlaces, 0600 en una carpeta 0700, con tamaño y número de archivos acotados. El daemon lo ingiere con `origin = spool-unverified` y lo borra.
  - **Lee** la **instantánea** de solo lectura que el daemon deja para el modo degradado (última rama base confirmada).

  Nada más: el cliente del hook no abre el almacén SQLite ni los archivos de configuración del perfil, y el daemon sigue siendo el único escritor del **almacén**, del índice global y de la instantánea.
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
  | Windows | Valor en la clave `Run` de HKCU. La tarea programada de inicio de sesión es solo la alternativa, si hace falta relanzar ante fallo | Un valor en HKCU |

- **Endurecimiento del autoarranque (SEC-14)**: el artefacto apunta a la ruta absoluta del binario, entre comillas en HKCU `Run`. `daemon enable` se niega si el binario vive en la caché de npx o en una carpeta temporal. `enable` y `disable` no se pueden invocar desde el MCP, y `disable` elimina exactamente lo que creó `enable`.

- **Arranque bajo demanda**: si un cliente no puede conectar, arranca el daemon y espera el handshake con un tiempo máximo y reintenta. Si dos clientes lo lanzan a la vez, el bloqueo de instancia única resuelve la carrera. **El daemon no hereda el entorno del cliente** (SEC-10), que puede ser `raptor-mcp` lanzado por un agente: si el autoarranque está registrado en macOS o Linux, el cliente lo pide al gestor de servicios (`launchctl kickstart`, `systemctl --user start`). En Windows, HKCU `Run` no es un gestor al que se le pueda pedir el arranque, así que el cliente **siempre** lanza el ejecutable con entorno limpio (la tarea programada solo entra si se adopta la alternativa del apartado 3). Sin autoarranque registrado, en cualquier SO, lanza el ejecutable instalado con un **entorno limpio** construido por allowlist (sin `GIT_*`, `LD_PRELOAD`, `DYLD_*`, `XDG_CONFIG_HOME` ni PATH del cliente) y cwd fijo en el perfil.
- **Sin autoarranque** (el desarrollador no lo activó, o falta systemd en Linux): el arranque bajo demanda sigue funcionando y el tiempo desde el inicio de sesión hasta el primer cliente se trata como hueco "sin atribuir" (BR-EDGE-005, ADR-GRP-013). Los clientes avisan de que el autoarranque está desactivado, sin activarlo.
- **Entorno heredado**: launchd y systemd arrancan con un PATH mínimo. El daemon no depende del PATH de la shell para encontrar Git; la resolución y el entorno de los procesos hijo (por allowlist) los define ADR-GRP-009.

### 4. Ciclo de vida

- Al arrancar: adquiere el bloqueo, abre el perfil, resuelve Git y entra en el estado que corresponda de BR-WF-002 ("Esperando Git", "Sin repos" u "Observando"). Si hay repos, reconcilia cada uno (ADR-GRP-010) y registra el hueco desde la última marca de observación (ADR-GRP-013).
- Parada ordenada (cierre de sesión, `raptor daemon stop`, señal de terminación): vacía lo pendiente, persiste la marca "observado hasta" y libera el bloqueo. **`raptor daemon stop` por el canal es un comando reservado** (apartado 6, SEC-13): un agente no puede parar el motor para dejar su trabajo sin atribuir. La parada registra su causa y el cliente que la pidió (ADR-GRP-013).
- **Actualización del binario**: si un cliente encuentra un daemon con una versión de protocolo incompatible, le pide parar y lanza el nuevo. Esa petición **solo se acepta si el ejecutable del cliente es el binario instalado** (misma ruta canónica que el autoarranque o que el propio daemon); si no, se trata como cualquier otra parada reservada. En Windows el instalador para el daemon antes de reemplazar el ejecutable.
- Logs en el directorio de estado del perfil, con rotación y sin contenido de archivos del usuario, valores de config, entorno ni argv de terceros (SEC-05); el panic hook redacta igual.

### 5. Canal local

- **macOS y Linux**: socket Unix dentro del directorio de ejecución del perfil (ADR-GRP-006), directorio 0700 y socket 0600, **creados con umask 077 antes de `bind`** (sin ventana `bind`→`chmod`). Si el directorio ya existe, el daemon verifica propietario (uid) y modo y **aborta** si no cuadran, sin "arreglarlo" con chmod (SEC-01). El daemon comprueba con las credenciales del par (peer credentials) que el cliente es el mismo usuario y rechaza cualquier otro.
- **Windows**: named pipe con nombre derivado del SID del usuario, DACL limitada a ese SID, creación como primera instancia (`FILE_FLAG_FIRST_PIPE_INSTANCE`, evita que otro proceso ocupe el nombre antes) y rechazo de clientes remotos (`PIPE_REJECT_REMOTE_CLIENTS`). El cliente comprueba que el servidor del pipe corre con su mismo SID y conecta con `SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION`, para que un servidor impostor no pueda suplantarlo.
- **Sin puertos TCP** ni ninguna escucha de red (NFR-03).
- **Contrato**: JSON-RPC 2.0 con mensajes delimitados y tamaño y profundidad máximos por mensaje, batches desactivados o acotados y timeout de handshake, definido en `crates/api`. Tiene tres partes:
  - **Handshake** con versión de protocolo y versión del binario. El daemon presenta además el **id de instancia del perfil** (ADR-GRP-006 § 4), que el cliente del hook de Guardrails compara con la constante de su dispatcher (Enmienda 2026-10-04; ADR-GRD-003 § 4).
  - **Consultas y comandos** (estado del motor, repos, worktrees, sesiones, eventos, registro y corrección).
  - **Stream de eventos por suscripción**, que publica los eventos del motor en orden de secuencia (ADR-GRP-011 mide su latencia).
- **Validación de entradas (SEC-02)**: tipos estrictos con rechazo de campos desconocidos (`deny_unknown_fields`) en todos los mensajes. Rutas: absolutas; se rechazan UNC, `\\?\`, nombres de dispositivo y ADS **antes de tocar el sistema de archivos** (canonicalizar una ruta UNC abriría una conexión SMB, M9); después se canonicalizan y se comprueba que pertenecen a un worktree observado (BR-VAL-002). Refs: reglas de `check-ref-format` y siempre tras `--`. Ningún parámetro llega a un shell (argv fijo, ADR-GRP-009).
- **Robustez frente a clientes (SEC-08)**: cola acotada por suscriptor; un cliente lento se desconecta con un evento "resync" y nunca bloquea al productor. Límites de conexiones y suscripciones por cliente y rate limit de consultas. Las consultas se sirven del estado en memoria, sin lanzar `git` por petición. (Enmienda 2026-10-04, Cockpit: excepción acotada para las consultas bajo demanda de grafo y de diff; ver la sección final.)
- **Contrato de salida (SEC-12)**: `crates/api` marca como no confiable todo texto procedente del repo o de un agente (rutas, ramas, nombres de agente declarados, diagnósticos). Los clientes CLI/TUI lo limpian de caracteres de control y escapes ANSI/OSC (OSC 52, título, hipervínculos) antes de mostrarlo. Las respuestas que el MCP devuelve a un agente son estructuradas, con longitud máxima por campo, sin mensajes de commit ni contenido de archivos y limitadas al repo del llamante (prompt injection indirecta, OWASP LLM01). El detalle de presentación queda pendiente de llevar a ADR-GRP-004 y a la spec del MCP (F-001-05). (Enmienda 2026-10-04, Cockpit: llevado a ADR-GRP-004 para CLI/TUI; la spec del MCP sigue pendiente.)

### 6. Comandos reservados al desarrollador

**Comandos reservados**: añadir o retirar repos, corregir o retirar una corrección de atribución, **retirar el registro de otro agente** y **parar el daemon** (decisión de Rene Bonilla, 2026-10-03, PQ-6; ampliada por la revisión de seguridad, SEC-03 y SEC-13).

**Modelo de amenaza**: un proceso del mismo usuario **no confiable**, en particular un **agente comprometido por prompt injection** (OWASP LLM01, MCP Top 10) que puede ejecutar comandos de shell, hablar JSON-RPC directo con el socket sin pasar por la CLI y abrir una pseudo-terminal. La decisión, por tanto, **se toma solo en el daemon**: el daemon no acepta ninguna marca enviada por el cliente ("confirmado", "tengo TTY") como prueba.

1. **Identificación del llamante (en el daemon)**: el daemon identifica al proceso cliente con un identificador **no reutilizable** obtenido del canal: pidfd en Linux, audit token en macOS, handle del proceso en Windows. Así se evita la carrera de reutilización de PID.
2. **Ascendencia (en el daemon)**: con ese identificador recorre los procesos antecesores y rechaza el comando si alguno es un proceso de agente detectado según ADR-GRP-012.
3. **Terminal de control y sesión (en el daemon)**: el daemon comprueba por su cuenta que el cliente tiene terminal de control y que el **líder de su sesión no desciende de un agente**. Un cliente lanzado con una pty (`script`) bajo el agente se rechaza por la ascendencia del líder de sesión.
4. **Confirmación interactiva (en el cliente)**: la CLI/TUI pide confirmación explícita al desarrollador. Es un paso de UX que evita errores, **no un control de seguridad**: el daemon no confía en él.
5. **MCP**: `raptor-mcp` no expone estas operaciones (allowlist de NFR-02).
6. **Registro de un agente (M7)**: el worktree se toma del **cwd del proceso llamante**, leído por el daemon con el identificador del punto 1, nunca de un parámetro; un registro con un worktree distinto se rechaza (BR-AUTH-001). Los nombres declarados se validan (formato, longitud, sin caracteres de control) y se prohíben los reservados (por ejemplo, "Claude Code" o "GitRaptor") para que un proceso no suplante a un agente detectado.
   - **Retiro de un registro (Q41, BR-WF-001)**. ⚠️ **ASSUMPTION** pendiente de confirmar por Rene: el desarrollador puede retirar cualquier registro (comando reservado, con los controles 1 a 4); un agente solo el suyo, es decir, el registro que él hizo en el worktree que es el cwd del llamante, tomado igual que al registrarse. Cualquier otro retiro pedido por un agente se rechaza y queda en la auditoría. El retiro termina la sesión registrada (ADR-GRP-013).
7. **Auditoría (SEC-03)**: cada comando reservado, aceptado o rechazado, queda en un registro append-only del perfil con fecha, operación, resultado y cliente, visible en los clientes (ADR-GRP-013).

**Comandos reservados de Guardrails (Enmienda 2026-10-04; ADR-GRD-007 § 1)**. La lista se amplía con estos comandos, que usan el mismo mecanismo de los puntos 1 a 7 (nombres provisionales):

| Comando | ¿Relaja? | Refuerzo |
|---|---|---|
| Instalar la protección (`raptor guard install`) | No | Confirmación UX con qué, dónde, por qué, cómo se revierte y los hooks previos (BR-AUTH-002). Sin ventana |
| Registrar la denegación del permiso | No | — |
| Adoptar una instalación huérfana | No | — |
| Rechazar una petición de la cola (futuro, US-GRD-015) | No | — |
| Desinstalar la protección (`raptor guard uninstall`) | **Sí** | **D5** |
| Retirar una instalación huérfana | **Sí** | **D5** |
| Excepción consciente (`raptor guard exec -- git …`) | **Sí** | **D5**, con la ventana antes de emitir el token de un solo uso (ADR-GRD-007 § 3). Por **D10**, también la aprobación explícita en el Cockpit (Q-GRD-1) |
| Confirmar la rama base y el suelo **iniciales** al instalar (US-GRD-001) (**D9**) | No: confirma `main` sin leer el suelo | Sin ventana. Con configuración del equipo, deja `base-unconfirmed` |
| Confirmar la rama base y el suelo **iniciales** con un comando explícito (US-GRD-014) (**D9**) | **Sí**, si el suelo trae relajaciones | **D5** cuando el suelo relaja (p. ej. desactiva el mínimo) |
| Confirmar un cambio del suelo o de la rama base (D7, D8; ADR-GRD-004 § 3 y § 4) | **Sí** | **D5** (por D8), con el diff de lo que se relaja y la ref y el commit de origen a la vista |
| Relajar la configuración con el comando de edición (futuro, US-GRD-013) | **Sí** | **D5** + factor del SO de [ADR-GRD-008](./ADR-GRD-008-factor-autenticacion-fuera-de-banda.md) obligatorio; sin él, fail-closed |
| Aprobar una petición de la cola (futuro, US-GRD-015) | **Sí** | **D5** + factor del SO de [ADR-GRD-008](./ADR-GRD-008-factor-autenticacion-fuera-de-banda.md) obligatorio; sin él, fail-closed |

- **Refuerzo D5** (Rene Bonilla, 2026-10-04) para los comandos que relajan: **anuncio** `reserved-action-pending` en el stream de todos los clientes; **ventana cancelable** antes de aplicar (⚠️ **ASSUMPTION**: 10 s), en la que cualquier cliente del usuario puede cancelar sin comando reservado; y **auditoría con la cadena completa de ascendencia** (ruta del ejecutable e identificador de cada proceso, terminal de control y líder de sesión) más la **aceptación del riesgo por acción** (ADR-GRP-013 § 1).
- **Añadir un repo no confirma la rama base ni el suelo** (**D9**, Rene Bonilla, 2026-10-04): la confirmación inicial es un comando de Guardrails (tabla de arriba). Hasta entonces, Guardrails protege la unión de ramas base en las dos fases de ADR-GRD-004 § 3.5 y el motor marca la rama base como "no confirmada" (ADR-GRP-007).
- El análisis de los vectores que los controles 1 a 3 no detectan (`tmux send-keys`, `osascript`, código plantado) y la aceptación del riesgo por acción están en ADR-GRD-007 § 2.

- **Operaciones del catálogo del Cockpit** (Enmienda 2026-10-04, Cockpit): la confirmación de trabajo ajeno y la excepción consciente desde el Cockpit reutilizan los controles 1 a 3; ver la sección final.

**Riesgo residual aceptado**: un agente que se desacopla de su árbol de procesos (doble fork con `setsid`, `launchctl submit`, `systemd-run --user`, servicios del SO) puede evadir la ascendencia. Se acepta para el MVP porque el motor no tiene privilegios por encima del usuario y la auditoría deja rastro; se revisa si aparece un mecanismo de atestación del SO más fuerte.

## Alternativas consideradas

| Alternativa | Por qué no |
|-------------|------------|
| **Librería embebida en cada cliente** | Hay huecos cuando no hay ningún cliente abierto (incumple Q1 y BR-CONS-005), los watchers se duplican por cliente y varios procesos escriben a la vez en el perfil. |
| **Daemon solo bajo demanda** | Deja un hueco desde el inicio de sesión hasta el primer cliente, justo cuando un agente puede estar trabajando. Se conserva como red de seguridad, no como mecanismo principal. |
| **Servicio del sistema (root o LocalSystem)** | Privilegios excesivos para leer repos del usuario, un proceso compartido entre usuarios y escrituras fuera del perfil del usuario. Se descarta. |
| **App `raptord` separada** | Exige enmendar ADR-GRP-002 y un segundo binario que distribuir por `winget`, `brew`, `npm` y script. Rechazada por PQ-5. |
| **Canal por TCP en localhost** | Abre un puerto accesible por cualquier proceso de la máquina y obliga a autenticar con tokens. Choca con NFR-03. |
| **Distinguir al desarrollador solo por confirmación interactiva** | Un agente con una pseudo-terminal la superaría. Se combina con la ascendencia de procesos (PQ-6). |
| **Verificar la terminal en el cliente** | Un agente puede hablar JSON-RPC directo al socket y declarar lo que quiera. Descartada en la revisión de seguridad (H1): toda comprobación se hace en el daemon. |
| **Ascendencia por PID** | El PID se puede reutilizar entre la consulta y la comprobación. Se sustituye por pidfd, audit token o handle (H1). |

## Consecuencias

- ✅ Cumple "0 huecos mientras la máquina está encendida": la observación no depende de que haya un cliente abierto.
- ✅ Un único escritor del perfil: sin escrituras concurrentes desde varios procesos (ADR-GRP-006). La única excepción, el spool del modo degradado de Guardrails, está acotada a archivos sueltos del directorio de estado y no toca el almacén (apartado 1).
- ⚠️ **Un proceso del mismo usuario puede escribir o borrar entradas del spool.** **Mitigación** (ADR-GRD-006 § 5 y § 6): validación, topes, `origin = spool-unverified` visible y excluido del KPI por defecto; ninguna decisión depende del spool.
- ✅ Un único binario que distribuir; ADR-GRP-002 no cambia.
- ✅ El canal no es accesible por otros usuarios ni por la red.
- ⚠️ **El autoarranque escribe fuera del perfil**, en contra de la letra de Q17 y de la verificación 2 de BR-CONS-001 ("fuera del repo, lo único que cambia son los datos del motor en el perfil"). **Mitigación**: solo lo escribe el instalador o un comando del desarrollador, nunca el motor por su cuenta, y se revierte al desinstalar. **Pendiente para el PO**: actualizar Q17 (y la verificación de BR-CONS-001 y el NFR "fuera del repo solo cambia el perfil") con esta excepción. No se edita el requerimiento desde este ADR.
- ⚠️ La distinción entre desarrollador y agente sigue siendo de mejor esfuerzo frente a un proceso del mismo usuario que se desacopla de su árbol (doble fork, `setsid`) o edita el perfil a mano. **Mitigación**: el modelo de amenaza incluye el agente comprometido por prompt injection; todas las comprobaciones se hacen en el daemon con identificadores no reutilizables, terminal de control y líder de sesión, y cada comando reservado queda auditado. El riesgo residual se acepta y se documenta en el contrato. El supuesto "la herramienta de shell de Claude Code no tiene terminal interactiva" deja de ser crítico (SPIKE-GRP-001 lo sigue midiendo).
- ⚠️ La confirmación interactiva añade un paso para añadir repos o corregir. **Mitigación**: un solo paso de confirmación, que la TUI integra en su propio flujo.
- ⚠️ En macOS, la ruta de un socket Unix tiene un límite de unos 104 bytes y el perfil vive bajo `~/Library/Application Support`. **Mitigación**: el identificador de la carpeta del perfil es corto (ADR-GRP-006) y el cliente conecta con una ruta relativa al directorio de ejecución si la absoluta excede el límite.
- ⚠️ Un proceso residente consume memoria y CPU de forma continua. **Mitigación**: presupuestos de huella medidos en INF-GRP-002 junto con la frescura.
- ⚠️ En Windows, sin supervisor que relance el daemon tras un fallo. **Mitigación**: el arranque bajo demanda lo relanza en el siguiente uso, y el tiempo caído se reconcilia como hueco.

Nota de integración (Time Machine, ADR-TMC-002, ADR-TMC-004 y ADR-TMC-005, aceptados el 2026-10-03): el canal expone además la operación protegida de la Time Machine (intención, snapshot previo y registro) y sus comandos de snapshot, undo, redo, restauración y timeline. La confirmación para deshacer trabajo ajeno reutiliza los controles del daemon de § 6 (identificador no reutilizable, ascendencia, terminal de control y líder de sesión), ampliados con la hora de inicio de cada antecesor, la contaminación por multiplexor y un reto de un solo uso ligado al plan (SEC-TMC-03). Un undo no es un comando reservado: un agente puede deshacer su propio trabajo.

## Validación

Las pruebas usan repos y perfiles temporales (variable de sobreescritura del perfil de ADR-GRP-006, solo en builds de test), nunca el repo de GitRaptor.

1. **Instancia única**: dos `raptor daemon` simultáneos; uno termina sin observar y el almacén solo recibe escrituras del otro.
2. **Bajo demanda**: con el daemon parado, `raptor` y `raptor-mcp` lo arrancan y completan el handshake; con dos clientes a la vez queda un solo daemon.
3. **Continuidad**: con el daemon autoarrancado y sin clientes, un commit en un worktree temporal aparece después en el historial (US-GRP-004).
4. **Caída**: matar el daemon a la fuerza; al relanzarlo, el intervalo caído queda registrado como hueco y sus cambios "sin atribuir" (US-GRP-005).
5. **Canal restringido (SEC-01)**: el socket es 0600 en un directorio 0700; un cliente de otro usuario es rechazado; un directorio pre-creado 0755 o de otro propietario impide arrancar al daemon; en Windows, la DACL del pipe solo contiene el SID del usuario, un pipe ocupado por otro proceso hace que el cliente rechace la conexión y un cliente remoto es rechazado. `lsof -i`/`netstat` no muestran ningún puerto en escucha.
6. **Entradas (SEC-02)**: `cargo-fuzz` del decodificador de `crates/api`; corpus de rutas maliciosas (traversal, symlink hacia fuera, UNC con captura de red: 0 conexiones SMB); una ref `--upload-pack=x` se rechaza.
7. **Comandos reservados (SEC-03)**: añadir o retirar un repo, corregir una atribución y parar el daemon enviados por un cliente JSON-RPC directo (sin la CLI) descendiente de un agente simulado se rechazan; ídem con pty (`script`) bajo el agente; un registro con worktree ajeno se rechaza; un agente que retira el registro de otro agente es rechazado y el que retira el suyo es aceptado; cada intento queda en el registro de auditoría; `raptor-mcp` no los ofrece (US-GRP-001, US-GRP-006, US-GRP-010). La evasión por doble fork/`setsid` se documenta como riesgo aceptado. Los comandos reservados de Guardrails se validan con ADR-GRD-007 (Validación 1 a 5: rechazo desde un agente, exclusión del MCP, anuncio y ventana, auditoría completa y vectores).
8. **Robustez (SEC-08)**: un cliente que no lee y 100 conexiones simultáneas; el p95 de los demás clientes sigue dentro del presupuesto de ADR-GRP-011.
9. **Entorno (SEC-10)**: arranque bajo demanda desde un cliente con `GIT_EXEC_PATH`, `LD_PRELOAD`/`DYLD_INSERT_LIBRARIES`, `PATH=.:…` o `XDG_CONFIG_HOME` hostiles: el daemon no los hereda.
10. **No repudio (SEC-13)**: un agente simulado que ejecuta `raptor daemon stop` es rechazado; `kill -9` con sesión activa deja un hueco "caída durante sesión activa".
11. **Autoarranque (SEC-14)**: `raptor daemon enable` y `disable` crean y eliminan exactamente los artefactos de la tabla del apartado 3, y nada más fuera del perfil, en los tres SO; ruta con espacios en Windows; `enable` desde la caché de npx se rechaza.
12. **Salida (SEC-12)**: una rama o un archivo con `\x1b]52;…` u `\x1b]0;…` sale escapado en CLI/TUI; snapshot de respuestas MCP sin campos fuera de la allowlist.
13. **Repo intacto**: todos los escenarios anteriores pasan por el arnés de INF-GRP-001 (BR-CONS-001).

## Referencias

- **Reglas**: BR-CONS-001, BR-CONS-005, BR-WF-002, BR-AUTH-001, BR-CONS-002, BR-VAL-002, BR-EDGE-005.
- **Historias**: US-GRP-001, US-GRP-002, US-GRP-004, US-GRP-005, US-GRP-014, US-GRP-015 y, por el canal, todas las demás.
- **Decisiones del context**: Q1, Q6, Q17, Q21, Q22, Q28, Q40.
- **Decisiones de producto**: PQ-1, PQ-5 y PQ-6 (Rene Bonilla, 2026-10-03).
- **NFR**: NFR-01, NFR-02, NFR-03, NFR-04, NFR-06.
- **ADRs**: ADR-GRP-001, ADR-GRP-002, ADR-GRP-006 (perfil), ADR-GRP-009 (resolución de Git), ADR-GRP-010 (reconciliación), ADR-GRP-011 (latencia del stream), ADR-GRP-012 (procesos de agente), ADR-GRP-013 (huecos).
- **Enablers**: TS-GRP-003, TS-GRP-004, INF-GRP-001, INF-GRP-002.
- **Seguridad**: `docs/architecture/non-functional.md` (Security NFRs SEC-01 a SEC-14).
- **Guardrails** (Enmienda 2026-10-04): ADR-GRD-003 § 4 (modo degradado, id de instancia), ADR-GRD-004 § 3 (confirmación inicial), ADR-GRD-006 § 5 (spool), ADR-GRD-007 § 1 y § 2 (comandos reservados, D5); decisiones D5, D7, D8, D9 y D10 de Rene Bonilla (2026-10-04).

## Revisión de seguridad (2026-10-03)

Enmienda tras la revisión del security-expert. No cambia la forma del motor; endurece canal, arranque y comandos reservados.

| Hallazgo | Cómo se cubre |
|---|---|
| H1 · Confirmación TTY en el cliente y ascendencia por PID | Apartado 6: decisión solo en el daemon con pidfd/audit token/handle, terminal de control y líder de sesión verificados por el daemon; "parar el daemon" reservado; modelo de amenaza "agente comprometido por prompt injection"; doble fork/`setsid` como riesgo residual aceptado (SEC-03) |
| H4 · Arranque bajo demanda que hereda el entorno del cliente | Apartado 3: arranque vía gestor de servicios o con entorno limpio por allowlist (SEC-10); entorno de los hijos `git` en ADR-GRP-009 |
| M5 · Carrera `bind`→`chmod`, directorios preexistentes, SQOS en Windows | Apartado 5: umask 077 antes de `bind`, verificación de propietario y modo con aborto, `SECURITY_IDENTIFICATION` (SEC-01); permisos del resto del perfil en ADR-GRP-006 (SEC-06) |
| M6 · Un agente para o mata el daemon para no ser atribuido | Apartado 4: parada reservada, petición de "versión incompatible" solo del binario instalado, causa y cliente en el hueco (SEC-13, ADR-GRP-013) |
| M7 · Registro de agente sin mecanismo y nombres suplantables | Apartado 6.6: worktree = cwd del llamante; nombres validados y reservados prohibidos |
| M8 · Escapes de terminal y prompt injection vía respuestas MCP | Apartado 5, contrato de salida de `crates/api` (SEC-12); pendiente de llevar a ADR-GRP-004 y a la spec del MCP |
| M9 · UNC en Windows abre SMB al canonicalizar | Apartado 5: rechazo de UNC, `\\?\`, dispositivos y ADS antes de tocar el FS (SEC-02) |
| L1 · Ruta sin comillas en HKCU Run; `enable` desde npx | Apartado 3: endurecimiento del autoarranque (SEC-14) |

Validación ampliada: SEC-01, SEC-02, SEC-03, SEC-08, SEC-10, SEC-12, SEC-13 y SEC-14 (puntos 5 a 12). Condición para pasar a `accepted`: esos puntos en la Validación (cubierto en texto) e INF-GRP-001 con repo canario y auditoría dinámica de `exec`.

## Enmienda (2026-10-04, Guardrails)

Aplicada desde la tabla de enmiendas de [non-functional-guardrails.md](../non-functional-guardrails.md) (J10). No cambia la forma del motor, el canal ni los controles 1 a 7. El `status` siguió en `proposed` hasta su aceptación (Rene Bonilla, 2026-10-04).

| Cambio | Dónde | Fuente |
|---|---|---|
| Excepción acotada a "el daemon es el único escritor del perfil": el cliente del hook escribe el spool y lee la instantánea del modo degradado en el directorio de estado | § 1; Consecuencias | ADR-GRD-003 § 4, ADR-GRD-006 § 5 |
| El handshake presenta el id de instancia del perfil | § 5 | ADR-GRD-003 § 4; ADR-GRP-006 § 4 |
| Lista ampliada de comandos reservados de Guardrails, con el refuerzo D5 para los que relajan | § 6; Validación 7 | ADR-GRD-007 § 1 y § 2; D5, D7, D8 |
| **Ronda de coherencia (2026-10-04)**: la confirmación inicial sale de "añadir un repo" y pasa a instalar (US-GRD-001, sin relajar y sin ventana) o a un comando explícito (US-GRD-014, con D5 si el suelo relaja) (**D9**); toda excepción, también la del Cockpit, con D5 (**D10**) | § 6 | ADR-GRD-004 § 3, ADR-GRD-007 § 1; D9, D10 |
| **Corrección tras el Judge (2026-10-04)**: la confirmación inicial de US-GRD-001 no relaja y no tiene ventana; D5 solo en la explícita de US-GRD-014; la unión remite a las dos fases de ADR-GRD-004 § 3.5; D9 y D10 en Referencias | § 6; Referencias | ADR-GRD-004 § 3.5, ADR-GRD-007 § 1 |

## Enmienda (2026-10-04, TS-GRP-004)

Sale de la implementación del canal ([Dev Spec de TS-GRP-004](../../requirements/features/motor-local/dev-specs/TS-GRP-004-dev-spec.md)). No cambia la forma del motor ni el modelo de amenaza. Concreta cómo se cumplen § 5 y § 6 en macOS y deja escritas las desviaciones. Es **Decisión del orquestador (2026-10-04), validada por el Arquitecto**. El Arquitecto recomienda que el security-expert revise los puntos 1 y 4 antes de dar la enmienda por cerrada. El `status` sigue en `accepted`.

| # | Cambio | Dónde | Motivo |
|---|---|---|---|
| 1 | **Identificador no reutilizable en macOS: `(pid, hora de inicio)`, no el audit token.** Se comprueba que cuadra con el audit token (pid y euid), que el cliente arrancó antes del `accept`, que cada padre arrancó antes que su hijo y que el cliente sigue siendo el mismo al terminar el recorrido. La comprobación se hace en cada petición | § 6.1 | Comparar el `pidversion` del token con otro proceso exige un flavor de `proc_pidinfo` sin wrapper seguro, y el workspace prohíbe `unsafe`. Es la misma identidad de ADR-GRP-012. **Riesgo residual**: que el PID se reutilice entre `connect` y `accept` |
| 2 | **Clasificador de agentes**: nombre `claude`, `…/claude/versions/<versión>` o una ruta que pase por `@anthropic-ai/claude-code`. Los intérpretes (`node`, `bun`, `deno`) cuentan como agente (fail-closed) hasta SPIKE-GRP-001. El recorrido solo para en el pid 1 o en un proceso de otro **euid**, y un proceso ilegible del mismo uid rechaza el comando | § 6.2 | En Terminal.app, `login` es setuid root con el ruid del usuario: hay que clasificar por euid. Medido |
| 3 | **Líder de sesión**: si es de otro euid (`login`), cuenta como "no agente" y su ascendencia no se recorre | § 6.3 | En Terminal.app el líder es `login` (root). Exigir el mismo uid dejaría fuera a todos los desarrolladores |
| 4 | **`daemon.replace`** se acepta sin los controles de reservado solo si el archivo que hay en la ruta de lanzamiento del daemon cambió desde que arrancó (`dev`, `inode`) y el ejecutable del llamante es ese archivo nuevo. En cualquier otro caso es un `daemon.stop` reservado | § 4 (SEC-13) | Cubre la carrera con `exec` y el upgrade de brew (cada versión en su carpeta del Cellar) |
| 5 | **Arranque bajo demanda**: `HOME`, `USER` y `LOGNAME` salen de la base de usuarios, no del cliente. `PATH` es fijo. El daemon hace `setsid()` al arrancar | § 3 (SEC-10) | Un `HOME` hostil no lleva el daemon a otro perfil. El MCP que lo lanzó no lo arrastra al salir |
| 6 | **Socket**: se crea bajo umask 077, así que ningún otro usuario tiene acceso en ningún momento. Después queda en 0600: macOS da 0700 a los sockets. Una ruta de más de 100 bytes se usa relativa a la carpeta de ejecución, con un `chdir` bajo mutex y la carpeta verificada tras el `chdir`. El `bind` ocurre antes de que el daemon cree hilos | § 5 | Límite de `sun_path` (Consecuencias) |
| 7 | **Confused deputy (DEP-MCP-3)**: un llamante que desciende del propio daemon, como un hook o un `git` lanzado por el futuro ejecutor de operaciones, se rechaza (`daemon-descendant`) y queda en la auditoría. **El riesgo residual de § 6 (doble fork, `setsid`) se extiende a los descendientes del daemon.** Los requisitos para el ejecutor (atribución al solicitante, contención por SO, nada heredable) están en la Dev Spec, D21 | § 6 | Q-MCP-5 |
| 8 | **Windows**: el named pipe con DACL, primera instancia, rechazo de clientes remotos, SID del servidor y SQOS sigue siendo la decisión, pero **no está implementado**. Hasta entonces el daemon arranca sin canal y el cliente responde "no soportado". Pendiente: etapa de validación multiplataforma | § 5 | Necesita Win32 sin `unsafe` propio |
| 9 | **Riesgo del modelo (revisión de seguridad, A-2)**: un agente que lanza el comando a través de otra aplicación del usuario obtiene una ascendencia limpia y una terminal de control: `open x.command` (Terminal.app), `tmux new-window` o la terminal de un IDE u orquestador. No necesita doble fork. ⚠️ **Pendiente de decisión de Rene** antes del release de `repo.retire` y `attribution.correct`. Control compensatorio previsto: cada reservado aceptado se publica (`reserved.audit`) y el Cockpit lo muestra | § 6 | Complementa los vectores de ADR-GRD-007 § 2 (`tmux send-keys`, `osascript`) |

## Enmienda (2026-10-04, ADR-GRD-008)

Aplicada desde la tabla de enmiendas de [ADR-GRD-008](./ADR-GRD-008-factor-autenticacion-fuera-de-banda.md). No cambia la forma del motor, el canal ni los controles 1 a 7. El `status` sigue en `accepted`.

| Cambio | Dónde |
|---|---|
| Relajar con el comando y aprobar en la cola: el factor es el de ADR-GRD-008, invocado por el daemon después de los controles 1 a 3 (con `daemon-descendant`), sin diálogo si fallan | § 6, tabla de Guardrails |
| `reserved-action-pending` añade el estado "esperando la autenticación del sistema" | § 6, refuerzo D5 |
| Validación 7: los casos del factor remiten a la Validación de ADR-GRD-008 | Validación 7 |
| Enmienda TS-GRP-004, punto 9 (A-2): el factor de ADR-GRD-008, en modo preferente, es el control compensatorio candidato para `repo.retire` y `attribution.correct`; sigue pendiente de decisión de Rene antes de su release | § 6 |

## Enmienda (2026-10-04, Cockpit)

Aplicada desde DEP-CKP-2, 3, 7, 9, 10 y 11 de [CTX-CKP-001](../../requirements/features/cockpit/context.md) y de [ADR-CKP-001](./ADR-CKP-001-prediccion-conflictos-merge-en-seco.md), [ADR-CKP-002](./ADR-CKP-002-catalogo-operaciones-ejecutor.md) y [ADR-CKP-003](./ADR-CKP-003-arquitectura-tui.md) (proposed). **Decisión del orquestador (2026-10-04), validada por Arquitecto**; el PO valida el alcance después. No cambia la forma del motor, el canal, la instancia única ni los controles 1 a 7. El `status` sigue en `accepted`. Si un ADR-CKP de origen no pasa a `accepted`, su fila se revisa con él.

| Cambio | Dónde | Fuente |
|---|---|---|
| Excepción acotada a "sin lanzar `git` por petición, desde el estado en memoria": consultas bajo demanda de grafo y de diff, leídas con `gix` | § 5 (SEC-08) | DEP-CKP-2, DEP-CKP-3; Q-CKP-4, Q-CKP-8 |
| "Ningún cliente embebe el motor" es, dentro del binario `raptor`, una frontera de módulo con comprobación estática | § 1 | ADR-CKP-003 § 5 y § 12 |
| El contrato de salida (SEC-12) de CLI/TUI queda llevado a ADR-GRP-004; la spec del MCP sigue pendiente | § 5 | DEP-CKP-9; ADR-CKP-003 § 8 (E1) |
| Operaciones del catálogo: confirmación de trabajo ajeno y excepción consciente del Cockpit con los controles 1 a 3; Cancelar no es reservado | § 6 | DEP-CKP-7, DEP-CKP-10; ADR-CKP-002 § 3, § 4 y § 6 |
| Las escrituras del Cockpit en el perfil (preferencias de la TUI, registro del KPI) las hace el daemon; "único escritor" no gana excepciones | § 1 | DEP-CKP-11; ADR-GRP-006 (Enmienda, Cockpit) |

**Consultas bajo demanda de grafo y de diff** (DEP-CKP-2, DEP-CKP-3):

- **Qué**: el grafo de cada worktree (commits de la rama desde su merge-base con la base confirmada, acotados por carril; ⚠️ **ASSUMPTION** de Q-CKP-4: unos 50 por carril, el resto colapsado) y el diff de un worktree en dos partes: lo que entraría al merge (`merge-base(base confirmada, rama)..rama`) y, aparte, lo sin commitear (BR-CKP-CALC-005).
- **Cómo**: con `gix` en solo lectura y **sin filtros, `textconv` ni diff externo** (ADR-GRP-009 § 1). No se lanza `git`. Binarios detectados y devueltos sin contenido. Topes por archivo, en total y de tiempo; al superarlos, "truncado".
- **Dónde**: en un pool de consultas del daemon separado del observador y del predictor de ADR-CKP-001. No consumen el presupuesto del motor de ADR-GRP-011: son respuestas a una petición, no eventos del stream.
- **SEC-08 se mantiene**: rate limit, tope de consultas en curso por conexión, cancelación al cerrarse la conexión, y respuestas fuera del orden del stream que nunca lo bloquean.
- **Para quién**: el contenido del diff solo va a la conexión CLI/TUI que lo pide. Nunca va al stream de difusión ni al MCP (Q-CKP-8), y nunca se persiste ni se registra (SEC-05, nota en `non-functional.md`).
- **Forma del contrato** (métodos, id de petición, cancelación, topes y campos): **pendiente, dueño: worker del canal (TS-GRP-004)** (necesidad N8 de ADR-CKP-003 § 4). No se fija aquí.
- **Actor por commit en el grafo** (relación commit → evento de ADR-GRP-013): opcional y no se decide aquí; sin ella, el grafo muestra "sin atribuir" (Q-CKP-4).

**Frontera de módulo** (ADR-CKP-003 § 5): el binario `raptor` contiene el motor porque `raptor daemon` es un subcomando (§ 1). Solo el módulo `daemon` de `apps/cli` importa `crates/core`. Los módulos de la TUI y de la CLI no importan `crates/core`, `crates/git` ni `crates/policy`, y una comprobación estática en CI lo hace cumplir (ADR-CKP-003, Validación V5).

**Operaciones del catálogo** (ADR-CKP-002): no son comandos reservados, porque un agente puede actuar sobre su propio trabajo. Lo que toca trabajo de otro actor exige la confirmación de ADR-TMC-005 § 3, con los controles 1 a 3 y un reto ligado a la huella del plan. La excepción consciente desde el Cockpit es un comando reservado sobre el proceso de la TUI, con D5 y D10, y no emite token (ADR-GRD-007, Enmienda (2026-10-04, Cockpit)). **Cancelar** una operación en curso no es reservado: lo puede pedir la conexión solicitante o, si se cerró, cualquier cliente CLI/TUI del usuario, nunca el MCP. En Windows, la confirmación de trabajo ajeno se rechaza (TQ-14). Pendiente: etapa de validación multiplataforma.
