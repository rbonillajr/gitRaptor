---
id: ADR-GRP-017
title: "App de escritorio como cliente del daemon: frontera de seguridad y presencia humana sin terminal"
type: adr
status: proposed
date: 2026-10-28
created: 2026-10-28
updated: 2026-10-28
deciders: [Rene Bonilla, Orquestador (autonomía delegada por Rene Bonilla), Arquitecto, security-expert]
domain: GRP
feature: desktop
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-003, ADR-GRP-004, ADR-GRP-005, ADR-GRP-006, ADR-GRP-009, ADR-GRP-014, ADR-GRP-016, ADR-MCP-001, ADR-CKP-002, ADR-CKP-003, ADR-GRD-007, ADR-GRD-008, ADR-TMC-005, SPIKE-GRD-002, TS-GRP-004, TS-GRP-007]
tags: [desktop, tauri, gui, canal-local, presencia-humana, factor-del-so, firma-de-codigo, comandos-reservados, capa-cockpit, tq-14, a-2, nfr-01, nfr-02, nfr-03, d7, br-27]
changelog:
  - 2026-10-28: creado (D7, app de escritorio en el MVP). Decisión del orquestador (2026-10-28), validada por Arquitecto.
  - 2026-10-28: revisión del security-expert ("aprobar con cambios"). Hallazgos A-1 a A-3, M-1 a M-3 y B-1 a B-3 aplicados. Decisión del orquestador (2026-10-28), validada por Arquitecto y security-expert.
---

# ADR-GRP-017 — App de escritorio como cliente del daemon

> **Estado**: `proposed`. **Decisión del orquestador (2026-10-28), validada por Arquitecto y security-expert** (revisión del 2026-10-28, "aprobar con cambios", ya aplicada; ver la sección "Revisión de seguridad (2026-10-28)"). Pasa a `accepted` cuando se den estas tres condiciones: la otra sesión aplica la enmienda propuesta a ADR-GRD-008 (A-3), SPIKE-GRD-002 y el spike por reservar cierran sus dependencias, y Rene resuelve lo de su sección. El stack no se reabre: Tauri + React + Vite según [ADR-GRP-001](./ADR-GRP-001-stack-tecnologico.md), con su enmienda de alcance del 2026-10-28.

## Contexto

D7 (Rene Bonilla, 2026-10-28; BR-27) mete la app de escritorio en el MVP, en paralelo, para usuarios que no van a usar una TUI. UI-1 (macOS primero) tiene que añadir y proteger un repo sin terminal, mostrar la flota y el grafo en vivo, ofrecer "Deshacer" de un botón y explicar un bloqueo de Guardrails con su excepción consciente.

El modelo de autorización del daemon supone una terminal:

- La capa `cockpit` (escrituras del catálogo, Cancelar una operación, salida de Git) solo se concede a un "sin atribuir" que pasa los controles 1 a 3 de ADR-GRP-005 § 6. El control 3 es **terminal de control y líder de sesión** (ADR-CKP-002 § 4).
- Los comandos reservados, la confirmación de trabajo ajeno (ADR-TMC-005 § 3) y la excepción consciente (ADR-GRD-007, D10) exigen esos mismos controles.
- En Windows, el control 3 es la consola de una sesión interactiva (Enmienda TQ-14 de ADR-GRP-005).

Una app lanzada desde el Dock no tiene terminal: con las reglas actuales recibiría la capa `mcp` y no podría escribir. Relajar el control 3 tampoco sirve:

- Un agente puede abrir la app con `open -a`, y la app nace con la ascendencia limpia de `launchd` (riesgo A-2 de la Enmienda TS-GRP-004 de ADR-GRP-005).
- Con el permiso de Accesibilidad, el agente puede conducir la ventana (`osascript`, `AXPress`, eventos `CGEvent`), el vector de ADR-GRD-007 § 2.
- Un bundle que el usuario puede escribir admite que se reemplace o que se inyecte código (`DYLD_INSERT_LIBRARIES`).

**Pregunta**: ¿qué perfil y qué capa recibe la app, y cómo prueba la presencia humana sin debilitar NFR-02, BR-AUTH-001 a 004 ni los controles de ADR-GRP-005 § 6?

**Restricciones activas**: no hay `architecture-constitution.md` en la cascada. ⚠️ **ASSUMPTION**: como en ADR-GRD-008, se toman como constitución ADR-GRP-001 y ADR-GRP-002.

## Decisión

