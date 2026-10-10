---
id: SPIKE-GRD-002
title: "Factor de autenticación del SO invocado desde el daemon en los tres SO"
type: spike
status: ready
feature: guardrails
domain: GRP
priority: critical
complexity: medium
created: 2026-10-04
updated: 2026-10-09
related:
  adrs: [ADR-GRD-008, ADR-GRD-007, ADR-GRD-004, ADR-GRP-005, ADR-GRP-002, ADR-GRD-003]
  stories: [US-GRD-013, US-GRD-015, TS-GRP-007]
  specs: []
ado:
  id: null
  url: null
tags: [guardrails, spike, factor-fuera-de-banda, presencia-humana, localauthentication, windows-hello, polkit, entrada-sintetica, fail-closed, unsafe-code, macsys, objc2-local-authentication, tq-14]
---

## SPIKE-GRD-002: Factor de autenticación del SO invocado desde el daemon en los tres SO

> **Estado (2026-10-09)**: **plan de la parte macOS listo y validado por el Arquitecto** (sección "Plan del spike: parte macOS", al final). Falta ejecutar el prototipo y la matriz en el Mac de Rene; no hay resultados todavía. Estado anterior (2026-10-04): sin empezar. **Alcance inmediato: macOS.** Linux y Windows: **Pendiente: etapa de validación multiplataforma** (en Windows, además, el daemon todavía no tiene canal: ADR-GRP-005, Enmienda TS-GRP-004, punto 8). Mientras una plataforma no tenga su parte cerrada, US-GRD-013 y US-GRD-015 quedan en fail-closed en ella (ADR-GRD-008 § 7, OQ-GRD-008-9).

**Valor**: confirmar con pruebas reales que el daemon puede obtener una prueba de presencia del SO que un proceso del mismo usuario no fabrica, antes de que ADR-GRD-008 se convierta en el contrato de las Dev Specs de US-GRD-013 y US-GRD-015.

> Un SPIKE no lleva Dev Spec: su entregable es un Research Brief en `research/SPIKE-GRD-002-resultados.md`. Es un prototipo aislado (`spikes/os-presence-factor/`): un binario mínimo que reproduce las dos formas de arranque del daemon (autoarranque y arranque bajo demanda) y llama al diálogo del SO, sin código del daemon. **Depende de**: nada. **Valida**: ADR-GRD-008 § 2, § 3 y § 6; la enmienda de ADR-GRD-004 (OQ-GRD-008-8) no depende de él. **Bloquea**: la Dev Spec de US-GRD-013 y la de US-GRD-015 (la parte macOS basta para implementarlas con alcance macOS; el release en Linux y Windows espera a la parte pendiente).

### Pregunta

¿Puede el daemon, sin `unsafe` en el código del workspace fuera de las excepciones de ADR-GRP-002 (`winsys` y `macsys`), abrir el diálogo de autenticación del SO desde sus dos formas de arranque, ligarlo a un plazo y a un uso único, y obtener un resultado que ninguna entrada sintética del mismo usuario complete?

### Hipótesis

- **macOS**: LocalAuthentication funciona desde el LaunchAgent y desde el arranque bajo demanda con `setsid()` y entorno limpio, porque la sesión gráfica se hereda del padre y no de la sesión POSIX. Un daemon arrancado desde SSH no tiene sesión gráfica y queda en fail-closed aunque el humano tenga abierta la sesión gráfica.
- **Plazo y reutilización**: el daemon puede cancelar el diálogo a los 60 s, y dos evaluaciones seguidas piden dos gestos.
- **Método**: el SO no informa si se usó Touch ID, el reloj o la contraseña; la auditoría registra "no informado".
- **Entrada sintética**: `osascript` (con y sin el permiso de Accesibilidad), eventos sintéticos de teclado y ratón y `tmux send-keys` en la pty del humano no completan el diálogo; el campo de contraseña activa la entrada segura.
- **`unsafe`**: las llamadas directas de LocalAuthentication y de Windows Hello exigen `unsafe`, que el workspace prohíbe; una dependencia que encapsule el `unsafe` o un crate aislado con una excepción explícita es la única vía.

### Experimento

**macOS (ahora)**

- **Formas de arranque**: autoarranque como LaunchAgent; arranque bajo demanda con `setsid()` y entorno limpio lanzado desde Terminal.app, desde tmux, desde un IDE y desde un proceso hijo de Claude Code; y lanzado desde una sesión SSH con la sesión gráfica abierta. Anotar `canEvaluatePolicy` frente al resultado real de la evaluación en cada caso.
- **Primera llamada**: el fallo "UI activation timed out" tras iniciar sesión; si un reintento lo resuelve y cuánto tarda.
- **Binario**: sin firmar, con firma ad hoc y con la firma de release; con y sin bundle. Si el diálogo muestra el nombre del binario y el texto de la plantilla fija, y cómo se ven los parámetros saneados (SEC-GRD-06).
- **Plazo y uso único**: cancelar desde el daemon a los 60 s y comprobar que el diálogo desaparece; dos evaluaciones seguidas con contextos nuevos y sin periodo de reutilización.
- **Método informado**: qué datos devuelve el SO tras un éxito con Touch ID, con el reloj y con la contraseña.
- **Entrada sintética**: `osascript` con System Events, con y sin Accesibilidad concedida a la terminal; eventos sintéticos de teclado y ratón; `tmux send-keys` en la pty del humano que lanza el comando. Con la contraseña conocida por el atacante, si el campo acepta pulsaciones sintéticas.
- **Estados del equipo**: pantalla bloqueada, salvapantallas, otro usuario en primer plano (cambio rápido de usuario), tapa cerrada con monitor externo (sin Touch ID) y Mac sin Touch ID.
- **Concurrencia**: dos evaluaciones a la vez desde el mismo proceso.
- **Binding**: compilar la llamada con `objc2-local-authentication`, con `robius-authentication` y con un crate aislado con `unsafe`. Comparar la superficie de dependencias (SEC-GRD-15) y cómo encaja cada opción con `unsafe_code = "forbid"`.

