---
id: ADR-GRD-008
title: Factor de autenticación del sistema operativo fuera del canal del agente
type: adr
status: proposed
date: 2026-10-04
created: 2026-10-04
updated: 2026-10-04
deciders: [Rene Bonilla]
domain: GRP
feature: guardrails
related: [ADR-GRD-007, ADR-GRD-004, ADR-GRD-006, ADR-GRP-005, ADR-GRP-012, ADR-GRP-013, CTX-GRD-001, BR-GRD-001]
tags: [guardrails, factor-fuera-de-banda, presencia-humana, localauthentication, windows-hello, polkit, fail-closed, anti-replay, br-auth-001, r-grd-3, r-grd-10, d5, q-grd-19]
---

# ADR-GRD-008 — Factor de autenticación del sistema operativo fuera del canal del agente

> **Estado**: propuesto. Queda en `proposed` hasta que Rene Bonilla responda las preguntas abiertas (OQ-GRD-008-1 a 9). US-GRD-013 y US-GRD-015 siguen bloqueadas hasta su aceptación.

## Contexto

D5 (Rene Bonilla, 2026-10-04; Q-GRD-19) exige un **factor de autenticación del SO, fuera del canal del agente**, antes de dos acciones reservadas que relajan de forma persistente: relajar la configuración con el comando de edición (US-GRD-013) y aprobar una petición de la cola (US-GRD-015). Sin el factor, esas acciones no se ofrecen (**fail-closed**).

El motivo está en ADR-GRD-007 § 2. Los controles 1 a 3 de ADR-GRP-005 § 6 (identificador no reutilizable, ascendencia, terminal y líder de sesión) no detectan cuatro vectores: el desacople del árbol (`setsid` + `script`, `launchctl submit`, `systemd-run --user`), la escritura en la pty del humano (`tmux send-keys`), la automatización de la interfaz (`osascript`, `SendKeys`) y el código plantado que el humano ejecuta sin saberlo. En los cuatro, el proceso que pide la acción **parece del humano**. Ninguna comprobación sobre el proceso llamante lo resuelve, porque el atacante es un proceso del mismo usuario.

**Pregunta central**: ¿cómo obtiene el daemon una prueba de presencia humana que no pueda fabricar un proceso descendiente de un agente, ni ningún otro proceso del mismo usuario sin interacción física, en macOS, Windows y Linux? Todo debe ser local (NFR-03) y no puede depender del canal del agente (terminal, pty, MCP).

**Restricciones activas**: no hay `architecture-constitution.md` en la cascada. Rigen ADR-GRP-001 (Rust; `clap`, `ratatui`, `rmcp`) y ADR-GRP-002 (monorepo y crates). ⚠️ **ASSUMPTION**: se tratan como constitución hasta que exista una formal (`/aadd-architect --init-constitution`). Nada de la Fase 3.

**Fuerzas de la decisión**:

1. **No fabricable por el mismo usuario**: la prueba exige un gesto biométrico o un secreto que un proceso del usuario no tiene.
2. **El verificador vive fuera del dominio escribible por el usuario**: si el estado que valida la prueba (una semilla o una clave registrada) está en el perfil, un agente lo lee o lo sustituye. El perfil lo escribe el daemon, pero cualquier proceso del usuario puede leerlo y escribirlo (ADR-GRP-006).
3. **La interfaz la dibuja el SO**, no la terminal, la pty ni el MCP.
4. **Local**: sin red ni servicios externos (NFR-03).
5. **Lo invoca el daemon desde Rust**, nunca el cliente (ADR-GRP-005 § 6: el daemon no acepta marcas del cliente).
6. **Fail-closed** si no está disponible (Q-GRD-19).

## Decisión

**Para cada acción que lo exige, el daemon invoca él mismo el diálogo de autenticación del SO: LocalAuthentication en macOS, Windows Hello en Windows 11 y polkit con agente gráfico en Linux. El diálogo se liga a un reto de un solo uso con el resumen del plan de la acción. La prueba no sale nunca del daemon: es el resultado de la llamada al SO, consumido una sola vez. Si el factor no está disponible, la acción no se ofrece. El factor se suma a los controles de ADR-GRP-005 § 6 y al anuncio, la ventana y la auditoría de D5; no sustituye a ninguno.**

### 1. Propiedad que se exige al factor