**La app es un cliente más del canal local: el webview solo habla con su backend Rust, y el backend habla con el daemon.**

- **Lectura**: la app lee como el desarrollador si su ascendencia está limpia y el daemon verifica su identidad.
- **Escritura**: para la clase de cliente `desktop` con identidad verificada, el factor del SO de ADR-GRD-008, que invoca el daemon, sustituye al control 3. Se mantienen los controles 1 y 2 y `daemon-descendant`.
- **Factor por acción** para todo lo reservado y para todo efecto remoto o no recuperable.
- **Presencia de sesión** solo para operaciones del catálogo que se revierten en local (NFR-01), y solo con la firma Developer ID verificada.
- **Sin factor**, la app es de solo lectura y muestra el comando de terminal equivalente.

### 1. Topología e IPC

| Tramo | Mecanismo | Regla |
|---|---|---|
| Webview → backend Tauri | Comandos IPC de Tauri v2 (`invoke`) y un `Channel` de Tauri por suscripción | Lista cerrada de comandos tipados, con parámetros validados y rechazo de campos desconocidos. **No hay un comando genérico** del tipo "llamar al método X del canal" |
| Backend Tauri → daemon | Canal local de ADR-GRP-005 § 5 (socket Unix 0600; named pipe en Windows), JSON-RPC de `crates/api`, handshake y capacidades de ADR-GRP-016 | Igual que la TUI: instantánea N, suscripción N+1, `resync` y reconexión (ADR-CKP-003) |
| Daemon → repos | Lectura con `gix` en el pool de consultas (grafo y diff, ADR-GRP-005, Enmienda Cockpit). Escritura solo por el ejecutor (ADR-CKP-002), con snapshot previo de la Time Machine (NFR-01) | Sin cambios |

- **Dependencias del backend** (`apps/desktop/src-tauri`): solo la biblioteca cliente del canal y, si hace falta, `crates/theme`. Hoy la biblioteca es `gitraptor_core::client`; con la enmienda E3 de ADR-CKP-003 pasa a un módulo de `crates/api`. El backend **no enlaza** `crates/git` ni `crates/policy`, y de `crates/core` solo usa `client` (la misma frontera que V5 de ADR-CKP-003).
- **Sin lógica de Git en la app**: ni el webview ni el backend lanzan `git`, abren el perfil o leen los repos.
- **El grafo lo lee el daemon** con `gix`, en consultas bajo demanda acotadas por carril. Leerlo en el proceso de la app duplicaría la frontera de ADR-GRP-009 (sin filtros ni `textconv`) y la fuente de verdad. Se reabre solo si V3 no cumple NFR-05, y entonces con enmienda de este ADR y de ADR-GRP-009.
- **Ningún otro proceso habla con el backend**: el IPC de Tauri es interno al proceso. Un proceso local que quiera algo tiene que hablar con el socket del daemon, que lo clasifica por **su** ascendencia.
- **La app verifica al daemon** (B-3): antes del handshake, el backend comprueba el par del socket con `LOCAL_PEERPID` en macOS. Debe ser del mismo usuario y su ejecutable debe ser un `raptor` instalado. Si la app está firmada, se exige además el mismo requisito de firma del § 3, con el identificador del CLI. Es la comprobación del par de ADR-CKP-003, extendida al ejecutable.
- **Entorno heredado** (M-3): el backend ignora `TAURI_*`, `WEBKIT_*`, `RUST_LOG` y cualquier variable de depuración que llegue por `open --env` o por `launchctl setenv`. Tampoco confía en el estado del webview guardado en disco (`localStorage`, IndexedDB) para nada que autorice. Las preferencias de la ventana (ADR-GRP-004) son solo cosméticas.

### 2. Superficie de Tauri (capabilities mínimas de Tauri v2)

- **Capability única** para la ventana `main`: solo los comandos propios de la app, declarados uno a uno en el manifiesto de la app. No lleva `core:default`, solo los permisos `core:*` que use un comando concreto.
- **Plugins prohibidos**: `shell` (incluido el *sidecar* por plugin), `fs`, `http`, `process`, `opener`, `deep-link`, `updater` (hasta el § 6) y cualquier plugin que abra la red o lance procesos desde el webview.
  - El selector de carpeta de "Añadir repo" lo abre el **backend** con la API Rust del diálogo; el webview no tiene ese permiso.
  - El daemon vuelve a validar la ruta elegida (SEC-02, BR-VAL-002).