**Linux — Pendiente: etapa de validación multiplataforma**

- **Agente**: qué agente de polkit atiende a un daemon bajo `systemd --user` y a uno arrancado bajo demanda, en GNOME (Wayland) y KDE (X11 y Wayland); qué sujeto conviene (el daemon o el proceso cliente, con su identidad) y en qué sesión cae.
- **Agente impostor**: si un proceso del mismo usuario puede registrarse como agente de la sesión y recibir la petición en una terminal.
- **`.policy`**: instalado con el comando que muestra `raptor doctor`; comprobación del contenido, del propietario y de los permisos en cada petición; sustitución de valores en el mensaje con `details`; distribuciones con `/usr` de solo lectura.
- **Sin sesión gráfica**: SSH, contenedor y WSL sin WSLg, en fail-closed.
- **Entrada sintética**: `xdotool` en X11 y su equivalente en Wayland.

**Windows — Pendiente: etapa de validación multiplataforma**

- **Diálogo**: Windows Hello desde la ventana transitoria del daemon; primer plano frente a las reglas de bloqueo del primer plano; con PIN, huella y reconocimiento facial (si el facial completa sin un gesto intencional).
- **Disponibilidad**: Windows 10, Hello sin configurar y sesión remota, en fail-closed.
- **Entrada sintética**: `SendInput` del mismo usuario y nivel de integridad.
- **Binding**: el mismo análisis de `unsafe` que en macOS, con el crate `windows`.

### Criterios de Éxito

- **Formas de arranque**: matriz confirmada de forma de arranque frente a resultado en macOS, lista para la disponibilidad que comprueba el daemon y para el mensaje de `raptor doctor`.
- **Ninguna entrada sintética completa el diálogo** en ningún caso probado. Cada intento queda documentado con su resultado.
- **Plazo, cancelación y uso único** verificados: el diálogo se cancela a los 60 s y cada acción pide un gesto nuevo.
- **Binding elegido** con su encaje en `unsafe_code = "forbid"`, como recomendación para la Dev Spec.
- **Coste**: el diálogo aparece en menos de 2 s tras la petición (⚠️ **ASSUMPTION**: objetivo de UX, sin NFR propio).
- **Vía de fracaso**:
  - Si una entrada sintética completa el diálogo en un SO, esa plataforma queda en fail-closed y se reabre ADR-GRD-008 § 2 para ella.
  - Si el arranque bajo demanda no puede abrir el diálogo, el factor solo está disponible con el autoarranque y `raptor doctor` lo explica; no se reabre el ADR.
  - Si no hay binding sin `unsafe` propio aceptable, se propone un ADR de excepción acotada a un crate aislado y auditado, antes de la Dev Spec.
  - Si el SO no permite cancelar el diálogo a los 60 s, el daemon anula la entrada pendiente a los 60 s y descarta cualquier resultado posterior.

### Time-box

⚠️ **ASSUMPTION**: 4 días para macOS. Linux y Windows, 3 días cada uno cuando llegue la etapa de validación multiplataforma.

## Plan del spike: parte macOS (2026-10-09)

> **Estado del plan**: escrito el 2026-10-09 a partir de la investigación previa y validado por el Arquitecto el mismo día, con los ajustes incorporados (ver § 9, "Validación del plan"). Concreta para macOS las secciones de Hipótesis, Experimento y Criterios de éxito. Si algo de esas secciones contradice este plan, **manda este plan**. Linux y Windows no cambian.

### 1. Investigación previa (fuentes consultadas el 2026-10-09)

Antes de escribir código se revisaron la documentación de Apple, los foros de desarrolladores, el código de los crates y los precedentes. Lo marcado como **extracto** se leyó en el resultado del buscador porque el foro de Apple bloqueó la descarga directa. Es una hipótesis que el prototipo tiene que confirmar.

