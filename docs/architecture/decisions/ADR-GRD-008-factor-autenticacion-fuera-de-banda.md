---
id: ADR-GRD-008
title: Factor de autenticación del sistema operativo fuera del canal del agente
type: adr
status: accepted
accepted: 2026-10-04
date: 2026-10-04
created: 2026-10-04
updated: 2026-10-04
deciders: [Rene Bonilla, Orquestador (autonomía delegada por Rene Bonilla), Arquitecto, PO]
domain: GRP
feature: guardrails
related: [ADR-GRD-007, ADR-GRD-004, ADR-GRD-006, ADR-GRP-005, ADR-GRP-012, ADR-GRP-013, CTX-GRD-001, BR-GRD-001, SPIKE-GRD-002, CTX-CKP-001]
tags: [guardrails, factor-fuera-de-banda, presencia-humana, localauthentication, windows-hello, polkit, fail-closed, anti-replay, br-auth-001, r-grd-3, r-grd-4, r-grd-10, d5, q-grd-19, q-grd-32, dep-mcp-3, dep-ckp-8]
---

# ADR-GRD-008 — Factor de autenticación del sistema operativo fuera del canal del agente

> **Estado**: aceptado el 2026-10-04. Las nueve preguntas abiertas (OQ-GRD-008-1 a 9) son **Decisión del orquestador (2026-10-04), validada por Arquitecto/PO**, bajo la autonomía que Rene Bonilla delegó en el orquestador (ver "Preguntas resueltas"). El ADR fija la forma; SPIKE-GRD-002 gatea las Dev Specs de US-GRD-013 y US-GRD-015 y elige el binding de cada SO (§ 6). Linux y Windows: **Pendiente: etapa de validación multiplataforma**.

## Contexto

D5 (Rene Bonilla, 2026-10-04; Q-GRD-19) exige un **factor de autenticación del SO, fuera del canal del agente**, antes de dos acciones reservadas que relajan de forma persistente: relajar la configuración con el comando de edición (US-GRD-013) y aprobar una petición de la cola (US-GRD-015). Sin el factor, esas acciones no se ofrecen (**fail-closed**).