- **CSP estricta**: `default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; frame-src 'none'; form-action 'none'`. Ningún origen es remoto, así que el webview no tiene red (NFR-03). Las fuentes van embebidas.
- **Endurecimiento** (B-1):
  - `withGlobalTauri: false` y `freezePrototype: true`.
  - `assetProtocol.enable: false` y `dangerousDisableAssetCspModification: false`.
  - En release: sin `devtools` y sin `devUrl` (solo el `frontendDist` empaquetado).
  - Ningún dominio remoto con acceso al IPC.
- **Sin vías de control externo**: ningún esquema de URL propio, ninguna asociación de archivos y ningún diccionario AppleScript. La app ignora sus argumentos de línea de órdenes. Una vía así permitiría a un agente conducirla con `open gitraptor://…` o `open -a GitRaptor --args …`.
- **Ventana fuera de capturas** (M-2): `NSWindow.sharingType = .none` en la ventana principal.
- **Texto no confiable** (SEC-12): el backend aplica las mismas categorías de escape que `present::ingest`. React lo pinta como texto, nunca como HTML ni como markdown (ADR-GRP-004, Enmienda 2026-10-28).
- **Patrón de aislamiento de Tauri** (*isolation pattern*): **no se adopta** (B-1). La defensa está en el daemon (§ 3 y § 4) y en que no hay un comando genérico (§ 1).
- **Cadena de suministro del frontend** (R-2, B-1): CI y release instalan con `pnpm install --frozen-lockfile --ignore-scripts` y ejecutan `pnpm audit`.

### 3. Perfil y capa que recibe la app

El daemon resuelve el solicitante como siempre: por la ascendencia del proceso, nunca por lo que declara el cliente (S-01, M-03).

| Caso | Perfil de lectura | Escritura |
|---|---|---|
| Hay un agente en la ascendencia (p. ej. el agente ejecuta el binario de la app) | `mcp` (vista acotada, allowlist) | Capa `mcp`. La app muestra "abierta por un agente: vista restringida" |
| Ascendencia limpia, sin `daemon-descendant`, e **identidad firmada verificada** (abajo) | **Completo de lectura**: vista del desarrollador; el diff bajo demanda va solo a esa conexión | Factor por acción, y presencia de sesión para lo revertible en local (§ 4) |
| Ascendencia limpia e **identidad por ruta** (solo alphas sin firmar, abajo) | Completo de lectura | **Solo factor por acción**. Nunca presencia de sesión |
| Ascendencia limpia sin ninguna de las dos identidades | Las reglas actuales de un cliente sin terminal (ADR-MCP-001 § 2) | Capa `mcp`, que rechaza las escrituras |

- **Identidad firmada verificada** (A-1): el daemon obtiene el código del par con `SecCodeCopyGuestWithAttributes` a partir de su **audit token**, nunca de una ruta, y lo valida contra el requisito `anchor apple generic and identifier "<bundle-id>" and certificate leaf[subject.OU] = "<TEAMID>"`.
  - El bundle id y el Team ID son constantes del binario del daemon.
  - Exige además *hardened runtime* (`CS_RUNTIME`) y que no haya ninguno de estos entitlements: `get-task-allow`, `disable-library-validation` ni `allow-dyld-environment-variables`.
  - Se comprueba al conectar y en cada petición de escritura, junto con la identidad del par de ADR-GRP-005 § 6.1.
  - La ruta sola no basta: el bundle se puede reescribir, admite `DYLD_INSERT_LIBRARIES` y tiene carreras TOCTOU.
  - **Dependencia de spike**: el binding de `Security.framework` choca con `unsafe_code = "forbid"`, el mismo obstáculo del punto 1 de la Enmienda TS-GRP-004. Mientras no se resuelva, ninguna app tiene identidad firmada y la presencia de sesión no existe.
- **Identidad por ruta** (solo alphas sin firmar): el ejecutable del par es el binario de la app en `/Applications` o `~/Applications`, sin traslocación de Gatekeeper y sin escritura para otros usuarios.
  - Basta para leer, que es de riesgo bajo (abajo).
  - Para escribir no prueba nada. Por eso solo admite el factor por acción, cuya seguridad descansa en el diálogo del SO con el texto que arma el daemon (§ 4) y no en la identidad de la app.
  - **Decisión** (A-1): las alphas sin firmar se quedan en lectura + factor por acción.