Una **prueba de presencia** es un resultado que emite un componente del SO y que cumple las tres condiciones:

1. **Interfaz propia del SO**: la dibuja un proceso del sistema, fuera de cualquier terminal, pty o canal MCP.
2. **Gesto o secreto verificado por el SO**: exige biometría, o un secreto del usuario (contraseña de sesión o PIN), que el SO valida contra un estado que el usuario no puede reescribir sin ese mismo secreto. Ese estado es el registro de Touch ID, de Windows Hello o de PAM.
3. **Resultado al llamante**: el SO devuelve el resultado al proceso que lo pidió, es decir, al daemon, y no al cliente.

La seguridad descansa en la condición 2. El aislamiento de la interfaz (condición 1) es defensa en profundidad: aunque un agente consiguiera interactuar con el diálogo, seguiría sin tener el dedo ni el secreto del humano.

### 2. Evaluación de opciones

| Opción | Frente a los vectores de ADR-GRD-007 § 2 | Invocable desde Rust | Coste de UX | Sin disponibilidad | Veredicto |
|---|---|---|---|---|---|
| **macOS · LocalAuthentication** (`deviceOwnerAuthentication`: Touch ID, Apple Watch o contraseña de sesión) | `tmux send-keys` y la pty: sin efecto, porque el diálogo no lee de la terminal. `setsid` o código plantado: pueden **provocar** el diálogo, pero no completarlo. `osascript`: haría falta el permiso de Accesibilidad **y** el dedo o la contraseña. ⚠️ Sin verificar si el diálogo del sistema rechaza eventos sintéticos | `objc2-local-authentication` (verificado en crates.io, 0.3.2). Debe correr en la **sesión de inicio del usuario**: Apple DTS desaconseja usarlo desde un LaunchDaemon global, y el autoarranque de ADR-GRP-005 § 3 es un **LaunchAgent**, que sí corre en esa sesión. ⚠️ Problema conocido de 2020: la primera llamada tras el inicio de sesión puede fallar con "UI activation timed out" (reintentar una vez) | 1 o 2 s con Touch ID; unos segundos más con la contraseña | Sin sesión gráfica (p. ej. el Mac solo por SSH): fail-closed | **Elegida** |
| macOS · Authorization Services (`AuthorizationCopyRights` con un derecho propio) | Como la anterior | FFI a `Security.framework`; definir un derecho propio exige escribir la base de derechos como administrador | Similar | — | Descartada: exige privilegios de administrador al instalar y no aporta nada frente a LocalAuthentication |
| **Windows · Windows Hello** (`IUserConsentVerifierInterop::RequestVerificationForWindowAsync`) | Igual que macOS: PIN o biometría verificados por el SO. ⚠️ Sin verificar si UIPI o el broker del diálogo rechazan `SendInput` del mismo usuario. ⚠️ Sin verificar si el **reconocimiento facial** completa la verificación sin un gesto intencional (el agente provoca el diálogo y la cámara ve al humano) | Crate `windows` (activation factory de `UserConsentVerifier`; Microsoft Learn y Raymond Chen documentan el patrón). Requiere **un HWND propio** y **Windows 11, build 22000 o posterior**. El daemon crea una ventana transitoria para el diálogo. ⚠️ Sin verificar que el diálogo obtenga el primer plano (reglas de bloqueo del primer plano) | 1 o 2 s | Windows 10 o Windows Hello no configurado (`NotConfiguredForUser`, `DeviceNotPresent`): fail-closed | **Elegida** |
| Windows · CredUI (`CredUIPromptForWindowsCredentials` y validación de la contraseña) | El secreto de la sesión | Win32 | Escribir la contraseña completa | — | Descartada: la contraseña pasa por el proceso del daemon (en contra del espíritu de SEC-GRD-12) y la validación es nuestra, no del SO |
| **Linux · polkit** (`CheckAuthorization` con una acción propia `auth_self` e interacción permitida) | El secreto (o la huella, vía `fprintd` y PAM) lo verifica el helper privilegiado de polkit. En Wayland, la entrada sintética es difícil. ⚠️ En X11, un proceso del usuario puede sintetizar entrada en el diálogo, pero sigue necesitando el secreto | `zbus_polkit` (verificado en crates.io, 5.1.0) sobre el bus del sistema. Requiere un archivo `.policy` en `/usr/share/polkit-1/actions/`, que **exige root al instalar**. Según la documentación de `robius-authentication`, el texto del diálogo sale del `.policy` y no se fija por programa. ⚠️ Sin verificar si `details` permite sustituir valores en el mensaje. ⚠️ Sin verificar qué agente atiende a un daemon bajo `systemd --user`, fuera de la sesión gráfica | Unos segundos (contraseña) | Sin agente de autenticación (SSH, contenedores, WSL sin WSLg) o sin el `.policy` instalado: fail-closed | **Elegida**, con OQ-GRD-008-1 y OQ-GRD-008-6 |
| Linux · agente en terminal (`pkttyagent`, `sudo -v`) | El diálogo vive en una terminal, la clase de canal que controla el agente; `sudo` además exige ser administrador | Sí | Bajo | — | Descartada: no está fuera del canal del agente |
| TOTP o código de un segundo dispositivo | **No cumple la fuerza 2**: el daemon tiene que guardar la semilla, y un proceso del mismo usuario la lee y genera códigos válidos. Protegerla con el llavero del SO vuelve a depender del SO | `totp-rs` u otro (sin verificar) | Abrir el móvil y teclear 6 dígitos | — | Descartada (OQ-GRD-008-2) |
| Llave FIDO2 (presencia en CTAP2) | Fuerte: toque físico en un hardware aparte; funciona sin sesión gráfica. Pero la clave pública registrada vive en el perfil y un agente la sustituye por una suya, salvo que se verifique la atestación del fabricante | Crates CTAP (sin verificar) | Comprar y llevar la llave; un toque | — | Fuera del MVP; candidata para Linux sin sesión gráfica (OQ-GRD-008-1) |