El motivo está en ADR-GRD-007 § 2. Los controles 1 a 3 de ADR-GRP-005 § 6 (identificador no reutilizable, ascendencia, terminal y líder de sesión) no detectan cuatro vectores: el desacople del árbol (`setsid` + `script`, `launchctl submit`, `systemd-run --user`), la escritura en la pty del humano (`tmux send-keys`), la automatización de la interfaz (`osascript`, `SendKeys`) y el código plantado que el humano ejecuta sin saberlo. A esos cuatro se suma el riesgo **A-2** de la revisión de seguridad del canal (ADR-GRP-005, Enmienda TS-GRP-004, punto 9): un agente lanza el comando a través de otra aplicación del usuario (`open x.command`, `tmux new-window`, la terminal de un IDE u orquestador) y obtiene una ascendencia limpia y una terminal de control. En todos, el proceso que pide la acción **parece del humano**. Ninguna comprobación sobre el proceso llamante lo resuelve, porque el atacante es un proceso del mismo usuario.

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
| **macOS · LocalAuthentication** (`deviceOwnerAuthentication`: Touch ID, Apple Watch o contraseña de sesión) | `tmux send-keys` y la pty: sin efecto, porque el diálogo no lee de la terminal. `setsid` o código plantado: pueden **provocar** el diálogo, pero no completarlo. `osascript`: haría falta el permiso de Accesibilidad **y** el dedo o la contraseña. ⚠️ Sin verificar si el diálogo del sistema rechaza eventos sintéticos | `objc2-local-authentication` (verificado en crates.io, 0.3.2). Debe correr en la **sesión de inicio del usuario**: Apple DTS desaconseja usarlo desde un LaunchDaemon global, y el autoarranque de ADR-GRP-005 § 3 es un **LaunchAgent**, que sí corre en esa sesión. El **arranque bajo demanda** (`setsid` y entorno limpio, Enmienda TS-GRP-004, punto 5) hereda la sesión gráfica del padre: si lo arranca un cliente por SSH, no hay sesión gráfica y es fail-closed aunque el humano tenga la sesión abierta; como el daemon es único, `raptor doctor` lo explica y sugiere reiniciarlo desde la sesión gráfica. ⚠️ Las llamadas de `LAContext` en este crate son `unsafe fn` y el workspace tiene `unsafe_code = "forbid"`: el binding lo elige SPIKE-GRD-002 (§ 6). ⚠️ Problema conocido de 2020: la primera llamada tras el inicio de sesión puede fallar con "UI activation timed out" (reintentar una vez) | 1 o 2 s con Touch ID; unos segundos más con la contraseña | Sin sesión gráfica (p. ej. el Mac solo por SSH): fail-closed | **Elegida** |
| macOS · Authorization Services (`AuthorizationCopyRights` con un derecho propio) | Como la anterior | FFI a `Security.framework`; definir un derecho propio exige escribir la base de derechos como administrador | Similar | — | Descartada: exige privilegios de administrador al instalar y no aporta nada frente a LocalAuthentication |
| **Windows · Windows Hello** (`IUserConsentVerifierInterop::RequestVerificationForWindowAsync`) | Igual que macOS: PIN o biometría verificados por el SO. ⚠️ Sin verificar si UIPI o el broker del diálogo rechazan `SendInput` del mismo usuario. ⚠️ Sin verificar si el **reconocimiento facial** completa la verificación sin un gesto intencional (el agente provoca el diálogo y la cámara ve al humano) | Crate `windows` (activation factory de `UserConsentVerifier`; Microsoft Learn y Raymond Chen documentan el patrón). Requiere **un HWND propio** y **Windows 11, build 22000 o posterior**. ⚠️ Las llamadas COM y de ventana probablemente exigen `unsafe` (sin comprobar; § 6). Hoy el daemon no tiene canal en Windows (Enmienda TS-GRP-004, punto 8): el cliente responde "no soportado" hasta la etapa de validación multiplataforma. El daemon crea una ventana transitoria para el diálogo. ⚠️ Sin verificar que el diálogo obtenga el primer plano (reglas de bloqueo del primer plano) | 1 o 2 s | Windows 10 o Windows Hello no configurado (`NotConfiguredForUser`, `DeviceNotPresent`): fail-closed | **Elegida** |
| Windows · CredUI (`CredUIPromptForWindowsCredentials` y validación de la contraseña) | El secreto de la sesión | Win32 | Escribir la contraseña completa | — | Descartada: la contraseña pasa por el proceso del daemon (en contra del espíritu de SEC-GRD-12) y la validación es nuestra, no del SO |
| **Linux · polkit** (`CheckAuthorization` con una acción propia `auth_self` e interacción permitida) | El secreto (o la huella, vía `fprintd` y PAM) lo verifica el helper privilegiado de polkit. En Wayland, la entrada sintética es difícil. ⚠️ En X11, un proceso del usuario puede sintetizar entrada en el diálogo, pero sigue necesitando el secreto | `zbus_polkit` (verificado en crates.io, 5.1.0) sobre el bus del sistema. Requiere un archivo `.policy` en `/usr/share/polkit-1/actions/`, que **exige root al instalar**. Según la documentación de `robius-authentication`, el texto del diálogo sale del `.policy` y no se fija por programa. ⚠️ Sin verificar si `details` permite sustituir valores en el mensaje. ⚠️ Sin verificar qué agente atiende a un daemon bajo `systemd --user`, fuera de la sesión gráfica | Unos segundos (contraseña) | Sin agente de autenticación (SSH, contenedores, WSL sin WSLg) o sin el `.policy` instalado: fail-closed | **Elegida**, con OQ-GRD-008-1 y OQ-GRD-008-6 |
| Linux · agente en terminal (`pkttyagent`, `sudo -v`) | El diálogo vive en una terminal, la clase de canal que controla el agente; `sudo` además exige ser administrador | Sí | Bajo | — | Descartada: no está fuera del canal del agente |
| TOTP o código de un segundo dispositivo | **No cumple la fuerza 2**: el daemon tiene que guardar la semilla, y un proceso del mismo usuario la lee y genera códigos válidos. Protegerla con el llavero del SO vuelve a depender del SO | `totp-rs` u otro (sin verificar) | Abrir el móvil y teclear 6 dígitos | — | Descartada (OQ-GRD-008-2) |
| Llave FIDO2 (presencia en CTAP2) | Fuerte: toque físico en un hardware aparte; funciona sin sesión gráfica. Pero la clave pública registrada vive en el perfil y un agente la sustituye por una suya, salvo que se verifique la atestación del fabricante | Crates CTAP (sin verificar) | Comprar y llevar la llave; un toque | — | Fuera del MVP; candidata para Linux sin sesión gráfica (OQ-GRD-008-1) |

**Por qué el factor del SO**: es la única familia que cumple las seis fuerzas a la vez. El estado verificador (huella registrada, PIN de Hello, hash de la contraseña en PAM) lo guarda el SO y no se puede reescribir sin ese mismo secreto. GitRaptor no guarda ningún secreto ni ningún registro de enrolamiento propio.

`robius-authentication` (verificado en crates.io, 0.3.1) envuelve las tres plataformas detrás de una sola API. **El binding de cada SO lo elige SPIKE-GRD-002** entre dos opciones compatibles con `unsafe_code = "forbid"` del workspace (`Cargo.toml`): `robius-authentication` con versión fijada y auditada (SEC-GRD-15), o un crate aislado con una excepción de `unsafe` aprobada en un ADR propio. Si ninguna sirve en un SO, ese SO queda en fail-closed sin reabrir este ADR (OQ-GRD-008-9). `zbus_polkit` no necesita `unsafe`.

### 3. Quién lo invoca y cómo se liga a la acción

**Lo invoca siempre el daemon.** El cliente (CLI o TUI) solo pide la acción y muestra el estado. El canal no tiene ningún campo de "prueba"; si el cliente envía uno, se ignora. El MCP no expone ninguna de estas acciones (BR-AUTH-004).