- **Lectura completa sin terminal: riesgo bajo, no nulo** (M-2): un agente con permiso de Accesibilidad puede leer la ventana (`AXValue`), y con Grabación de pantalla, capturarla. `sharingType = .none` (§ 2) corta las capturas, no la Accesibilidad. Se acepta porque las lecturas no cambian el repo (NFR-01) y la vista es la que el desarrollador ya ve en la TUI.

### 4. Presencia humana: factor del SO invocado por el daemon

Se reutiliza el mecanismo de [ADR-GRD-008](./ADR-GRD-008-factor-autenticacion-fuera-de-banda.md) § 3:

- lo invoca el daemon, el diálogo es del SO y la prueba nunca cruza el canal;
- plan y reto de un solo uso, con fail-closed;
- un diálogo a la vez y limitación ante la fatiga.

**Para la clase `desktop` con identidad verificada, el factor sustituye al control 3. Los controles 1 y 2 y `daemon-descendant` se siguen comprobando en cada petición, y si fallan, la petición se rechaza sin diálogo.** Esto **modifica** ADR-GRD-008 § 3, donde el factor se suma y el paso 1 exige los controles 1 a 3. La enmienda propuesta está en "Enmiendas requeridas" (A-3).

| Qué pide la app | Factor | Vigencia |
|---|---|---|
| **Presencia de sesión** (solo con identidad firmada): capa `cockpit` para las operaciones del catálogo que **se revierten en local con la Time Machine** (NFR-01): Deshacer de trabajo propio o sin atribuir, y las operaciones locales de UI-2 | Una vez. Texto del diálogo: **"Permitir que GitRaptor escriba en tus repos hasta las HH:MM. Si no abriste tú la app, cancela."** | Ligada a la conexión y al `(pid, inicio)` de la app. **Caduca** a los 15 min sin escrituras o a las 4 h. **Se invalida** al bloquear la pantalla, suspender el equipo, cambiar de usuario, cerrar la conexión o reiniciar el daemon |
| **Factor por acción**: comandos reservados (ADR-GRP-005 § 6 y ADR-GRD-007 § 1: añadir o retirar un repo, instalar la protección, `mcp.enable`…), confirmación de trabajo ajeno (ADR-TMC-005 § 3), excepción consciente (D10), confirmaciones D5/D8 y **todo efecto remoto o no recuperable** (push, borrar una rama remota, los PRs de UI-2) | Uno por acción, ligado al resumen del plan canónico (A-2) | Ninguna reutilización. Lleva la ventana, el anuncio y la auditoría de D5 cuando la acción relaja |

- **Texto del factor por acción**: lo arma el daemon en el `localizedReason` de LocalAuthentication, dentro del diálogo del SO. Usa una plantilla fija por acción con el verbo, el repo y el objetivo saneados (SEC-GRD-06), más un **código corto de correlación**. La app muestra el mismo código junto a la acción pedida.
- **El botón del webview solo pide la acción y nunca la confirma**. La única confirmación es el diálogo del SO. Por eso un clic sintético sobre la ventana no basta.
- **Desde la app, el factor es obligatorio** para todo lo de la tabla, también para lo que en la terminal es "preferente" o solo D5 (ADR-GRD-008 § 5). La app no tiene control 3 al que volver.
- **Acción compuesta**: "Añadir y proteger" puede ir como un solo plan con un solo factor si el plan canónico describe las dos partes. La forma la fija la Dev Spec.
- **Cancelar la ventana D5** no es reservado (ADR-GRP-005 § 6), y la app lo ofrece sin presencia de sesión. **Cancelar una operación en curso** (ADR-CKP-002 § 6) sí exige la capa `cockpit`, es decir, presencia de sesión (B-2).
- **Auditoría**: cada intento entra en la auditoría de ADR-GRD-008 § 4 con la clase de cliente `desktop`, el tipo de identidad (firmada o por ruta) y el código de correlación. La presencia de sesión se audita igual. Todo se publica en `reserved.audit`, que muestran la app y la TUI.
- **Disponibilidad**: el daemon anuncia si existen el factor y la identidad firmada con capacidades de ADR-GRP-016. Los nombres los fija la Dev Spec (p. ej. `presence.os-factor`, `presence.session`).

### 5. Degradación: UI-1 sin factor del SO

El módulo de presencia de ADR-GRD-008 § 6 aún no existe en el código; lo gatea SPIKE-GRD-002. Mientras no exista, y siempre que el SO no lo ofrezca (Windows 10, Hello sin configurar, Linux sin `.policy`, sesión sin interfaz):