**Por qué el factor del SO**: es la única familia que cumple las seis fuerzas a la vez. El estado verificador (huella registrada, PIN de Hello, hash de la contraseña en PAM) lo guarda el SO y no se puede reescribir sin ese mismo secreto. GitRaptor no guarda ningún secreto ni ningún registro de enrolamiento propio.

`robius-authentication` (verificado en crates.io, 0.3.1) envuelve las tres plataformas detrás de una sola API. Se prefieren los bindings directos para la ruta crítica de seguridad, por control y por superficie de la cadena de suministro (SEC-GRD-15). Su uso se puede reevaluar en el SPIKE (§ 7).

### 3. Quién lo invoca y cómo se liga a la acción

**Lo invoca siempre el daemon.** El cliente (CLI o TUI) solo pide la acción y muestra el estado. El canal no tiene ningún campo de "prueba"; si el cliente envía uno, se ignora. El MCP no expone ninguna de estas acciones (BR-AUTH-004).

1. **Controles previos**: el daemon aplica primero ADR-GRP-005 § 6, puntos 1 a 3. Si fallan, rechaza la acción sin mostrar el diálogo, para que un agente detectado no pueda abrir diálogos.
2. **Plan y reto**: el daemon construye el **plan canónico** de la acción y su **resumen** (SHA-256 sobre la serialización canónica). Después genera un **nonce** de ≥ 128 bits con el CSPRNG del SO y guarda **en memoria** la entrada pendiente: nonce, resumen, acción, identidad del cliente y momento. Contenido del plan:
   - **Relajar con el comando** (US-GRD-013): nivel, clave, valor anterior y nuevo, y el identificador del contenido del archivo destino antes de escribir.
   - **Aprobar en la cola** (US-GRD-015): id de la petición, operación normalizada, repo, worktree, actor, regla y transición.
3. **Anuncio** (D5): `reserved-action-pending` en todos los clientes, con el resumen legible del plan y el estado "esperando la autenticación del sistema".
4. **Diálogo del SO**: el daemon lo invoca con un texto que arma él a partir de una **plantilla fija por acción** y de los parámetros del plan saneados con las reglas de SEC-GRD-06 (categorías Cc, Cf, Zl y Zp neutralizadas, longitud acotada). En Linux, el texto lo fija el `.policy` (§ 2) y el detalle se ve en el anuncio de los clientes.
   - **Sin reutilización**: un contexto nuevo por petición. En macOS, sin periodo de reutilización de un desbloqueo reciente. En Linux, `auth_self` y nunca `auth_self_keep`.