1. **Controles previos**: el daemon aplica primero ADR-GRP-005 § 6, puntos 1 a 3, con la Enmienda TS-GRP-004: identidad `(pid, inicio)` comprobada en cada petición en macOS, clasificador por euid (los intérpretes cuentan como agente hasta SPIKE-GRP-001) y rechazo **`daemon-descendant`** de los descendientes del propio daemon (DEP-MCP-3, confused deputy). Si fallan, rechaza la acción **sin mostrar el diálogo** (`precheck-denied`), para que un agente detectado no pueda abrir diálogos. Si la conexión que pidió la acción se cierra antes de aplicarla, la entrada se anula (`cancelled`).
2. **Plan y reto**: el daemon construye el **plan canónico** de la acción y su **resumen** (SHA-256 sobre la serialización canónica). Después genera un **nonce** de ≥ 128 bits con el CSPRNG del SO y guarda **en memoria** la entrada pendiente: nonce, resumen, acción, identidad del cliente y momento. Contenido del plan:
   - **Relajar con el comando** (US-GRD-013): nivel, clave, valor anterior y nuevo, y el identificador del contenido del archivo destino antes de escribir.
   - **Aprobar en la cola** (US-GRD-015): id de la petición, operación normalizada, repo, worktree, actor, regla y transición.
3. **Anuncio** (D5): `reserved-action-pending` en todos los clientes, con el resumen legible del plan y el estado "esperando la autenticación del sistema". El **Cockpit** es un cliente más: aprobar desde su cola pasa por el mismo factor, que invoca el daemon, y muestra ese estado; rechazar es un comando reservado sin factor y sin ventana (Q-CKP-14). DEP-CKP-8 se cierra con US-GRD-015 + este ADR + SPIKE-GRD-002.
4. **Diálogo del SO**: el daemon lo invoca con un texto que arma él a partir de una **plantilla fija por acción** y de los parámetros del plan saneados con las reglas de SEC-GRD-06 (categorías Cc, Cf, Zl y Zp neutralizadas, longitud acotada). En Linux, el texto lo fija el `.policy` (§ 2) y el detalle se ve en el anuncio de los clientes.
   - **Sin reutilización**: un contexto nuevo por petición. En macOS, sin periodo de reutilización de un desbloqueo reciente. En Linux, `auth_self` y nunca `auth_self_keep`.
5. **Resultado**: solo `verificado` cuenta. Cualquier otro resultado (rechazo del usuario, cancelación, error, no disponible o tiempo agotado) anula la entrada y se audita.
6. **Ventana** (D5): con el resultado `verificado` se abre la ventana cancelable, y cualquier cliente puede cancelar. Va **después** del factor: el humano que acaba de autenticarse todavía puede cancelar si leyó mal, y la ventana no gasta tiempo antes de saber si el factor pasa (OQ-GRD-008-4).
7. **Aplicación**: al cerrarse la ventana, el daemon **recalcula el plan** con el estado actual. Ejemplos: el archivo destino no cambió, o la petición sigue pendiente y no ha caducado. Solo si el resumen coincide aplica la acción y **consume la entrada de forma atómica**. Si no coincide, aborta y lo audita como `factor-stale`.

**Anti-replay**: no existe ningún artefacto de prueba que se pueda copiar o reenviar. Un `verificado` del SO consume exactamente una entrada pendiente, ligada a un nonce y a un resumen del plan. Es el mismo patrón que el token de excepción (ADR-GRD-007 § 3) y el reto ligado al plan de la Time Machine (SEC-TMC-03), pero sin token: la prueba nunca cruza el canal.

**Vida de la entrada** (OQ-GRD-008-4): 60 s para responder al diálogo + la ventana de 10 s de D5. El plazo lo impone el **daemon**: a los 60 s anula la entrada y descarta cualquier resultado tardío, aunque el SO no cierre el diálogo. En la cola manda lo que venza antes: 60 + 10 s o el plazo de la petición (5 min, BR-TIME-001); si la petición caduca durante el diálogo o la ventana, no se aplica nada.

**Concurrencia y fatiga**:

- **Un diálogo a la vez por usuario**: una segunda petición recibe `factor-busy` y no abre otro diálogo.
- **Limitación ante la fatiga de diálogos**: tras varios resultados no verificados seguidos, el daemon deja de abrir diálogos durante un periodo, lo anuncia y lo audita (`factor-throttled`). ⚠️ **ASSUMPTION**: 3 en 10 min y un periodo de 10 min.

**Disponibilidad y fail-closed**:

- El daemon comprueba la disponibilidad al arrancar y en cada petición: `canEvaluatePolicy` en macOS, `CheckAvailabilityAsync` en Windows, y en Linux el `.policy` instalado con el contenido esperado (hash fijo en el binario), propiedad de root y no escribible por el usuario, más un agente presente.
- **Sin sesión gráfica** (Linux por SSH, contenedores, WSL sin WSLg, un Mac por SSH o un daemon arrancado bajo demanda desde SSH), **Windows 10 o Windows Hello sin configurar**: no disponible (OQ-GRD-008-1 y 7). Windows, además, no tiene canal hasta la etapa multiplataforma.
- **Si no está disponible**: los clientes muestran la acción como no disponible, con el motivo y cómo habilitarla. Cualquier petición se rechaza con `factor-unavailable` y se audita. **No hay degradación a D5** para US-GRD-013 ni US-GRD-015.
- **Endurecer y rechazar nunca exigen el factor**: nadie queda sin poder proteger. Una petición de la cola que nadie puede aprobar caduca y se deniega.
- El motivo se explica sin instrucciones para saltarse la protección, con la guía para habilitar el factor (p. ej. configurar el PIN de Windows Hello).
- Un agente que mata el agente de polkit o provoca el bloqueo por fatiga solo consigue que esas acciones no se puedan usar (denegación de servicio), nunca que se apliquen.