- **La app solo lee.** Cada acción de escritura se muestra con su explicación y con el **comando de terminal equivalente**, con la ruta absoluta del `raptor` que sirve al daemon y un botón "Copiar". La app nunca ejecuta ese comando. Copiar usa el portapapeles del webview, sin plugin.
- **Forma del comando copiable** (M-1): cada argumento va entre comillas simples POSIX, con `'` escapada como `'\''`. Si algún argumento contiene caracteres de las categorías de SEC-12 (C0, DEL, C1, bidi, anchura cero), el comando **no se ofrece** y la app explica por qué.
- **Bloqueo de Guardrails**: se explica igual que en la TUI. La excepción consciente se ofrece como `raptor guard exec -- git …`, que pasa por los controles de la terminal.
- **Windows y Linux**: la app de Windows depende de Windows Hello (TS-GRP-007) y la de Linux, de polkit. Hasta entonces, solo lectura. **Pendiente: etapa de validación multiplataforma**.

### 6. Empaquetado, arranque del daemon y firma

- **La app incluye `raptor` y `raptor-mcp`** de su misma versión dentro del bundle (`bundle.externalBin`). Un usuario sin terminal obtiene todo con una sola instalación.
- **Arranque del daemon**: lo hace la biblioteca cliente, con el arranque bajo demanda de ADR-GRP-005 § 3 (argv y entorno fijos, sin shell), sobre el `raptor` del bundle y sin el plugin `shell`.
  - Si ya hay un daemon en marcha, de cualquier instalación, la app se conecta a él tras verificarlo (B-3). **Nunca lo reemplaza**: `daemon.replace` solo lo acepta el daemon desde su propio binario actualizado (SEC-13).
  - Si faltan capacidades, la app degrada y explica el motivo.
- **Autoarranque desde la app** (`raptor daemon enable` del bundle): solo con la app en `/Applications` o `~/Applications` y sin traslocación, porque el artefacto guarda la ruta estable (Enmienda US-GRP-004 de ADR-GRP-005). SEC-14 se aplica a esa ruta.
- **Alphas semanales** (v0.1.0-alpha.N, cada viernes desde `main`):
  - Se añade `GitRaptor-<versión>-aarch64-apple-darwin.dmg` **sin firmar** (firma ad hoc) al borrador de prerelease de ADR-GRP-014. Solo Apple silicon; ⚠️ **ASSUMPTION**: Intel, después.
  - La app solo tiene identidad por ruta: lectura y factor por acción, sin presencia de sesión (§ 3). El updater está desactivado.
  - Gatekeeper exige aprobar la app en Ajustes → Privacidad y seguridad, y la nota de la release lo explica.
- **v0.1.0**:
  - Firma Developer ID con *hardened runtime*, sin los entitlements prohibidos del § 3, y notarización, con los secretos de ADR-GRP-014 § 4. Es lo que habilita la identidad firmada y la presencia de sesión.
  - Si se adopta el updater de Tauri, corre en el backend con su propia clave de firma, nunca en el webview, y entra con el permiso `updater` como única excepción a la lista del § 2.

## Alternativas consideradas

| Opción | Por qué no |
|---|---|
| Relajar el control 3 para la app (bastaría la ascendencia limpia) | Un agente abre la app con `open -a` y obtiene ascendencia limpia (A-2). Debilitaría BR-AUTH-001 y NFR-02 |
| Identidad de la app por ruta para escribir | El bundle se puede reescribir, admite `DYLD_INSERT_LIBRARIES` y tiene carreras TOCTOU (A-1) |
| La app escribe en el repo o lee el grafo en su proceso (`crates/git` enlazado) | Duplica la frontera de ADR-GRP-009 y rompe "el daemon es la única fuente de verdad" (ADR-GRP-005 § 1). Escribir fuera del ejecutor salta el snapshot previo (NFR-01) |
| Una capa nueva `desktop` en el contrato | Multiplica las tablas de ADR-CKP-002 y Guardrails para el mismo poder de `cockpit`. La diferencia está en cómo se prueba la presencia, no en lo que se permite |
| Factor por cada escritura, sin sesión | Pedir Touch ID en cada "Deshacer" provoca fatiga de diálogos, y el humano acaba aprobando por reflejo. La sesión, limitada a lo revertible en local, equipara la app a la TUI |
| Sesión que cubre también lo remoto o no recuperable | Un agente con Accesibilidad la aprovecharía para efectos que la Time Machine no deshace (A-2) |
| Que la app pida el factor ella misma y envíe la prueba | La prueba cruzaría el canal y un proceso del mismo usuario podría reenviarla. Contradice ADR-GRD-008 § 3 |
| Patrón de aislamiento de Tauri | Añade complejidad y no protege lo que importa: la autorización está en el daemon y no hay un comando genérico (B-1) |
| Exigir el CLI instalado aparte | Deja fuera a los usuarios sin terminal de D7 |