| # | Hallazgo | Estado | Fuente |
|---|---|---|---|
| F1 | `deviceOwnerAuthentication` usa Touch ID, Apple Watch o la contraseña de la sesión. Con la tapa cerrada o sin sensor queda la contraseña | Verificado | [LAPolicy.deviceOwnerAuthentication](https://developer.apple.com/documentation/localauthentication/lapolicy/deviceownerauthentication) |
| F2 | `invalidate()` cancela la evaluación pendiente, que falla con `appCancel`. Así el daemon puede cerrar el diálogo a los 60 s | Verificado (en la documentación; falta medir si el diálogo desaparece) | [LAContext.invalidate()](https://developer.apple.com/documentation/localauthentication/lacontext/invalidate()) |
| F3 | `touchIDAuthenticationAllowableReuseDuration` vale 0 por defecto. Con un valor mayor que 0, un desbloqueo reciente aprueba **sin diálogo**. Además, un `LAContext` reutilizado aprueba sin preguntar | Verificado | [Apple](https://developer.apple.com/documentation/localauthentication/lacontext/touchidauthenticationallowablereuseduration), comentario en el código de `robius-authentication` |
| F4 | `evaluatePolicy` solo devuelve sí o no y un error. Ninguna API dice si se usó Touch ID, el reloj o la contraseña; `evaluatedPolicyDomainState` sirve para otra cosa | No encontrado (refuerza `not-reported`) | [evaluatedPolicyDomainState](https://developer.apple.com/documentation/localauthentication/lacontext/evaluatedpolicydomainstate) |
| F5 | `LAEnvironment` y `LADomainState` (macOS 15) informan de los mecanismos disponibles (contraseña configurada, biometría, companion). Útiles para `raptor doctor`. `LARight` no aporta seguridad frente a `LAContext` para un solo sí o no | Verificado (APIs); la conclusión es nuestra | [LAEnvironment](https://developer.apple.com/documentation/localauthentication/laenvironment), [LARight](https://developer.apple.com/documentation/localauthentication/laright) |
| F6 | Desde un LaunchAgent con la pantalla bloqueada falla con `-1004` ("not in a console session"). La primera llamada tras iniciar sesión puede fallar con `-1000` ("UI activation timed out") y un reintento lo resuelve | Extracto | Foros de Apple [672162](https://developer.apple.com/forums/thread/672162) y [131437](https://developer.apple.com/forums/thread/131437) |
| F7 | Apple DTS dice que LocalAuthentication es "un framework de apps". En la práctica hay CLI sin bundle que lo usan (`tidcli`). El nombre del diálogo lo saca `coreauthd` del bundle del PID; sin bundle, sale sin nombre o con el del binario. Un `Info.plist` incrustado con `-sectcreate __TEXT __info_plist` es la vía sin `.app` | Extracto, sin medir | Foros [129480](https://developer.apple.com/forums/thread/129480) y [740600](https://developer.apple.com/forums/thread/740600); [tidcli](https://github.com/singe/tidcli) |
| F8 | No se encontró que LocalAuthentication exija firma, notarización ni entitlements en macOS (`NSFaceIDUsageDescription` es solo de iOS). Indicio en una beta de macOS 27 (probablemente ya publicado a la fecha del spike): `-1007 "Caller is not Apple signed"` con la vista embebida | Sin documentación en contra; el indicio de macOS 27 es un extracto. Cada resultado del spike registra la versión exacta (`sw_vers`) | [NSFaceIDUsageDescription](https://developer.apple.com/documentation/bundleresources/information-property-list/nsfaceidusagedescription), [foros, tag LocalAuthentication](https://developer.apple.com/forums/tags/localauthentication) |
| F9 | **Authorization Services** sigue vigente, pero su plug-in solo ofrece Touch ID a ejecutables firmados por Apple: a terceros solo les pide la contraseña. Cualquier proceso del usuario puede registrar antes el derecho (`config.add.` es `allow`). Refuerza el descarte de ADR-GRD-008 § 2 | Extracto | Foro [106386](https://developer.apple.com/forums/thread/106386), [TN2095](https://developer.apple.com/library/archive/technotes/tn2095/_index.html) |
| F10 | El sensor de Touch ID habla con el Secure Enclave por un canal cifrado con una clave del sensor. Un proceso del usuario no puede simular una huella | Verificado | [Apple Platform Security](https://support.apple.com/guide/security/touch-id-and-face-id-security-sec067eb0c9e/web) |
| F11 | Desde Mojave, macOS rechaza los clics sintéticos en los avisos de seguridad, con una lista blanca que ya se eludió en 2019. No se encontró documentación de que el diálogo de `coreauthd` filtre las pulsaciones sintéticas ni de que active la entrada segura (que protege contra **leer**, no contra **inyectar**) | En parte verificado | [TechCrunch 2019](https://techcrunch.com/2019/06/03/macos-security-flaw-synthetic-clicks/), [TN2150](https://developer.apple.com/library/archive/technotes/tn2150/_index.html) |
| F12 | Los diálogos falsos de credenciales con `osascript` son una técnica conocida: roban la contraseña, pero no aprueban nada en el daemon | Verificado | [Embrace The Red](https://embracethered.com/blog/posts/2021/spoofing-credential-dialogs/), [Elastic](https://www.elastic.co/guide/en/security/current/prompt-for-credentials-with-osascript.html) |
| F13 | SSH crea un espacio de nombres bootstrap sin sesión gráfica. `pam_tid` falla dentro de tmux cuando el servidor de tmux quedó fuera de la sesión Aqua (por eso existe `pam_reattach`). **Que `setsid()` conserve el audit session y el bootstrap de la sesión gráfica no está verificado**: son atributos de Mach y BSM, no de la sesión POSIX, así que es plausible | Verificado en parte (TN2083 es antiguo); `setsid()`, sin verificar | [TN2083](https://developer.apple.com/library/archive/technotes/tn2083/_index.html), [pam_reattach](https://github.com/fabianishere/pam_reattach) |
| F14 | `objc2-local-authentication` 0.3.2 cubre `LAContext`, `LARight`, `LADomainState` y `LAEnvironment`, pero `evaluatePolicy_localizedReason_reply`, `invalidate` y el resto son `pub unsafe fn`, con callbacks `block2` | Verificado | [docs.rs](https://docs.rs/objc2-local-authentication/latest/objc2_local_authentication/struct.LAContext.html) |
| F15 | `robius-authentication` 0.3.1 (MIT) ofrece una API sin `unsafe` (`blocking_authenticate`), pero no expone `invalidate()`, tiene la variante `async` desactivada y arrastra `zbus` y polkit en Linux y Windows Hello en Windows | Verificado | [Cargo.toml de robius](https://github.com/project-robius/robius/blob/main/crates/authentication/Cargo.toml) |
| F16 | Precedentes: `sudo` con `pam_tid` llama a Touch ID desde la CLI, sin helper (y falla fuera de la sesión Aqua). 1Password CLI delega en su app de escritorio. Secretive ata el factor a una clave del Secure Enclave (`SecAccessControl` con `.userPresence`): el factor produce una firma, no un booleano | Verificado | [pam_reattach](https://github.com/fabianishere/pam_reattach), [1Password](https://developer.1password.com/docs/cli/app-integration-security/), [Secretive](https://github.com/maxgoedjen/secretive) |

**Lo que cambia frente a ADR-GRD-008 § 2 y § 6**: nada de la decisión. Se confirma LocalAuthentication y el descarte de Authorization Services (F9). El binding ya tiene sitio sin ADR nuevo (§ 4 de este plan).

### 2. Hipótesis del plan (macOS)

| ID | Hipótesis | Si es falsa |
|---|---|---|
| H1 | Un binario sin `.app` ni `NSApplication`, que espera la respuesta en un canal, abre el diálogo de `deviceOwnerAuthentication` desde el LaunchAgent (dominio `gui/<uid>`). La respuesta llega en un hilo del framework sin run loop propio; una respuesta que llega después del plazo, con el canal ya cerrado, no causa comportamiento indefinido (vida del bloque de `block2`); y se sabe si `LAContext` es `Send` y `Sync` | Plan B: helper `.app` mínimo lanzado por el daemon (§ 3.3). Si lo que falla es la seguridad de memoria, se corrige en el diseño del módulo `ffi_*` |
| H2 | El arranque bajo demanda conserva el audit session y el bootstrap de la sesión gráfica cuando lo lanza un proceso de esa sesión: Terminal.app, un IDE, Claude Code, `raptor-mcp` hijo de Claude Code o un cliente de tmux cuyo servidor nació en la sesión. En el producto, el cliente lanza el daemon con `env_clear()` (`crates/core/src/client.rs`) y es el propio daemon el que llama a `setsid()` al arrancar (`crates/core/src/daemon/mod.rs`) | El factor solo existe con el autoarranque; `raptor doctor` lo explica (vía de fracaso ya prevista) |
| H3 | Desde SSH, desde un servidor de tmux nacido por SSH y con la pantalla bloqueada, el resultado es "no disponible" (`-1004` u otro), nunca un éxito | ADR-GRD-008 fija fail-closed para un daemon arrancado desde SSH. Si aun así el diálogo se abre en la pantalla del humano, **el daemon no amplía la disponibilidad por su cuenta**: decide con los datos de la sesión (`SessionGetInfo`: `sessionIsRemote`, `sessionHasGraphicAccess`; `launchctl managername`) y no solo con `canEvaluatePolicy` (H4). El caso se documenta |
| H4 | `canEvaluatePolicy` puede decir "sí" donde `evaluatePolicy` luego falla (p. ej. pantalla bloqueada). La disponibilidad que muestra el daemon no basta, y el resultado real manda | Si coinciden siempre, la comprobación previa basta para `raptor doctor` |
| H5 | `invalidate()` cierra el diálogo en pantalla y la respuesta llega como `appCancel` | Vía de fracaso ya prevista: el daemon anula a los 60 s y descarta el resultado tardío |
| H6 | Con un `LAContext` nuevo por petición y la reutilización a 0, dos evaluaciones seguidas piden dos gestos, también justo después de desbloquear el Mac con Touch ID | Fallo de diseño: reabrir ADR-GRD-008 § 3 (uso único) |
| H7 | El SO no informa del método: la auditoría registra `not-reported` | Si `LADomainState` u otra vía lo informa sin coste, se audita el método |
| H8 | El diálogo muestra el nombre que fija un `Info.plist` incrustado en el binario, y el texto de la plantilla con los parámetros saneados (SEC-GRD-06). **El nombre es UX, no seguridad**: un agente puede copiar el `Info.plist` | Primero, "GitRaptor" va en el texto de la plantilla. El fallo de H8 **no** activa por sí solo el plan B |
| H9 | Ninguna entrada sintética del mismo usuario completa el diálogo **sin el secreto**: `osascript` con System Events (con y sin Accesibilidad), `CGEventPost` y `tmux send-keys` | Fallo de seguridad: macOS queda en fail-closed y se reabre ADR-GRD-008 § 2 para macOS |
| H10 | **Con el secreto conocido** (el atacante sabe la contraseña), las pulsaciones sintéticas sí completan el campo de contraseña. Es el riesgo residual "secreto filtrado" de ADR-GRD-008 | Si el campo las rechaza, el riesgo residual baja y se anota |
| H11 | El binding va en `crates/macsys`, sobre `objc2-local-authentication`, con el `unsafe` en un módulo privado `ffi_*`, sin ADR nuevo | Si no cabe (dependencias o reglas de `macsys`), se propone el ADR de excepción acotada de la vía de fracaso |

### 3. Prototipo mínimo (`spikes/os-presence-factor/`)

Workspace propio (`[workspace]` vacío, como `spikes/watcher-viability/`), fuera del workspace del producto, así que el `unsafe` del prototipo no toca `unsafe_code = "forbid"`. No enlaza ningún crate de GitRaptor ni usa el perfil real. Un solo binario, `presence-probe`:

1. **`presence-probe evaluate --reason <texto> [--timeout 60] [--policy owner|biometrics-or-watch]`**: crea un `LAContext` nuevo, fija la reutilización a 0, llama a `canEvaluatePolicy`, después a `evaluatePolicy` y espera la respuesta en un canal. A los `--timeout` segundos llama a `invalidate()`. Imprime una línea JSON: la disponibilidad previa, el resultado (`verified`, `denied`, `cancelled`, `timeout` o `unavailable`), el código de `LAError`, la latencia hasta el diálogo y hasta el resultado, el `LADomainState`, el audit session id (`getaudit_addr`), `SessionGetInfo` (`sessionIsRemote`, `sessionHasGraphicAccess`), `launchctl managername` (`launchctl procinfo` exige root y no se usa), el `sid` POSIX, el hilo en que llegó la respuesta y la versión de macOS (`sw_vers`).
2. **`presence-probe serve`**: modo daemon. Escucha en un socket Unix del directorio temporal y ejecuta `evaluate` por cada petición, con un diálogo a la vez (la segunda petición recibe `busy`). Reproduce las dos formas de arranque:
   - **LaunchAgent**: un `.plist` temporal con `launchctl bootstrap gui/<uid>`, igual que `crates/core/src/autostart.rs` (sin `LimitLoadToSessionType`, como hoy), y una variante con `LimitLoadToSessionType=Aqua`;
   - **bajo demanda**: el mismo orden que el producto. El cliente lanza `serve` con el entorno vaciado (`env_clear()` más las variables mínimas) y es `serve` quien llama a `setsid()` al arrancar (Enmienda TS-GRP-004, punto 5; `crates/core/src/client.rs` y `crates/core/src/daemon/mod.rs`). No hay un lanzador intermedio con `fork`.
3. **Variantes de empaquetado**: sin firmar, con firma ad hoc y con hardened runtime, sin `Info.plist` y con `Info.plist` incrustado (`-sectcreate`). La firma Developer ID se prueba solo si las otras fallan. Si fallan H1 o R5, plan B: un `PresenceProbe.app` mínimo que hace la llamada y devuelve el resultado al daemon por su pipe de salida. **El plan B cambia el modelo de confianza**: si el helper está en una ruta que el usuario puede escribir, un agente lo sustituye por uno que responde `verified`, sin matar el daemon ni perder el registro en memoria. Por eso exige que el daemon verifique la firma del helper (cdhash o Team ID fijados en el binario) en cada lanzamiento, y una enmienda de ADR-GRD-008 § 1.3 y § 6 antes de la Dev Spec.
4. **Comparación de bindings**: la llamada con `objc2-local-authentication`, envuelta como la envolvería `macsys`. De `robius-authentication` solo se lee la API y se mide el árbol de dependencias (`cargo tree`, SEC-GRD-15), sin integrarlo.
5. **Ataques** (`attacks/`, scripts de shell): `osascript` con System Events (`keystroke` y `click` sobre el diálogo), un `CGEventPost` mínimo y `tmux send-keys` en la pty que lanzó la petición. Cada ataque se lanza con y sin el permiso de Accesibilidad concedido a la terminal. La variante con la contraseña conocida (H10) se ejecuta **con la sesión gráfica iniciada en una cuenta de prueba local** creada para el spike, sin Touch ID enrolado (el único camino es la contraseña), porque `deviceOwnerAuthentication` autentica al usuario de la sesión. Nunca con la contraseña de Rene.
6. **Robo de foco**: el probe abre el diálogo mientras un humano escribe en la terminal (por ejemplo, la contraseña de un `sudo` que el agente le pidió), para medir si el diálogo toma el foco del teclado y recibe las pulsaciones reales y el Enter.

Los resultados, las capturas del diálogo y las salidas JSON van a `spikes/os-presence-factor/results/` y el informe a `research/SPIKE-GRD-002-resultados.md`.

### 4. Encaje con el diseño existente

**Binding y `unsafe` (ADR-GRD-008 § 2 y § 6; ADR-GRP-002, Enmienda 2026-10-07)**: ADR-GRD-008 pedía, para un crate aislado, "una excepción de `unsafe` aprobada en un ADR propio". Esa excepción ya existe: `crates/macsys` es una de las dos excepciones de ADR-GRP-002, con `unsafe` solo en módulos privados `ffi_*`, un bloque por llamada con su `SAFETY` y la función de fitness `crates/winsys/tests/unsafe_boundary.rs`. Ya aloja tres superficies (`ffi_procargs`, `ffi_kinfo` y `ffi_fsevents`). **Decisión del orquestador (2026-10-09), validada por Arquitecto**: el binding de producción va en `macsys`, como un módulo público `presence` sobre un `ffi_local_auth` privado, sobre `objc2-local-authentication`. La Enmienda 2026-10-07 de ADR-GRP-002 cuenta como el "ADR propio" que pide ADR-GRD-008 § 2, y no hace falta un ADR nuevo. **El "Registro" en ADR-GRP-002 lo escribe la Dev Spec, no el spike**, y debe fijar:
  - **Una feature `presence`** en `macsys` y en `core`, que solo activa `apps/cli`, con un gate parecido a `tm_chaos_gate` que compruebe que `raptor-mcp` no la tiene. Sin ella, `raptor-mcp` enlazaría `LocalAuthentication.framework` (porque `apps/mcp` depende de `core` y `core` de `macsys`), en contra de ADR-GRD-008 § 6 ("ningún cliente lo enlaza").
  - **La familia `objc2` con versión fijada (`=`)**, solo bajo `cfg(target_os = "macos")`. Es la primera dependencia de terceros con `unsafe` en `macsys`.
  - **Revisión obligatoria del security-expert** antes del merge.

  Frente a `robius-authentication`, el argumento fuerte no es el plazo, que ya lo impone el daemon (ADR-GRD-008 § 3). Es que, sin `invalidate()`, un diálogo viejo puede quedar en pantalla mientras el daemon abre el siguiente, y eso rompe "un diálogo a la vez". A eso se suman la reutilización, que debe quedar en 0 de forma explícita, y la superficie de dependencias de SEC-GRD-15 (`zbus` y polkit). `robius` queda como referencia. Si el prototipo muestra que `macsys` no puede alojarlo, se aplica la vía de fracaso (ADR de excepción acotada).

**`authz` y `TERMINAL_PROOF` (ADR-GRP-005 § 6, DS-TS-GRP-004 § 9, TQ-14)**: el factor **se suma** a `check_reserved`, no lo sustituye.
- `check_reserved` (`crates/core/src/channel/authz.rs`) corre primero. Si rechaza, no hay diálogo (`precheck-denied`, ADR-GRD-008 § 3.1). `TERMINAL_PROOF` sigue diciendo si la plataforma tiene prueba de terminal, que se comprueba al compilar.
- La presencia del SO es otra pregunta y se responde **en tiempo de ejecución**: la sesión gráfica puede estar o no, y la pantalla bloqueada o no. Por eso no se modela como una constante gemela de `TERMINAL_PROOF`. Será un trait del módulo de presencia del daemon (ADR-GRD-008 § 6) con una implementación macOS sobre `macsys::presence` y "no disponible" en Linux y Windows hasta su etapa. El trait se **inyecta** en el daemon, igual que `Checks` recibe `terminal_proof`, no como estado global. El doble de prueba sigue el patrón de la feature `chaos`: nunca en `default` y con un gate de CI (ADR-GRD-008, Validación 14).
- **Reutilización prevista**: TS-GRP-007 (Windows Hello para los comandos reservados de alto riesgo, que cierra M-01) y TQ-14 b (confirmar trabajo ajeno en la Time Machine, Fase 2) consumen el mismo trait. Este spike no los implementa, pero el plan evita decidir nada que los impida: la interfaz recibe el texto de la plantilla y devuelve un resultado; no sabe de qué acción se trata.

**Motor de decisión (ADR-GRD-003)**: no es una decisión nueva; es lo que ya dice ADR-GRD-008 § 3, recordado aquí para no adelantar la Dev Spec de US-GRD-015. El factor **no entra** en la función pura de `crates/policy` ni en el contrato `Decision`. La evaluación sigue devolviendo permitir, pedir confirmación o denegar. "Pedir confirmación" se trata como denegar hasta que exista la cola (S-GRD-9). Con US-GRD-015, aprobar una entrada de la cola es un comando reservado del daemon que pasa por `check_reserved` y después por el factor. La evaluación no cambia y el canal no lleva ningún campo de prueba (ADR-GRD-008 § 3).

### 5. Matriz de experimentos (macOS)

Cada celda anota la disponibilidad previa, el resultado, el código de error, la latencia y una captura del diálogo.

| ID | Experimento | Hipótesis |
|---|---|---|
| E1 | `evaluate` directo desde Terminal.app: Touch ID, contraseña y Apple Watch (si hay); denegar; cancelar | H1, H7, H8 |
| E2 | `serve` como LaunchAgent (con y sin `LimitLoadToSessionType=Aqua`), petición desde Terminal.app | H1 |
| E3 | `serve` bajo demanda (orden del producto) lanzado desde Terminal.app, tmux (servidor nacido en la sesión gráfica), la terminal de un IDE, un hijo de Claude Code y **un `raptor-mcp` simulado hijo de Claude Code** (el camino real más frecuente). Se compara el audit session id con el de la sesión gráfica | H2 |
| E4 | `serve` bajo demanda lanzado desde SSH con la sesión gráfica abierta, y desde un servidor de tmux nacido por SSH | H2, H3 |
| E5 | Estados del equipo: pantalla bloqueada, salvapantallas, cambio rápido de usuario, tapa cerrada con monitor externo, Mac sin Touch ID (o Touch ID desactivado) | H3, H4 |
| E6 | Primera llamada tras iniciar sesión (`-1000`): frecuencia, si un reintento lo resuelve y cuánto tarda | F6 |
| E7 | Cancelación a los 60 s con `invalidate()`; una respuesta que llega con el canal ya cerrado; dos evaluaciones seguidas; una evaluación justo después de desbloquear con Touch ID; matar `serve` con el diálogo abierto (que no quede huérfano en pantalla) | H1, H5, H6 |
| E8 | Dos peticiones a la vez contra `serve` (`busy`) y dos `LAContext` a la vez en el mismo proceso | ADR-GRD-008 § 3 (concurrencia) |
| E9 | Empaquetado: sin firmar, ad hoc, hardened runtime, con y sin `Info.plist`; qué nombre muestra el diálogo, cómo se ven los parámetros saneados y si un motivo largo se trunca y oculta el repo o el valor | H1, H8, F8 |
| E10 | Entrada sintética sin el secreto: `osascript` (System Events) y `CGEventPost`, con y sin Accesibilidad; `tmux send-keys` en la pty del humano | H9 |
| E11 | Entrada sintética con el secreto conocido, con la sesión gráfica iniciada en la cuenta de prueba local (sin Touch ID) | H10 |
| E13 | Robo de foco: el diálogo se abre mientras el humano escribe en la terminal (p. ej. la contraseña de un `sudo`); ¿toma el foco y recibe las pulsaciones reales y el Enter? | R9 |
| E12 | Binding: la llamada con `objc2-local-authentication` (cancelación, reutilización, hilos, `Send`/`Sync`); de `robius-authentication`, lectura de la API y `cargo tree` | H1, H11 |

### 6. Criterios de éxito (macOS)

El spike se cierra cuando **todos** se cumplen. Cada uno sale de la matriz, no de una opinión. Un criterio exige una **decisión con evidencia**, no un resultado concreto: si una hipótesis se refuta, el criterio se cumple activando su vía de fracaso.

1. **Matriz de arranque**: E2 a E5 completos, o con cada celda que no se pudo probar marcada "no verificado" y su motivo (R7), con el resultado y el código de error de cada forma de arranque y estado. De ahí salen la comprobación de disponibilidad del daemon y el texto de `raptor doctor`.
2. **H9 decidida con evidencia**: cada intento de E10 queda con su resultado y su captura. Si alguno completa el diálogo sin el secreto, se activa la vía de fracaso (macOS en fail-closed y se reabre ADR-GRD-008 § 2 para macOS).
3. **H10 y robo de foco medidos** (E11 y E13): se sabe si el secreto conocido más pulsaciones sintéticas completa el diálogo, y si el diálogo puede quedarse con pulsaciones que el humano escribía en la terminal. Los riesgos residuales quedan escritos con esos datos.
4. **Plazo y uso único**: E7 muestra que el diálogo se cancela a los 60 s (o se aplica la vía de fracaso) y que cada evaluación pide un gesto nuevo, también tras un desbloqueo reciente.
5. **Método informado**: E1 dice si hay alguna vía para saber el método. Si no, la Dev Spec fija `not-reported`.
6. **Binding**: E12 confirma `macsys` sobre `objc2-local-authentication` (o activa la vía de fracaso) y deja escrita la superficie `unsafe` exacta: llamadas, bloques y `SAFETY`, el hilo de la respuesta, si hace falta run loop, la vida del bloque ante una respuesta tardía y `Send`/`Sync` de `LAContext`.
7. **Coste**: el diálogo aparece en menos de 2 s en E1 a E3 (⚠️ **ASSUMPTION**, igual que arriba).
8. **Empaquetado**: E9 dice qué necesita el binario de release (firma, hardened runtime, `Info.plist`) para que el diálogo muestre un nombre legible. **El nombre es UX, no seguridad**. Incrustar un `Info.plist` en `raptor` cambia su identidad ante TCC (los permisos del SO), así que entra como **enmienda de ADR-GRP-014**, propuesta por el spike y no aplicada sin validarla.

**Entregables**: `research/SPIKE-GRD-002-resultados.md` con el veredicto por hipótesis, la matriz y las recomendaciones para las Dev Specs de US-GRD-013 y US-GRD-015. El prototipo queda en `spikes/os-presence-factor/`. Si una vía de fracaso se activa, se proponen las enmiendas de ADR-GRD-008 o ADR-GRP-002 (no se aplican sin validarlas).

### 7. Riesgos

| ID | Riesgo | Mitigación o tratamiento |
|---|---|---|
| R1 | **El agente simula al humano** provocando el diálogo por un vector no detectable (A-2, `setsid`, código plantado) y el humano lo aprueba por costumbre. Variante: el agente abre desde su propio proceso diálogos idénticos (mismo nombre con un `Info.plist` copiado, mismo texto) para acostumbrar al humano; el bloqueo por fatiga del daemon no los cuenta | Lo cubre ADR-GRD-008 (texto de la plantilla con la acción, el repo y el valor; anuncio en todos los clientes; ventana de 10 s después del factor; un diálogo a la vez; bloqueo por fatiga; auditoría). Los diálogos del agente no aprueban nada en el daemon: el riesgo es solo la costumbre, y queda como residual. El spike mide si el texto del diálogo deja ver la acción (E9) |
| R2 | **El agente conoce la contraseña** (la vio en una terminal, la robó con un diálogo falso, F12) y la escribe con pulsaciones sintéticas | Riesgo residual "secreto filtrado" de ADR-GRD-008. E11 lo mide. Si se confirma, la Dev Spec propone al PO la política solo biométrica o con el reloj (`deviceOwnerAuthenticationWithBiometricsOrWatch`) como opción **que solo endurece**, sin quitar la contraseña por defecto (OQ-GRD-008-5) |
| R3 | **El agente sustituye o envenena el daemon**: un binario falso en el socket, otro binario en el `.plist` del LaunchAgent, o una dylib inyectada sin hardened runtime. Un daemon falso no necesita el factor para escribir la configuración | Fuera del alcance del factor: es la integridad del daemon y del perfil, el mismo riesgo residual que H-04 y el registro confirmado (ADR-GRD-004, ADR-GRD-008 § 5). El spike solo anota si el hardened runtime cambia algo para LocalAuthentication (E9) y lo deja como dato para ADR-GRP-014 |
| R4 | **`setsid()` no conserva la sesión gráfica** (H2 falsa) | El factor solo con el autoarranque; `raptor doctor` lo explica. No se reabre el ADR |
| R5 | **Apple endurece LocalAuthentication** (indicio de `-1007` en macOS 27, F8) y exige bundle o firma de Apple | E9 lo prueba con la versión de macOS de la máquina (`sw_vers`). El plan B es el helper `.app` firmado, con la verificación de su firma y la enmienda de ADR-GRD-008 (§ 3). Si un día solo vale la firma de Apple, macOS queda en fail-closed (OQ-GRD-008-9) |
| R6 | **El prototipo toca el perfil o la sesión real** | Ninguna llamada a GitRaptor; LaunchAgent con una etiqueta propia del spike (`dev.gitraptor.spike.presence`) que se retira al terminar (`launchctl bootout`); los ataques de E11 usan una cuenta de prueba local, nunca la contraseña de Rene |
| R7 | **Pruebas que exigen un humano** (CI sin humano, ADR-GRD-008, Consecuencias) | La matriz se ejecuta a mano en el Mac de Rene, con el guion de `results/`. Lo que no se pueda probar (p. ej. Apple Watch sin reloj emparejado, un Mac sin Touch ID) se marca "no verificado" en el informe, sin inventar el resultado |
| R8 | **El time-box no alcanza** | Orden para los 4 días. Día 1: E1, E7 y el binding con `objc2` (E12). Día 2: E2, E3 y E4. Día 3: E10, E11 y E13. Día 4: E5, E9, E6 y E8, la lectura de `robius` y el informe (medio día). E7 va el primer día porque decide H6: si es falsa, se reabre el uso único de ADR-GRD-008 § 3. E4 es el gate manual de la Validación 15 de ADR-GRD-008 |
| R9 | **Robo de foco**: el agente provoca el diálogo justo cuando el humano escribe en la terminal (p. ej. la contraseña de un `sudo` que el agente le pidió) y el diálogo recibe las pulsaciones reales y el Enter. Es el vector más realista de "el agente simula al humano" con la contraseña | E13 lo mide. Si se confirma, la Dev Spec lo trata: retardo antes de aceptar la entrada, la política solo biométrica de R2 como opción que endurece, o el texto del anuncio. Si ninguna medida basta, se documenta como riesgo residual con su dato |

### 8. Decisiones de este plan

- **Decisión del orquestador (2026-10-09), validada por Arquitecto**: el binding de producción va en `crates/macsys` sobre `objc2-local-authentication`, como una superficie más de la excepción de ADR-GRP-002, sin ADR nuevo. El Registro de ADR-GRP-002 lo escribe la Dev Spec, con la feature `presence` que solo activa `apps/cli`, la versión de `objc2` fijada y la revisión del security-expert (§ 4).
- **Decisión del orquestador (2026-10-09), validada por Arquitecto**: la presencia del SO se comprueba en tiempo de ejecución, detrás del trait de ADR-GRD-008 § 6, que se inyecta en el daemon. No es una constante como `TERMINAL_PROOF`. El factor va después de `check_reserved` (§ 4).
- **Decisión del orquestador (2026-10-09), validada por Arquitecto**: el prototipo hace la llamada en el propio proceso daemon, sin `.app`. El helper `.app` es el plan B solo si fallan H1 o R5, y exige verificar su firma y enmendar ADR-GRD-008 antes de la Dev Spec (§ 3).
- **Decisión del orquestador (2026-10-09), validada por Arquitecto**: atar el factor a una clave del Secure Enclave (el patrón de Secretive, F16) queda **fuera** de este spike y del MVP. No cambia la propiedad que exige ADR-GRD-008 § 1, porque la prueba no sale del daemon, y su coste es alto: exige entitlements de llavero, es decir, una app firmada con perfil de aprovisionamiento. Se anota como mejora candidata.

### 9. Validación del plan

**Arquitecto (`nassa-architect:architect`, 2026-10-09)**. Veredicto: plan sólido, con ajustes antes de ejecutarlo. Las cinco decisiones, de acuerdo; cuatro con ajuste. Todos los ajustes obligatorios están incorporados:

| Ajuste | Dónde quedó |
|---|---|
| D1: con `presence` en `macsys`, `raptor-mcp` enlazaría LocalAuthentication; feature `presence` solo en `apps/cli` con un gate, `objc2` fijado, revisión del security-expert y Registro escrito por la Dev Spec. El argumento fuerte contra `robius` es "un diálogo a la vez", no el plazo | § 4 y § 8 |
| D2: el trait se inyecta; el doble sigue el patrón de `chaos` con un gate de CI | § 4 |
| D3: el fallo de H8 no activa el plan B; el plan B exige verificar la firma del helper y enmendar ADR-GRD-008 | § 2 (H8), § 3 y § 8 |
| D4: motivo de coste (entitlements de llavero) | § 8 |
| D5: es una nota de ADR-GRD-008 § 3, no una decisión nueva | § 4 |
| La validación no puede declararse antes de hacerse: se añade esta sección | § 9 |
| R8 dejaba fuera E4 y E7: orden por días | § 7 (R8) |
| Los criterios deben exigir una decisión con evidencia, no un resultado; celdas "no verificado" admitidas | § 6 |
| E11 con la sesión gráfica de la cuenta de prueba | § 3 y § 5 |
| H3: el daemon no amplía la disponibilidad por su cuenta; `SessionGetInfo` y `launchctl managername` | § 2 (H3) y § 3 |
| Robo de foco (riesgo y experimento) | § 3, § 5 (E13) y § 7 (R9) |
| Seguridad de memoria del binding (hilo, run loop, vida del bloque, `Send`/`Sync`) | § 2 (H1), § 5 (E7, E12) y § 6 |
| Errores factuales: `launchctl procinfo` exige root; el daemon hace `setsid()` y el cliente `env_clear()`; registrar `sw_vers` | § 1 (F8), § 2 (H2) y § 3 |

Opcionales incorporados: los diálogos idénticos del agente (R1), el motivo largo truncado (E9), matar el daemon con el diálogo abierto (E7), `raptor-mcp` hijo de Claude Code (E3), la enmienda de ADR-GRP-014 por el `Info.plist` (criterio 8) y la Pregunta corregida ("fuera de las excepciones de ADR-GRP-002"). No incorporado: quitar la variante `LimitLoadToSessionType=Aqua` de E2. El Arquitecto la considera de poco valor, pero cuesta minutos y confirma un supuesto.

**Sin ADR nuevo**: solo hace falta si se activa el plan B (enmienda de ADR-GRD-008). Lo demás es un Registro en ADR-GRP-002 y una enmienda de ADR-GRP-014, que propondrán la Dev Spec y el informe del spike.