5. **Resultado**: solo `verificado` cuenta. Cualquier otro resultado (rechazo del usuario, cancelación, error, no disponible o tiempo agotado) anula la entrada y se audita.
6. **Ventana** (D5): con el resultado `verificado` se abre la ventana cancelable, y cualquier cliente puede cancelar. Va **después** del factor: el humano que acaba de autenticarse todavía puede cancelar si leyó mal, y la ventana no gasta tiempo antes de saber si el factor pasa (OQ-GRD-008-4).
7. **Aplicación**: al cerrarse la ventana, el daemon **recalcula el plan** con el estado actual. Ejemplos: el archivo destino no cambió, o la petición sigue pendiente y no ha caducado. Solo si el resumen coincide aplica la acción y **consume la entrada de forma atómica**. Si no coincide, aborta y lo audita como `factor-stale`.

**Anti-replay**: no existe ningún artefacto de prueba que se pueda copiar o reenviar. Un `verificado` del SO consume exactamente una entrada pendiente, ligada a un nonce y a un resumen del plan. Es el mismo patrón que el token de excepción (ADR-GRD-007 § 3) y el reto ligado al plan de la Time Machine (SEC-TMC-03), pero sin token: la prueba nunca cruza el canal.

**Vida de la entrada**: plazo del diálogo + ventana. ⚠️ **ASSUMPTION**: 60 s para responder al diálogo y la ventana de 10 s de D5 (OQ-GRD-008-4). En la cola también manda el plazo de la petición (5 min, BR-TIME-001): si la petición caduca durante el diálogo o la ventana, no se aplica nada.

**Concurrencia y fatiga**:

- **Un diálogo a la vez por usuario**: una segunda petición recibe `factor-busy` y no abre otro diálogo.
- **Limitación ante la fatiga de diálogos**: tras varios resultados no verificados seguidos, el daemon deja de abrir diálogos durante un periodo, lo anuncia y lo audita (`factor-throttled`). ⚠️ **ASSUMPTION**: 3 en 10 min y un periodo de 10 min.

**Disponibilidad y fail-closed**:

- El daemon comprueba la disponibilidad al arrancar y en cada petición: `canEvaluatePolicy` en macOS, `CheckAvailabilityAsync` en Windows, y el `.policy` instalado más un agente presente en Linux.
- **Si no está disponible**: los clientes muestran la acción como no disponible, con el motivo y cómo habilitarla. Cualquier petición se rechaza con `factor-unavailable` y se audita. **No hay degradación a D5** para US-GRD-013 ni US-GRD-015.
- Un agente que mata el agente de polkit o provoca el bloqueo por fatiga solo consigue que esas acciones no se puedan usar (denegación de servicio), nunca que se apliquen.

**La lista de acciones que exigen el factor no es configurable.** Va fijada en el binario y versionada, igual que la tabla de vectores de ADR-GRD-007 § 2. Si fuera una clave de configuración, un agente podría relajarla.

### 4. Qué se audita (ADR-GRP-013 § 1)

Cada intento entra en la auditoría append-only de los comandos reservados con los datos de D5 (cadena completa de ascendencia, terminal de control y líder de sesión) y además:

- la **acción** y el **resumen del plan**;
- el **método**: `os-localauthentication`, `os-windows-hello` o `os-polkit`;
- el **resultado**: `verified`, `denied`, `cancelled`, `timeout`, `unavailable`, `busy`, `throttled` o `stale`;
- el tipo de autenticación, solo si el SO lo informa (biometría o secreto);
- los momentos de petición, resultado, cierre de la ventana y aplicación.

**Nunca** se audita el nonce, una contraseña, un PIN ni datos biométricos. Los mensajes de error del SO se guardan como código, no como texto (SEC-GRD-12).

### 5. A qué acciones se aplica