## Consecuencias

- ✅ Ninguna regla de ADR-GRP-005 § 6 se debilita:
  - sin factor, la app no escribe;
  - con factor, la prueba es más fuerte que la de la terminal (condición 2 de ADR-GRD-008 § 1);
  - lo remoto y lo irreversible siempre piden factor por acción.
- ✅ Hay un solo camino de escritura (ejecutor + snapshot). La app hereda NFR-01, Guardrails y la auditoría sin código propio.
- ⚠️ **"Deshacer" de un botón y "añadir y proteger sin terminal" dependen del factor de macOS**, y la presencia de sesión depende además de la firma y del binding de `SecCode`. **Mitigación**:
  - UI-1 entra en solo lectura con comandos copiables;
  - el factor por acción llega con SPIKE-GRD-002;
  - la sesión llega con la firma (v0.1.0).
- ⚠️ En las alphas sin firmar, cada escritura pide Touch ID. Es aceptable para dogfooding.
- ⚠️ Los clientes de Windows y Linux tardan más, en línea con "macOS primero".

### Riesgos residuales

| Id | Riesgo | Estado |
|---|---|---|
| R-1 | Un agente con permiso de Accesibilidad conduce la ventana durante una presencia de sesión | Acotado (A-2): la sesión solo cubre lo revertible en local, dura como mucho 15 min sin escrituras o 4 h, y se invalida al bloquear la pantalla o suspender el equipo. Lo remoto y lo reservado piden factor por acción. Está en la clase de ADR-GRD-007 § 2 |
| R-2 | Una dependencia npm comprometida en el webview pide escrituras durante la sesión | Mitigado: no hay comando genérico (§ 1); el botón no confirma (§ 4); las escrituras son recuperables y quedan auditadas; CI instala con `--frozen-lockfile --ignore-scripts` y corre `pnpm audit` |
| R-3 | Sin firma (alphas), un proceso del usuario reemplaza el binario de la app | Acotado (A-1): con identidad por ruta no hay presencia de sesión. El factor por acción muestra en el diálogo del SO el texto armado por el daemon |
| R-4 | El humano aprueba por costumbre un diálogo que provocó un agente | Mitigado: el texto lo arma el daemon con el plan; el código de correlación coincide con el de la app; el texto de la sesión dice "si no abriste tú la app, cancela"; ADR-GRD-008 limita la fatiga |
| R-5 | Un agente con Accesibilidad lee la ventana (`AXValue`) | Aceptado, riesgo bajo (M-2): solo lectura. `sharingType = .none` corta las capturas de pantalla |

## Validación (funciones de fitness)

| # | Comprobación | Gate |
|---|---|---|
| V1 | `cargo metadata`: `gitraptor-desktop` no depende de `gitraptor-git` ni de `gitraptor-policy`, y de `gitraptor-core` solo usa el módulo `client` (comprobación estática como V5 de ADR-CKP-003) | CI, job `desktop` |
| V2 | Un test lee `tauri.conf.json` y `capabilities/` y comprueba: <ul><li>que no están los plugins prohibidos del § 2;</li><li>la CSP exacta;</li><li>`withGlobalTauri: false` y `freezePrototype: true`;</li><li>`assetProtocol.enable: false` y `dangerousDisableAssetCspModification: false`;</li><li>que no hay `devUrl` en release ni esquema de URL</li></ul> | CI, job `desktop` |
| V3 | Grafo de un repo de 100K commits por el canal: primera pintura y deltas dentro de NFR-05 y del presupuesto de ADR-GRP-011 | Banco de INF-GRP-002, en macOS |
| V4 | Tests del daemon con el doble de prueba del factor y de la identidad. Resultado esperado en cada caso: <ul><li>sin presencia de sesión: la escritura se rechaza;</li><li>con sesión e identidad firmada: se acepta una escritura local;</li><li>con sesión, una escritura remota o reservada: pide el factor por acción;</li><li>con identidad por ruta: nunca hay sesión;</li><li>agente en la ascendencia: perfil `mcp`;</li><li>sesión invalidada por bloqueo, suspensión o reinicio del daemon: se rechaza</li></ul> | CI, tests de `crates/core` |
| V5 | El frontend no usa `dangerouslySetInnerHTML` ni renderiza markdown de texto no confiable (regla de ESLint) | CI, job `desktop` |
| V6 | El comando copiable entrecomilla en POSIX y no se ofrece con caracteres de SEC-12 (M-1). Test con un corpus de argumentos hostiles | CI, job `desktop` |
| V7 | El backend ignora las variables `TAURI_*`, `WEBKIT_*` y `RUST_LOG` heredadas (M-3) y rechaza un daemon cuyo ejecutable no es un `raptor` instalado (B-3) | CI, tests de `src-tauri` |