**La lista de acciones que exigen el factor no es configurable.** Va fijada en el binario y versionada, igual que la tabla de vectores de ADR-GRD-007 § 2. Si fuera una clave de configuración, un agente podría relajarla.

### 4. Qué se audita (ADR-GRP-013 § 1)

Cada intento entra en la auditoría append-only de los comandos reservados con los datos de D5 (cadena completa de ascendencia, terminal de control y líder de sesión) y además:

- la **acción** y el **resumen del plan**;
- el **método**: `os-localauthentication`, `os-windows-hello` o `os-polkit`;
- el **resultado**: `verified`, `denied`, `cancelled`, `timeout`, `unavailable`, `busy`, `throttled`, `stale` o `precheck-denied` (incluye `daemon-descendant`, sin diálogo);
- el tipo de autenticación, solo si el SO lo informa (biometría o secreto); si no, `not-reported` (probable en macOS; lo comprueba SPIKE-GRD-002);
- los momentos de petición, resultado, cierre de la ventana y aplicación.

Cada intento se publica también en `reserved.audit`, para que el Cockpit lo muestre (control compensatorio de A-2).

**Nunca** se audita el nonce, una contraseña, un PIN ni datos biométricos. Los mensajes de error del SO se guardan como código, no como texto (SEC-GRD-12).

### 5. A qué acciones se aplica

| Acción | Con este ADR | Después |
|---|---|---|
| Relajar la configuración con el comando (US-GRD-013) | **Obligatorio**; fail-closed. Endurecer no lo pide | — |
| Aprobar una petición de la cola (US-GRD-015) | **Obligatorio**; fail-closed. Rechazar no lo pide (mantiene la protección) | — |
| Quitar un endurecimiento personal editando el perfil o el local (Q-GRD-32) | **Obligatorio** para confirmar la relajación pendiente (`personal-relax-pending`); fail-closed. Enmienda de ADR-GRD-004, requisito antes de la Dev Spec de US-GRD-013 (OQ-GRD-008-8) | — |
| Confirmaciones de D8 (US-GRD-007, US-GRD-014) | Mecanismo MVP de D5, sin cambios (no las bloquea, D8) | Historia posterior (OQ-GRD-008-3), la primera: **preferente**, D5 solo si `unavailable`; cierra R-GRD-10 |
| Desinstalar o retirar una huérfana (US-GRD-003) | Mecanismo MVP de D5, sin cambios | Historia posterior (OQ-GRD-008-3), junto con D8: preferente |
| Excepción consciente (US-GRD-006; también en el Cockpit, D10) | Mecanismo MVP de D5, sin cambios | Historia posterior (OQ-GRD-008-3), la última (frecuencia y fatiga): preferente |
| `repo.retire`, `attribution.correct` (A-2) y añadir un repo a la allowlist del MCP (DEP-MCP-3, Q-MCP-3) | Comandos reservados de ADR-GRP-005 § 6, sin cambios | Candidatos para la misma vía preferente; A-2 es decisión pendiente de Rene antes de su release |
| Instalar, registrar la denegación, adoptar una huérfana | No (no relajan) | — |
| Confirmar trabajo ajeno en la Time Machine (TQ-14 b, Fase 2) | Fuera de alcance | Puede reutilizar esta capacidad |

**Adoptarlo después no requiere otro diseño**: es una fila más en la lista fija del binario, más sus pruebas. Cada acción va fijada como **obligatoria** (fail-closed) o **preferente**. En el modo preferente **solo `unavailable` cae a D5**, y la ausencia queda en el anuncio y en la auditoría; `denied`, `cancelled`, `timeout`, `busy`, `throttled` y cualquier error son rechazo sin caída, porque si no un agente provocaría diálogos hasta el bloqueo por fatiga para forzar D5. El modo no es configurable. Mientras una acción no lo adopte, sigue con D5 y su aceptación del riesgo por acción.

**Niveles personales: trinquete con el factor** (OQ-GRD-008-8, Q-GRD-32; enmienda de ADR-GRD-004):