| Acción | Con este ADR | Después |
|---|---|---|
| Relajar la configuración con el comando (US-GRD-013) | **Obligatorio**; fail-closed. Endurecer no lo pide | — |
| Aprobar una petición de la cola (US-GRD-015) | **Obligatorio**; fail-closed. Rechazar no lo pide (mantiene la protección) | — |
| Desinstalar o retirar una huérfana (US-GRD-003) | Mecanismo MVP de D5, sin cambios | Lo adopta cuando se decida (OQ-GRD-008-3) |
| Excepción consciente (US-GRD-006; también en el Cockpit, D10) | Mecanismo MVP de D5, sin cambios | Igual (OQ-GRD-008-3) |
| Confirmaciones de D8 (US-GRD-007, US-GRD-014) | Mecanismo MVP de D5, sin cambios (no las bloquea, D8) | Igual; al adoptarlo se cierra R-GRD-10 (OQ-GRD-008-3) |
| Instalar, registrar la denegación, adoptar una huérfana | No (no relajan) | — |
| Confirmar trabajo ajeno en la Time Machine (TQ-14 b, Fase 2) | Fuera de alcance | Puede reutilizar esta capacidad |

**Adoptarlo después no requiere otro diseño**: es una fila más en la lista fija del binario, más sus pruebas. Mientras una acción no lo adopte, sigue con D5 y su aceptación del riesgo por acción.

**Lo que este ADR no cierra** (OQ-GRD-008-8):

- **Nivel de equipo**: el comando de US-GRD-013 solo escribe en el working tree. La relajación rige cuando llega a la rama principal y el humano la confirma (D7, D8), y esa confirmación **todavía no lleva el factor**. Para el equipo, el gate real es D8.
- **Niveles personales**: el perfil y el local son archivos que un proceso del usuario puede editar sin pasar por el comando. Es el riesgo R-GRD-4. Mientras siga abierto, el factor del comando no impide que un agente quite un endurecimiento personal editando el archivo.

### 6. Ubicación en el código

- **Módulo de presencia del daemon**: detrás de un trait con una implementación por SO (`cfg(target_os)`), alojado en el daemon (`raptor daemon`, ADR-GRP-005). Ningún cliente lo enlaza.
- **Dependencias nuevas** (SEC-GRD-15 y SEC-07 de motor-local):
  - `objc2-local-authentication` en macOS;
  - `windows` en Windows (⚠️ los nombres de las features no están verificados);
  - `zbus_polkit` en Linux.
- **Distribución en Linux**: el `.policy` exige root y no lo puede instalar un canal por usuario (npm o script). Se habilita con un paso privilegiado explícito o con paquetes del sistema (OQ-GRD-008-6).
- **Doble de prueba**: una implementación programable (verificado, rechazado, no disponible, tiempo agotado) que **solo se compila en builds de test**, como la variable de sobreescritura del perfil de SEC-06. Ningún entorno ni ninguna bandera la selecciona en release.

### 7. Validación previa: SPIKE de viabilidad

Se propone **SPIKE-GRD-002 — Factor del SO desde el daemon en los tres SO** (todavía sin crear; ver Enmiendas). Debe responder estas preguntas:

1. LocalAuthentication desde el LaunchAgent y desde el daemon arrancado bajo demanda con entorno limpio (SEC-10); el fallo de la primera llamada tras el inicio de sesión.
2. Windows Hello con una ventana transitoria del daemon: primer plano y comportamiento del reconocimiento facial.
3. polkit desde `systemd --user` y desde el arranque bajo demanda: qué agente atiende la petición, sujeto y sesión, y sustitución en el mensaje.
4. Comportamiento de los tres diálogos ante entrada sintética del mismo usuario (`osascript` con y sin Accesibilidad, `SendInput`, `xdotool` en X11).

Si una plataforma no es viable, queda en fail-closed sin cambiar este ADR (OQ-GRD-008-9).

## Alternativas consideradas

| Alternativa | Por qué no |
|---|---|
| Solo los controles de ADR-GRP-005 § 6 y D5 para US-GRD-013 y US-GRD-015 | Contradice D5: no cierran los vectores de ADR-GRD-007 § 2 |
| El cliente invoca el diálogo y envía la prueba | El daemon no confía en marcas del cliente (ADR-GRP-005 § 6); un agente hablaría JSON-RPC directo con una "prueba" inventada |
| Un token de prueba que viaja por el canal, firmado por el daemon | Añade un artefacto que se puede copiar; con el resultado dentro del daemon basta |
| TOTP, código por correo o push a un móvil | TOTP no cumple la fuerza 2; el correo y el push exigen red y servicios externos (NFR-03) |
| Diálogo propio de GitRaptor (ventana o TUI) | Lo dibuja un proceso del usuario y no verifica ningún secreto: un agente lo automatiza |
| Pedir la contraseña en la terminal y validarla con PAM | Es el canal del agente; además, la contraseña pasaría por el proceso |
| Degradar a D5 cuando el factor no esté disponible | Contradice Q-GRD-19 (fail-closed); un agente podría forzar la degradación matando el agente de polkit |
| Lista de acciones con factor configurable | Un agente la relajaría editando la configuración |