## Dependencias de spike

| Pregunta | Dónde |
|---|---|
| ¿Hay un binding de `SecCodeCopyGuestWithAttributes` y de la validación de requisitos compatible con `unsafe_code = "forbid"`, o hace falta una excepción como `macsys`? (A-1) | **Spike por reservar** (identidad de la app de escritorio) |
| ¿El evento `oapp` permite detectar un `open -a` lanzado por un agente, con `keySenderPIDAttr`? | **Spike por reservar** |
| ¿Los `AXPress` o los `CGEvent` sintéticos alcanzan el diálogo de LocalAuthentication? ¿Se puede marcar su origen sintético con `kCGEventSourceUnixProcessID`? | SPIKE-GRD-002 (amplía su verificación de eventos sintéticos) |
| ¿Puede el daemon invocar LocalAuthentication siendo un proceso sin UI dentro del bundle (`externalBin`)? ¿A nombre de quién aparece el diálogo? | SPIKE-GRD-002 |

## Enmiendas requeridas en otros artefactos

No se aplican desde este ADR. Las aplica el orquestador al aceptarlo.

| Artefacto | Cambio |
|---|---|
| ADR-GRP-005 § 6 (control 3) | Para la clase de cliente `desktop` con identidad verificada, el control 3 se cumple con el factor del SO (§ 4 de este ADR) |
| ADR-CKP-002 § 4 (capa) | La capa `cockpit` también se concede a la app con presencia de sesión, limitada a las operaciones revertibles en local |
| ADR-MCP-001 § 2 (perfil) | Lectura completa para la app con identidad (firmada o por ruta) y ascendencia limpia (§ 3) |
| **ADR-GRD-008 § 3 y § 5** (A-3). **Otra sesión edita ADR-GRD-008**: texto propuesto para esa sesión, abajo | — |
| ADR-GRP-014 § 2 y § 3 | Añadir el `.dmg` de la app a las alphas, su firma en la release y la instalación con `--frozen-lockfile --ignore-scripts` |
| AGENTS.md (Alcance) | La app de escritorio deja de ser Fase 3 (D7) |

**Texto propuesto para ADR-GRD-008** (enmienda "2026-10-28, app de escritorio", origen ADR-GRP-017 A-3):

> **§ 3, paso 1 (Controles previos)**: para la clase de cliente `desktop` con identidad verificada (ADR-GRP-017 § 3), **el factor sustituye al control 3**. Los controles 1 y 2 y `daemon-descendant` se mantienen; si fallan, se rechaza sin diálogo (`precheck-denied`). Para cualquier otro cliente, el factor se sigue sumando a los controles 1 a 3.
>
> **§ 5 (A qué acciones se aplica)**, fila nueva: "Acciones pedidas desde la app de escritorio (ADR-GRP-017 § 4)": el factor es **obligatorio** y fail-closed para los comandos reservados, la confirmación de trabajo ajeno, la excepción consciente, las confirmaciones D5/D8 y todo efecto remoto o no recuperable, uno por acción y sin modo preferente. La presencia de sesión de ADR-GRP-017 es una entrada de factor más, con el plan "escribir hasta HH:MM", sin reutilización entre sesiones e invalidada al bloquear la pantalla, suspender el equipo, cambiar de usuario o reiniciar el daemon.
>
> **§ 3, paso 4 (Diálogo del SO)**: en macOS, el texto va en `localizedReason`, con el verbo, el repo, el objetivo y un código corto de correlación que también muestra el cliente.

## Pendiente de Rene