- Afecta a los valores de Guardrails en los que los niveles personales **solo endurecen** (permisos, ramas protegidas, tamaño de diff, rutas prohibidas, plazo de la cola). No a los valores donde gana el más específico, como el umbral de inactividad del Motor local.
- El daemon guarda el **resumen confirmado** de cada nivel personal (perfil y local). **Todo endurecimiento**, con el comando o a mano, adelanta el confirmado al instante. Sin eso, "endurecer a mano y luego revertir a mano" pasaría sin factor.
- Una edición que **relaja** frente al confirmado no rige: rige la combinación más restrictiva y el estado muestra `personal-relax-pending` con el valor anterior, el nuevo y el archivo de origen. Se confirma con el factor, con el mismo flujo y reto que US-GRD-013. Si una edición endurece y relaja, la parte que endurece rige al instante.
- **Sin factor**, la relajación personal no rige. La vía de escape es desinstalar (D5) y volver a instalar: la confirmación inicial explícita (US-GRD-014) compara contra el registro confirmado, que sobrevive a la desinstalación, y su anuncio dice "relaja los niveles personales" con lo que cambia. Si la máquina tiene el factor, esa re-base lo pide y no cae a D5. Solo un perfil perdido y recreado, sin registro que comparar, adopta el archivo actual, nunca por debajo del suelo, y lo audita.
- **Riesgo residual**: el registro confirmado vive en el almacén del perfil, que cualquier proceso del usuario puede escribir. Mientras corre, el daemon lo tiene en memoria y un cambio externo cuenta como relajación pendiente; forjarlo y matar el daemon deja un hueco auditado, la misma clase que H-04 y el suelo confirmado.
- **Worktree**: se lee del blob commiteado y solo endurece; quitar un endurecimiento del worktree no está protegido por diseño. La garantía es el suelo más los niveles personales.

**Lo que este ADR no cierra**:

- **Nivel de equipo**: el comando de US-GRD-013 solo escribe en el working tree. La relajación rige cuando llega a la rama principal y el humano la confirma (D7, D8), y esa confirmación **todavía no lleva el factor**. Para el equipo, el gate real es D8; R-GRD-10 se cierra con la historia de OQ-GRD-008-3.

### 6. Ubicación en el código

- **Binding por SO**: lo elige SPIKE-GRD-002 (§ 2), compatible con `unsafe_code = "forbid"`.
- **Módulo de presencia del daemon**: detrás de un trait con una implementación por SO (`cfg(target_os)`), alojado en el daemon (`raptor daemon`, ADR-GRP-005). Ningún cliente lo enlaza.
- **Dependencias nuevas** (SEC-GRD-15 y SEC-07 de motor-local):
  - `objc2-local-authentication` en macOS;
  - `windows` en Windows (⚠️ los nombres de las features no están verificados);
  - `zbus_polkit` en Linux.
- **Distribución en Linux** (OQ-GRD-008-6): el `.policy` exige root y no lo puede instalar un canal por usuario (npm o script). **GitRaptor nunca se ejecuta como root.** `raptor doctor` muestra el comando exacto para instalarlo y el de retirarlo, y el usuario los ejecuta. El comando comprueba el hash esperado (constante del binario) antes de copiar y nunca copia desde una ruta que otros puedan escribir. Los paquetes del sistema lo traen hecho. La desinstalación del producto muestra el comando para retirarlo. Una distro con `/usr` de solo lectura queda en fail-closed.
- **Doble de prueba**: una implementación programable (verificado, rechazado, no disponible, tiempo agotado) que **solo se compila en builds de test**, como la variable de sobreescritura del perfil de SEC-06. Ningún entorno ni ninguna bandera la selecciona en release.

### 7. Validación previa: SPIKE de viabilidad

**[SPIKE-GRD-002 — Factor de autenticación del SO invocado desde el daemon en los tres SO](../../requirements/features/guardrails/technical-stories/SPIKE-GRD-002-factor-so-daemon.md)** (creado el 2026-10-04; la parte macOS empieza ya; Linux y Windows: Pendiente: etapa de validación multiplataforma). Debe responder estas preguntas:

1. LocalAuthentication desde el LaunchAgent y desde el daemon arrancado bajo demanda con entorno limpio (SEC-10); el fallo de la primera llamada tras el inicio de sesión.
2. Windows Hello con una ventana transitoria del daemon: primer plano y comportamiento del reconocimiento facial.
3. polkit desde `systemd --user` y desde el arranque bajo demanda: qué agente atiende la petición, sujeto y sesión, y sustitución en el mensaje.
4. Comportamiento de los tres diálogos ante entrada sintética del mismo usuario (`osascript` con y sin Accesibilidad, `SendInput`, `xdotool` en X11).
5. Cancelación por el daemon a los 60 s; binario firmado y sin firmar; si el SO informa del método; cambio rápido de usuario y pantalla bloqueada; agente de polkit impostor y sujeto de polkit.
6. Qué binding cumple `unsafe_code = "forbid"` (§ 2).

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