## Consecuencias

- ✅ Cierra para US-GRD-013 y US-GRD-015 los cuatro vectores de ADR-GRD-007 § 2 que dicen "No": la prueba exige un gesto o un secreto que el agente no tiene.
- ✅ GitRaptor no guarda secretos ni registros de enrolamiento: el estado verificador es del SO.
- ✅ Una sola capacidad del daemon, reutilizable por las demás acciones que relajan y por la Time Machine (TQ-14 b).
- ⚠️ **Disponibilidad desigual**: Windows 10, Windows Hello sin configurar, Linux sin sesión gráfica o sin el `.policy`, y los Mac sin sesión gráfica quedan sin US-GRD-013 ni US-GRD-015 (fail-closed).
- ⚠️ **Aprobación refleja**: un agente puede provocar el diálogo por un vector no detectable y el humano puede aceptarlo sin leer. Lo limitan el texto del diálogo, el anuncio, la ventana posterior, un diálogo a la vez, la limitación por fatiga y la auditoría. Es un riesgo residual.
- ⚠️ **Secreto filtrado**: si el agente conoce la contraseña o el PIN del usuario (p. ej. porque el humano la escribió en una terminal que el agente observa), las variantes con secreto caen. Las que solo admiten biometría no (OQ-GRD-008-5).
- ⚠️ **Linux exige un paso con root** para instalar el `.policy` (OQ-GRD-008-6), y el texto del diálogo no lleva el detalle de la acción.
- ⚠️ **Tres dependencias nuevas, una por SO**, en la ruta crítica de seguridad (SEC-GRD-15).
- ⚠️ **No cierra R-GRD-4 ni R-GRD-10 por sí solo** (§ 5; OQ-GRD-008-3 y OQ-GRD-008-8).
- ⚠️ **CI sin humano**: la propiedad real (que el diálogo no se puede completar sin el humano) solo se verifica a mano, por SO, antes de cada release que toque el módulo.

## Validación

Todas las pruebas automáticas usan el doble de prueba y repos y perfiles temporales, nunca el repo de GitRaptor.

1. **Fail-closed**: con el doble en "no disponible", los clientes muestran relajar y aprobar como no disponibles con el motivo; una petición por la CLI o por JSON-RPC directo se rechaza con `factor-unavailable`, no escribe nada y se audita.
2. **Controles previos**: una petición desde un agente simulado (CLI, JSON-RPC directo, pty) se rechaza **sin** invocar el doble.
3. **Sin prueba del cliente**: una petición con un campo de "prueba" inventado se trata igual que sin él.
4. **Ligadura**: si el archivo destino o la petición de la cola cambian entre el `verificado` y el cierre de la ventana, no se aplica nada y se audita `factor-stale`.
5. **Un solo uso**: un `verificado` aplica una acción; una segunda aplicación con la misma entrada se rechaza.
6. **Ventana**: tras un `verificado`, otro cliente cancela dentro de la ventana y no se aplica nada.
7. **Plazo de la cola**: una petición que caduca durante el diálogo no se aprueba aunque el doble devuelva `verificado` después.
8. **Concurrencia y fatiga**: dos peticiones a la vez abren un solo diálogo (`factor-busy`); tres resultados no verificados seguidos activan `factor-throttled`.
9. **Vectores** (el doble simula que el humano no acepta): un `tmux send-keys` en la pty del humano que lanza el comando de relajar abre el diálogo, se rechaza y no escribe nada.
10. **Auditoría**: cada caso anterior deja su entrada con método, resultado y ascendencia; el escáner de secretos no encuentra el nonce ni secretos plantados.
11. **MCP**: el catálogo no ofrece estas acciones y el canal las rechaza desde la conexión del MCP.
12. **Build de release**: un gate de CI comprueba que el binario de release no contiene el doble y que ninguna variable ni bandera lo activa.
13. **Manual por SO** (gate de release; lo ejecuta un humano): Touch ID y contraseña desde el LaunchAgent y bajo demanda; Windows Hello en Windows 11 con PIN y con biometría; polkit en GNOME (Wayland) y KDE con el `.policy`; por SSH sin sesión gráfica, fail-closed; los intentos de entrada sintética de SPIKE-GRD-002 no completan el diálogo.