1. **Cuenta de Apple Developer** (99 USD al año) para firmar y notarizar la app y el CLI en v0.1.0. Sin ella no hay identidad firmada, y la app se queda en lectura + factor por acción (sin "Deshacer" de un toque).
2. **Updater de Tauri en v0.1.0**: comprobación automática o solo a petición. Las dos abren la red desde la app y hay que contrastarlas con NFR-03. Se recomienda "solo a petición".
3. **Prioridad de SPIKE-GRD-002 (macOS) y del spike por reservar** dentro de UI-1. La otra opción es aceptar que UI-1 se entregue en solo lectura con comandos copiables.
4. **Canal de la app**: un cask de Homebrew en el tap propio, además del `.dmg`.

## Revisión de seguridad (2026-10-28)

security-expert, veredicto **"aprobar con cambios"**. Todos los cambios están aplicados. **Decisión del orquestador (2026-10-28), validada por Arquitecto y security-expert.**

| Id | Severidad | Hallazgo | Resolución |
|---|---|---|---|
| A-1 | Alta | La identidad por ruta no basta (bundle reescribible, `DYLD_INSERT_LIBRARIES`, TOCTOU) | § 3: `SecCodeCopyGuestWithAttributes` por audit token + requisito con bundle id y Team ID, `CS_RUNTIME` y sin `get-task-allow`, `disable-library-validation` ni `allow-dyld-environment-variables`. Sin firma Developer ID verificada no hay presencia de sesión. Las alphas, en lectura + factor por acción. El binding queda como dependencia del spike por reservar |
| A-2 | Alta | La presencia de sesión es demasiado amplia frente a la Accesibilidad (`osascript`, `AXPress`) | § 4: la sesión cubre solo lo revertible en local; lo remoto o no recuperable pide factor por acción; caduca a los 15 min o a las 4 h; se invalida al bloquear, suspender, cambiar de usuario o reiniciar el daemon; texto fijo del diálogo |
| A-3 | Alta | Contradice ADR-GRD-008 § 3 ("se suma, no sustituye", y el paso 1 exige los controles 1 a 3) | Se retira "este ADR no modifica ADR-GRD-008". § 4 declara la sustitución y "Enmiendas requeridas" trae el texto propuesto para la sesión de ADR-GRD-008 |
| M-1 | Media | Comando copiable inyectable | § 5: comillas simples POSIX en cada argumento; con caracteres de SEC-12, no se ofrece. V6 |
| M-2 | Media | El argumento de "lectura completa sin terminal" era incorrecto (`AXValue` lee la ventana) | § 3 corregido; `sharingType = .none` en § 2; R-5, riesgo bajo |
| M-3 | Media | Entorno heredado por `open --env` y estado del webview en disco | § 1: el backend ignora `TAURI_*`, `WEBKIT_*` y `RUST_LOG`, y no confía en el estado guardado en disco. V7 |
| B-1 | Baja | Endurecimiento incompleto de Tauri | § 2 y V2: `assetProtocol.enable: false` y `dangerousDisableAssetCspModification: false`. El patrón de aislamiento no se adopta. R-2 con `--frozen-lockfile --ignore-scripts` |
| B-2 | Baja | "Cancelar" era ambiguo | § 4: cancelar la ventana D5 no exige capa; cancelar una operación (ADR-CKP-002 § 6) exige `cockpit` |
| B-3 | Baja | La app no verificaba al daemon frente a un impostor | § 1: `LOCAL_PEERPID`, mismo usuario, ejecutable `raptor` instalado y requisito de firma si existe. V7 |
| — | — | Factor por acción: texto y confirmación | § 4: `localizedReason` armado por el daemon con verbo, repo y objetivo; código de correlación en el diálogo y en la app; el botón del webview solo pide la acción |
| — | — | Preguntas que necesitan spike | "Dependencias de spike": binding `SecCode`, eventos sintéticos y `kCGEventSourceUnixProcessID`, LocalAuthentication desde el `externalBin`, y `keySenderPIDAttr` del evento `oapp` |

## Referencias

- [BRD-GRP-001, D7 y BR-27](../../business/gitraptor-documento-de-negocio.md)
- [ADR-GRD-008 — Factor de autenticación del SO](./ADR-GRD-008-factor-autenticacion-fuera-de-banda.md)
- [ADR-GRP-005 — Forma del motor y canal local](./ADR-GRP-005-forma-motor-proceso-segundo-plano.md)
- [Tauri v2 — Security](https://v2.tauri.app/security/)
- [Apple — Code Signing Services (`SecCodeCopyGuestWithAttributes`)](https://developer.apple.com/documentation/security/code_signing_services)