- ✅ Cierra para US-GRD-013 y US-GRD-015 los cuatro vectores de ADR-GRD-007 § 2 que dicen "No" y el riesgo A-2: la prueba exige un gesto o un secreto que el agente no tiene.
- ✅ Cierra R-GRD-4 para los niveles personales con el trinquete (§ 5), con el riesgo residual del registro confirmado en el perfil.
- ✅ GitRaptor no guarda secretos ni registros de enrolamiento: el estado verificador es del SO.
- ✅ Una sola capacidad del daemon, reutilizable por las demás acciones que relajan y por la Time Machine (TQ-14 b).
- ⚠️ **Disponibilidad desigual**: Windows 10, Windows Hello sin configurar, Linux sin sesión gráfica o sin el `.policy`, los Mac sin sesión gráfica y un daemon arrancado bajo demanda desde SSH quedan sin US-GRD-013 ni US-GRD-015 (fail-closed). Hoy Windows no tiene canal. Sin factor, una relajación personal solo sale desinstalando y reinstalando con D5.
- ⚠️ **Denegación de servicio por fatiga**: el bloqueo por fatiga también impide desinstalar mientras dura, cuando desinstalar adopte el factor (OQ-GRD-008-3).
- ⚠️ **Forzar `unavailable`**: en el modo preferente de la historia de OQ-GRD-008-3, un agente que vuelve el factor no disponible fuerza D5. No empeora el MVP de hoy; consta en R-GRD-3.
- ⚠️ **Aprobación refleja**: un agente puede provocar el diálogo por un vector no detectable y el humano puede aceptarlo sin leer. Lo limitan el texto del diálogo, el anuncio, la ventana posterior, un diálogo a la vez, la limitación por fatiga y la auditoría. Es un riesgo residual.
- ⚠️ **Secreto filtrado**: si el agente conoce la contraseña o el PIN del usuario (p. ej. porque el humano la escribió en una terminal que el agente observa), las variantes con secreto caen. Las que solo admiten biometría no (OQ-GRD-008-5).
- ⚠️ **Linux exige un paso con root** para instalar el `.policy` (OQ-GRD-008-6), y el texto del diálogo no lleva el detalle de la acción.
- ⚠️ **Dependencias nuevas por SO** en la ruta crítica de seguridad (SEC-GRD-15), elegidas por SPIKE-GRD-002 dentro de `unsafe_code = "forbid"`.
- ⚠️ **R-GRD-10 sigue abierto** hasta la historia de OQ-GRD-008-3 (las confirmaciones de D8 adoptan el factor).
- ⚠️ **CI sin humano**: la propiedad real (que el diálogo no se puede completar sin el humano) solo se verifica a mano, por SO, antes de cada release que toque el módulo.

## Validación

Todas las pruebas automáticas usan el doble de prueba y repos y perfiles temporales, nunca el repo de GitRaptor.

1. **Fail-closed**: con el doble en "no disponible", los clientes muestran relajar y aprobar como no disponibles con el motivo; una petición por la CLI o por JSON-RPC directo se rechaza con `factor-unavailable`, no escribe nada y se audita.
2. **Controles previos**: una petición desde un agente simulado (CLI, JSON-RPC directo, pty), desde un descendiente del daemon (`daemon-descendant`) o con un cambio de identidad del llamante se rechaza **sin** invocar el doble (`precheck-denied`). Si la conexión se cierra antes de aplicar, la entrada se anula.
3. **Sin prueba del cliente**: una petición con un campo de "prueba" inventado se trata igual que sin él.
4. **Ligadura**: si el archivo destino o la petición de la cola cambian entre el `verificado` y el cierre de la ventana, no se aplica nada y se audita `factor-stale`.
5. **Un solo uso**: un `verificado` aplica una acción; una segunda aplicación con la misma entrada se rechaza.
6. **Ventana**: tras un `verificado`, otro cliente cancela dentro de la ventana y no se aplica nada.
7. **Plazo de la cola**: una petición que caduca durante el diálogo no se aprueba aunque el doble devuelva `verificado` después.
8. **Concurrencia y fatiga**: dos peticiones a la vez abren un solo diálogo (`factor-busy`); tres resultados no verificados seguidos activan `factor-throttled`.
9. **Vectores** (el doble simula que el humano no acepta): un `tmux send-keys` en la pty del humano que lanza el comando de relajar abre el diálogo, se rechaza y no escribe nada.
10. **Auditoría**: cada caso anterior deja su entrada con método, resultado y ascendencia; el escáner de secretos no encuentra el nonce ni secretos plantados.
11. **MCP**: el catálogo no ofrece estas acciones y el canal las rechaza desde la conexión del MCP.
12. **Trinquete personal**: endurecer a mano adelanta el confirmado; revertirlo a mano queda en `personal-relax-pending` hasta un `verificado`; con el doble en "no disponible" no rige; una edición externa del registro confirmado con el daemon en marcha cuenta como relajación pendiente.
13. **Plazo del daemon**: un `verificado` que llega después de 60 s se descarta.
14. **Build de release**: un gate de CI comprueba que el binario de release no contiene el doble y que ninguna variable ni bandera lo activa.
15. **Manual por SO** (gate de release; lo ejecuta un humano): Touch ID y contraseña desde el LaunchAgent y bajo demanda; arranque bajo demanda desde SSH, fail-closed; Windows sin canal, "no soportado" hasta la etapa multiplataforma; Windows Hello en Windows 11 con PIN y con biometría; polkit en GNOME (Wayland) y KDE con el `.policy`; por SSH sin sesión gráfica, fail-closed; los intentos de entrada sintética de SPIKE-GRD-002 no completan el diálogo.

## Preguntas resueltas (OQ-GRD-008-n)