## Preguntas abiertas (OQ-GRD-008-n)

Decisiones de producto o de alcance para Rene Bonilla. El ADR queda `proposed` hasta que se respondan.

| ID | Pregunta | Opciones | Recomendación del Arquitecto |
|---|---|---|---|
| OQ-GRD-008-1 | ¿Qué pasa en Linux sin sesión gráfica (SSH, contenedores, WSL sin WSLg)? | (a) Fail-closed: no hay relajar ni aprobar. (b) Agente de polkit en terminal. (c) Llave FIDO2 con atestación. (d) Pedir a Windows Hello desde WSL | **(a) en el MVP**. (b) vuelve al canal del agente. (c) es la mejor candidata después del MVP. (d) depende de la interoperabilidad de WSL y no está estudiada |
| OQ-GRD-008-2 | ¿Entra TOTP o un segundo dispositivo en el MVP? | (a) No. (b) Sí, como alternativa donde no haya factor del SO | **(a)**: la semilla la puede leer cualquier proceso del usuario (fuerza 2), así que no cierra R-GRD-3 |
| OQ-GRD-008-3 | ¿Cuándo adoptan el factor desinstalar, la excepción y las confirmaciones de D8? | (a) Ya en el MVP, obligatorio y fail-closed. (b) Después de US-GRD-013 y US-GRD-015, como "factor si está disponible; D5 si no", con la ausencia en la auditoría. (c) Nunca | **(b)**, empezando por desinstalar y D8 (cierra R-GRD-10). (a) dejaría sin desinstalar a quien no tenga factor; (b) no empeora el MVP y endurece donde se puede |
| OQ-GRD-008-4 | ¿Cuánto tiempo vale la prueba y se mantiene la ventana de 10 s después del factor? | Plazo del diálogo: 30, 60 o 120 s. Ventana: (a) se mantiene. (b) Se quita cuando el factor pasa | **60 s para el diálogo, sin reutilizar desbloqueos recientes, y (a)**: D5 pide D5 más el factor, y la ventana posterior es la defensa contra la aprobación refleja |
| OQ-GRD-008-5 | En macOS, ¿solo biometría o biometría y contraseña? | (a) `deviceOwnerAuthentication` (Touch ID, Apple Watch o contraseña). (b) Solo biometría, fail-closed sin Touch ID | **(a)**: muchos Mac de escritorio no tienen Touch ID, y el agente no conoce la contraseña. Opcional después: preferir biometría cuando exista |
| OQ-GRD-008-6 | ¿Cómo se instala el `.policy` de polkit, que exige root? | (a) Paso privilegiado explícito que ejecuta el usuario (p. ej. un subcomando con `sudo` que el `doctor` sugiere). (b) Solo en paquetes del sistema. (c) Sin polkit: Linux siempre fail-closed | **(a)**, con fail-closed hasta que se haga. El paso es reversible y lo desinstala la desinstalación del producto |
| OQ-GRD-008-7 | ¿Windows 10 o Windows Hello sin configurar? | (a) Fail-closed con la guía para configurar el PIN de Windows Hello. (b) CredUI como alternativa | **(a)**: CredUI hace pasar la contraseña por el daemon. Windows 10 ya no tiene soporte general desde octubre de 2025 |
| OQ-GRD-008-8 | El factor del comando no impide relajar un nivel personal editando el archivo (R-GRD-4). ¿Se cierra? | (a) R-GRD-4 queda aceptado y el factor solo protege el comando. (b) Llevar la regla de D7 a los niveles personales: quitar un endurecimiento personal queda pendiente hasta confirmarlo con el factor (enmienda de ADR-GRD-004). (c) Que solo el daemon escriba los archivos personales | **(b), antes de la Dev Spec de US-GRD-013**. Sin ella, el factor del comando tiene poco efecto de seguridad. (c) no impide escribir a un proceso del mismo usuario |
| OQ-GRD-008-9 | ¿Se acepta el ADR antes o después de SPIKE-GRD-002? | (a) Aceptar la forma ahora; el SPIKE gatea las Dev Specs de US-GRD-013 y US-GRD-015. (b) Esperar al SPIKE | **(a)**: la decisión no depende del SPIKE. Si una plataforma no es viable, queda en fail-closed sin reabrir el ADR |