Las nueve son **Decisión del orquestador (2026-10-04), validada por Arquitecto/PO**, bajo la autonomía que Rene Bonilla delegó en el orquestador. El PO validó las que tocan una garantía al usuario (1, 3, 5, 7 y 8).

| ID | Pregunta | Decisión | Validación |
|---|---|---|---|
| OQ-GRD-008-1 | ¿Qué pasa sin sesión gráfica (SSH, contenedores, WSL sin WSLg)? | **Fail-closed en el MVP**, en Linux y también en un Mac sin sesión gráfica: relajar con el comando y aprobar en la cola no se ofrecen. Endurecer, rechazar y dejar caducar siguen disponibles; relajar por un commit del equipo más la confirmación de D8 sigue con D5. FIDO2 con atestación es la candidata después del MVP; pedir Windows Hello desde WSL no está estudiado | Arquitecto: de acuerdo (añade el Mac sin sesión). PO: de acuerdo, con el ajuste de que endurecer y rechazar nunca exigen el factor |
| OQ-GRD-008-2 | ¿Entra TOTP o un segundo dispositivo en el MVP? | **No**: la semilla la puede leer cualquier proceso del usuario (fuerza 2) | Arquitecto: de acuerdo |
| OQ-GRD-008-3 | ¿Cuándo adoptan el factor desinstalar, la excepción y las confirmaciones de D8? | **En una historia de usuario aparte, después de US-GRD-013 y US-GRD-015**, en modo **preferente**: factor si el SO lo tiene; D5 solo si `unavailable`, con la ausencia en el anuncio y en la auditoría. Rechazo, cancelación, plazo agotado, `busy`, `throttled` o error son rechazo sin caída. Orden: confirmaciones de D8 y desinstalar primero (cierra R-GRD-10), la excepción al final. Nadie pierde la capacidad de desinstalar por no tener factor | Arquitecto: ajuste (solo `unavailable` cae; modo fijo en el binario; es una US del PO). PO: de acuerdo, con el riesgo residual "forzar `unavailable`" en R-GRD-3 |
| OQ-GRD-008-4 | ¿Cuánto vale la prueba y se mantiene la ventana? | **60 s para el diálogo**, impuestos por el daemon, con un contexto nuevo por petición y sin reutilizar desbloqueos. **La ventana de 10 s se mantiene y va después del factor**. En la cola manda lo que venza antes | Arquitecto: de acuerdo (añade el plazo impuesto por el daemon) |
| OQ-GRD-008-5 | En macOS, ¿solo biometría o también contraseña? | **`deviceOwnerAuthentication`**: Touch ID, Apple Watch o la contraseña de sesión, porque muchos Mac de escritorio no tienen Touch ID. El método se audita si el SO lo informa; si no, `not-reported`. Preferir la biometría se decide después | Arquitecto y PO: de acuerdo (el secreto filtrado queda como riesgo residual) |
| OQ-GRD-008-6 | ¿Cómo se instala el `.policy` de polkit? | **GitRaptor nunca corre como root**: `raptor doctor` muestra los comandos de instalar y retirar, que ejecuta el usuario; el comando comprueba el hash fijo en el binario y no copia desde rutas escribibles por otros; cada petición comprueba contenido, propietario root y permisos. Los paquetes del sistema lo traen hecho. Fail-closed hasta entonces | Arquitecto: de acuerdo, con los dos añadidos de integridad. Recomienda revisión del security-expert |
| OQ-GRD-008-7 | ¿Windows 10 o Windows Hello sin configurar? | **Fail-closed** con la guía para configurar el PIN de Windows Hello, sin instrucciones para saltarse la protección. Sin CredUI | Arquitecto y PO: de acuerdo. Hoy Windows no tiene canal |
| OQ-GRD-008-8 | ¿Se cierra R-GRD-4 (relajar un nivel personal editando el archivo)? | **Sí, con el trinquete personal de § 5 (Q-GRD-32)**: endurecer rige al instante y adelanta el confirmado; quitar un endurecimiento queda en `personal-relax-pending` hasta confirmarlo con el factor; sin factor no rige y la vía de escape es reinstalar con D5. Enmienda de ADR-GRD-004, requisito antes de la Dev Spec de US-GRD-013. El worktree queda fuera por diseño | Arquitecto: ajuste (trinquete; riesgo residual del registro; re-base al reinstalar). PO: de acuerdo, con alcance, visibilidad y vía de escape; pide Q-GRD-32 y una historia nueva |
| OQ-GRD-008-9 | ¿Se acepta antes o después de SPIKE-GRD-002? | **Ahora**. SPIKE-GRD-002 gatea las Dev Specs de US-GRD-013 y US-GRD-015 y elige el binding. La parte macOS empieza ya; Linux y Windows: Pendiente: etapa de validación multiplataforma, en fail-closed mientras tanto | Arquitecto: de acuerdo; recomienda que el security-expert revise OQ-6 y el riesgo residual de OQ-8 |

## Enmiendas requeridas en otros artefactos

Se aplicaron con la aceptación (2026-10-04), salvo las marcadas como pendientes.

| Artefacto | Enmienda | Estado |
|---|---|---|
| ADR-GRP-005 § 6 | Filas "Relajar la configuración" y "Aprobar una petición de la cola": el factor es el de ADR-GRD-008. `reserved-action-pending` con el estado "esperando la autenticación del sistema". Validación 7 remite a la de ADR-GRD-008. Enmienda TS-GRP-004, punto 9 (A-2): el factor es el control compensatorio candidato | **Aplicada (2026-10-04)** |
| ADR-GRD-007 § 1, § 2 y Consecuencias | Filas de relajar y aprobar: "factor de ADR-GRD-008". § 2: los vectores se cierran con ADR-GRD-008 en las acciones que lo adoptan. Consecuencias: "requiere un ADR propio" pasa a ADR-GRD-008 | **Aplicada (2026-10-04)** |
| ADR-GRD-004 § 2 | Trinquete de los niveles personales (§ 5, Q-GRD-32), requisito antes de la Dev Spec de US-GRD-013 | **Aplicada (2026-10-04)** |
| ADR-GRP-013 § 1 | La auditoría de los comandos reservados incluye los intentos del factor con método, resultado y momentos (§ 4) | **Aplicada (2026-10-04)** |
| ADR-GRD-006 § 1 | Si el registro de decisiones de la cola anota que la aprobación se hizo con el factor | Pendiente: se decide en la Dev Spec de US-GRD-015 |
| `non-functional-guardrails.md` | SEC-GRD-05 cita ADR-GRD-008; nuevo SEC-GRD-20; riesgos residuales; fila J10 "Nuevo ADR" aplicada | **Aplicada (2026-10-04)** |
| `technical-stories.md` (Guardrails) | Fila de SPIKE-GRD-002 y su archivo; US-GRD-013 y US-GRD-015 esperan a SPIKE-GRD-002 para su Dev Spec | **Aplicada (2026-10-04)** |
| US-GRD-013, US-GRD-015 e índice de historias | Dependencias con ADR-GRD-008, SPIKE-GRD-002 y Q-GRD-32. US-GRD-015 ya no depende del Cockpit: la cola completa va en el daemon y la CLI | **Aplicada (2026-10-04)**. Requisitos Técnicos: en su Dev Spec |
| `context.md` de Guardrails (PO) | Q-GRD-32; R-GRD-3 con el mecanismo y el riesgo "forzar `unavailable`"; R-GRD-4 con Q-GRD-32 | **Aplicada (2026-10-04)** |
| `business-rules.md` de Guardrails y una historia nueva (PO) | Extender la regla de D7 (Q-GRD-21) a los niveles personales e historia "relajación personal pendiente" (US-GRD-013 está en el tope de escenarios); historia de adopción de OQ-GRD-008-3 | Pendiente: PO, con `/aadd-stories` |
| `context.md` del Cockpit | DEP-CKP-8 = US-GRD-015 (cola en el daemon y la CLI, con el factor de ADR-GRD-008) | **Aplicada (2026-10-04)** |
| ADR-TMC-005 (TQ-14 b) | Nota informativa: la presencia verificada por el SO de la Fase 2 puede reutilizar esta capacidad | Pendiente (informativa, Fase 2) |

## Referencias

- **Reglas y riesgos**: BR-AUTH-001, BR-AUTH-004, BR-TIME-001, BR-CONS-001; Q-GRD-19, Q-GRD-21, Q-GRD-22, Q-GRD-23, Q-GRD-24, Q-GRD-32; R-GRD-3, R-GRD-4, R-GRD-10; NFR-03.
- **Otros frentes**: ADR-GRP-005, Enmienda TS-GRP-004 (puntos 1, 2, 5, 7, 8 y 9); DEP-MCP-3, Q-MCP-3, Q-MCP-5; Q-CKP-14, DEP-CKP-8; SPIKE-GRD-002, SPIKE-GRP-001.
- **Decisiones**: D5, D8 y D10 de Rene Bonilla (2026-10-04).
- **Historias**: US-GRD-013 y US-GRD-015 (gate duro); US-GRD-003, US-GRD-006, US-GRD-007 y US-GRD-014 (adopción posterior, OQ-GRD-008-3).
- **ADRs**: ADR-GRD-007 § 1 a § 3 (padre), ADR-GRD-004 § 2 y § 4, ADR-GRD-006 § 1; ADR-GRP-005 § 3 y § 6, ADR-GRP-006, ADR-GRP-012, ADR-GRP-013 § 1; ADR-TMC-005 (SEC-TMC-03, TQ-14).
- **Seguridad**: SEC-GRD-05, SEC-GRD-06, SEC-GRD-12, SEC-GRD-15; SEC-06, SEC-07 y SEC-10 de motor-local; OWASP LLM06.
- **Fuentes externas consultadas (2026-10-04)**: crates.io (`objc2-local-authentication` 0.3.2, `zbus_polkit` 5.1.0, `robius-authentication` 0.3.1); README de `robius-authentication` (requisitos de polkit); Microsoft Learn, `IUserConsentVerifierInterop::RequestVerificationForWindowAsync` (build 22000, HWND); Apple Developer Forums (LocalAuthentication en la sesión de usuario, no en un LaunchDaemon).