## Enmiendas requeridas en otros artefactos

No se aplican desde este ADR. Se aplican cuando Rene Bonilla lo acepte.

| Artefacto | Enmienda |
|---|---|
| ADR-GRP-005 § 6 | En las filas "Relajar la configuración" y "Aprobar una petición de la cola", citar ADR-GRD-008 como el mecanismo del factor. Añadir el estado "esperando la autenticación del sistema" a `reserved-action-pending`. Validación 7: remitir a la Validación de ADR-GRD-008 |
| ADR-GRD-007 § 1, § 2 y Consecuencias | Filas de relajar y aprobar: "factor de ADR-GRD-008". § 2, columna "Qué lo cierra": "ADR-GRD-008, en las acciones que lo adoptan". Consecuencias: "requiere un ADR propio" pasa a ADR-GRD-008 |
| ADR-GRP-013 § 1 | La auditoría de los comandos reservados incluye los intentos del factor con método, resultado y momentos (§ 4) |
| ADR-GRD-006 § 1 | A valorar: si el registro de decisiones de la cola anota que la aprobación se hizo con el factor |
| ADR-GRD-004 § 2 y § 4 | Solo si OQ-GRD-008-8 = (b): quitar un endurecimiento personal queda pendiente hasta confirmarlo con el factor |
| `non-functional-guardrails.md` | SEC-GRD-05: citar ADR-GRD-008. Nuevo SEC-GRD-20 (factor fuera de banda: invocado por el daemon, ligado al plan, un solo uso, fail-closed, lista no configurable, doble solo en test). Riesgos residuales: aprobación refleja, secreto filtrado, entrada sintética en X11, denegación de servicio matando el agente de polkit. Fila J10 "Nuevo ADR": aplicada cuando se acepte |
| `technical-stories.md` (Guardrails) | Fila nueva para SPIKE-GRD-002 (§ 7) y su archivo; US-GRD-013 y US-GRD-015 pasan a esperar también a SPIKE-GRD-002 |
| US-GRD-013, US-GRD-015 | Dependencias: citar ADR-GRD-008 y SPIKE-GRD-002. Requisitos Técnicos (Arquitecto, Fase 2) |
| `context.md` de Guardrails (PO) | R-GRD-3: citar el mecanismo; R-GRD-4 según OQ-GRD-008-8; Q-GRD-19 con la referencia al ADR |
| ADR-TMC-005 (TQ-14 b) | Nota informativa: la presencia verificada por el SO de la Fase 2 puede reutilizar esta capacidad |

## Referencias

- **Reglas y riesgos**: BR-AUTH-001, BR-AUTH-004, BR-TIME-001; Q-GRD-19, Q-GRD-22, Q-GRD-24; R-GRD-3, R-GRD-4, R-GRD-10; NFR-03.
- **Decisiones**: D5, D8 y D10 de Rene Bonilla (2026-10-04).
- **Historias**: US-GRD-013 y US-GRD-015 (gate duro); US-GRD-003, US-GRD-006, US-GRD-007 y US-GRD-014 (adopción posterior).
- **ADRs**: ADR-GRD-007 § 1 a § 3 (padre), ADR-GRD-004 § 2 y § 4, ADR-GRD-006 § 1; ADR-GRP-005 § 3 y § 6, ADR-GRP-006, ADR-GRP-012, ADR-GRP-013 § 1; ADR-TMC-005 (SEC-TMC-03, TQ-14).
- **Seguridad**: SEC-GRD-05, SEC-GRD-06, SEC-GRD-12, SEC-GRD-15; SEC-06, SEC-07 y SEC-10 de motor-local; OWASP LLM06.
- **Fuentes externas consultadas (2026-10-04)**: crates.io (`objc2-local-authentication` 0.3.2, `zbus_polkit` 5.1.0, `robius-authentication` 0.3.1); README de `robius-authentication` (requisitos de polkit); Microsoft Learn, `IUserConsentVerifierInterop::RequestVerificationForWindowAsync` (build 22000, HWND); Apple Developer Forums (LocalAuthentication en la sesión de usuario, no en un LaunchDaemon).
