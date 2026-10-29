---
id: ADR-GRP-017
title: "App de escritorio como cliente del daemon: frontera de seguridad y presencia humana sin terminal"
type: adr
status: proposed
date: 2026-10-28
created: 2026-10-28
updated: 2026-10-28
deciders: [Rene Bonilla, Orquestador (autonomía delegada por Rene Bonilla), Arquitecto]
domain: GRP
feature: desktop
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-003, ADR-GRP-004, ADR-GRP-005, ADR-GRP-006, ADR-GRP-009, ADR-GRP-014, ADR-GRP-016, ADR-MCP-001, ADR-CKP-002, ADR-CKP-003, ADR-GRD-007, ADR-GRD-008, ADR-TMC-005, SPIKE-GRD-002, TS-GRP-004, TS-GRP-007]
tags: [desktop, tauri, gui, canal-local, presencia-humana, factor-del-so, comandos-reservados, capa-cockpit, tq-14, a-2, nfr-01, nfr-02, nfr-03, d7, br-27]
changelog:
  - 2026-10-28: creado (D7, app de escritorio en el MVP). Decisión del orquestador (2026-10-28), validada por Arquitecto. Pendiente de la revisión de security-expert.
---

# ADR-GRP-017 — App de escritorio como cliente del daemon

> **Estado**: `proposed`. La forma es **Decisión del orquestador (2026-10-28), validada por Arquitecto**. Pasa a `accepted` cuando el security-expert revise los § 3 y § 4 (ver "Revisión de seguridad pendiente"). El stack no se reabre: Tauri + React + Vite según [ADR-GRP-001](./ADR-GRP-001-stack-tecnologico.md), con su enmienda de alcance del 2026-10-28.

## Contexto

D7 (Rene Bonilla, 2026-10-28; BR-27) mete la app de escritorio en el MVP, en paralelo, para usuarios que no van a usar una TUI. UI-1 (macOS primero) tiene que añadir y proteger un repo sin terminal, mostrar la flota y el grafo en vivo, ofrecer "Deshacer" de un botón y explicar un bloqueo de Guardrails con su excepción consciente.

El modelo de autorización del daemon supone una terminal:

- La capa `cockpit` (escrituras del catálogo, Cancelar, salida de Git) solo se concede a un "sin atribuir" que pasa los controles 1 a 3 de ADR-GRP-005 § 6, y el control 3 es **terminal de control y líder de sesión** (ADR-CKP-002 § 4).
- Los comandos reservados, la confirmación de trabajo ajeno (ADR-TMC-005 § 3) y la excepción consciente (ADR-GRD-007, D10) exigen los mismos controles.
- En Windows, el control 3 es la consola de una sesión interactiva (Enmienda TQ-14 de ADR-GRP-005).

Una app lanzada desde el Dock no tiene terminal: con las reglas actuales recibiría la capa `mcp` y no podría escribir. Tampoco basta con relajar el control 3 para ella. Un agente puede abrir la app con `open -a`, y la app nace entonces con la ascendencia limpia de `launchd`: es el riesgo A-2 de la Enmienda TS-GRP-004 de ADR-GRP-005. Y si el agente tiene el permiso de Accesibilidad, puede automatizar la ventana (`osascript`), el mismo vector de ADR-GRD-007 § 2.

**Pregunta**: ¿qué perfil y qué capa recibe la app, y cómo prueba la presencia humana sin debilitar NFR-02, BR-AUTH-001 a 004 ni los controles de ADR-GRP-005 § 6?

**Restricciones activas**: no hay `architecture-constitution.md` en la cascada. ⚠️ **ASSUMPTION**: rigen como constitución ADR-GRP-001 y ADR-GRP-002, como en ADR-GRD-008.

## Decisión

**La app es un cliente más del canal local. Su backend Rust habla con el daemon y el webview solo habla con su backend. La app lee como el desarrollador si el daemon verifica su identidad y su ascendencia está limpia. Para escribir, el control 3 se sustituye por el factor del SO de ADR-GRD-008, que invoca el daemon: una vez por sesión para la capa `cockpit` y una vez por acción para todo lo reservado. Mientras ese factor no exista, la app es de solo lectura y muestra el comando de terminal equivalente.**

### 1. Topología e IPC

| Tramo | Mecanismo | Regla |
|---|---|---|
| Webview → backend Tauri | Comandos IPC de Tauri v2 (`invoke`) y un `Channel` de Tauri por suscripción | Lista cerrada de comandos tipados, con parámetros validados y rechazo de campos desconocidos. **No hay un comando genérico** del tipo "llamar al método X del canal" |
| Backend Tauri → daemon | Canal local de ADR-GRP-005 § 5 (socket Unix 0600; named pipe en Windows), JSON-RPC de `crates/api`, handshake y capacidades de ADR-GRP-016 | Igual que la TUI: instantánea N, suscripción N+1, `resync` y reconexión (ADR-CKP-003) |
| Daemon → repos | Lectura con `gix` en el pool de consultas (grafo y diff, ADR-GRP-005, Enmienda Cockpit) y escritura solo por el ejecutor (ADR-CKP-002) con snapshot previo de la Time Machine (NFR-01) | Sin cambios |

- **Dependencias del backend** (`apps/desktop/src-tauri`): solo la biblioteca cliente del canal (hoy `gitraptor_core::client`; con la enmienda E3 de ADR-CKP-003, su módulo en `crates/api`) y `crates/theme` si hace falta. **No enlaza** `crates/git` ni `crates/policy`, y de `crates/core` solo usa `client`. Es la misma frontera de módulo que la Validación V5 de ADR-CKP-003.
- **Sin lógica de Git en la app**: ni en el webview ni en el backend. La app no lanza `git` ni abre el perfil, y no lee los repos.
- **El grafo lo lee el daemon** (`gix`, consultas bajo demanda acotadas por carril). Leerlo en el proceso de la app duplicaría la frontera de ADR-GRP-009 (sin filtros, sin `textconv`) y la fuente de verdad. Se reabre solo si la medición de V3 no cumple NFR-05, con enmienda de este ADR y de ADR-GRP-009.
- **Ningún otro proceso habla con el backend**: el IPC de Tauri vive dentro del proceso. Un proceso local que quiera algo tiene que hablar con el socket del daemon, que lo clasifica por **su** ascendencia.

### 2. Superficie de Tauri (capabilities mínimas de Tauri v2)

- **Capability única** para la ventana `main`: solo los comandos propios de la app, declarados uno a uno en el manifiesto de la app. Sin `core:default`; solo los permisos `core:*` que use un comando concreto.
- **Plugins prohibidos**: `shell` (incluido el *sidecar* por plugin), `fs`, `http`, `process`, `opener`, `deep-link`, `updater` (hasta el § 6) y cualquier plugin que abra la red o lance procesos desde el webview. El selector de carpeta para "Añadir repo" lo abre el **backend** con la API Rust del diálogo; el webview no recibe ese permiso. La ruta elegida la vuelve a validar el daemon (SEC-02, BR-VAL-002).
- **CSP estricta**: `default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; frame-src 'none'; form-action 'none'`. Sin orígenes remotos: el webview no tiene red (NFR-03). Las fuentes van embebidas.
- **Endurecimiento**: `withGlobalTauri: false`, `freezePrototype: true`, sin `devtools` en release, sin `devUrl` en release (solo `frontendDist` empaquetado) y sin dominios remotos con acceso al IPC.
- **Sin vías de control externo**: ningún esquema de URL propio, ninguna asociación de archivos, ningún diccionario AppleScript. Los argumentos de línea de órdenes de la app se ignoran. Una vía así permitiría que un agente condujera la app con `open gitraptor://…` o `open -a GitRaptor --args …`.
- **Texto no confiable** (SEC-12): el backend aplica las mismas categorías de escape que `present::ingest` y React lo pinta como texto, nunca como HTML ni como markdown (ADR-GRP-004, Enmienda 2026-10-28).
- **Patrón de aislamiento de Tauri** (*isolation pattern*): no se adopta en UI-1. Lo decide la revisión de seguridad (abajo), porque la defensa principal está en el daemon (§ 3 y § 4).

### 3. Perfil y capa que recibe la app

El daemon resuelve el solicitante como siempre: por la ascendencia del proceso, nunca por lo que declare el cliente (S-01, M-03).

| Caso | Perfil de lectura | Capa de escritura |
|---|---|---|
| Hay un agente en la ascendencia (p. ej. el agente ejecuta el binario de la app) | `mcp` (vista acotada, allowlist) | `mcp`. La app muestra "abierta por un agente: vista restringida" |
| Ascendencia limpia, sin `daemon-descendant`, e **identidad de app verificada** (abajo) | **Completo de lectura** (vista del desarrollador, con el diff bajo demanda solo para esa conexión) | Ninguna hasta la presencia de sesión (§ 4) |
| Ascendencia limpia, identidad sin verificar | Las reglas actuales de un cliente sin terminal (ADR-MCP-001 § 2) | `mcp`, que rechaza escrituras |

- **Identidad de app verificada**: el ejecutable del par (identificador no reutilizable de ADR-GRP-005 § 6.1) es el binario de la app instalado en `/Applications` o `~/Applications`, sin traslocación de Gatekeeper, y sin escritura para otros usuarios. Con la app firmada (§ 6), además, la firma de código del par tiene el Team ID esperado, que es una constante del binario del daemon. Se comprueba al conectar y en cada petición, igual que la identidad del par.
- **Por qué lectura completa sin terminal**: la salida va a una ventana, no a un stdio que lea el agente. Para leerla, un agente que abre la app con `open` necesita además el permiso de Grabación de pantalla. Las lecturas no cambian el repo (NFR-01).

### 4. Presencia humana: factor del SO invocado por el daemon

Se reutiliza el mecanismo de [ADR-GRD-008](./ADR-GRD-008-factor-autenticacion-fuera-de-banda.md) § 3, que este ADR no modifica: lo invoca el daemon, el diálogo es del SO y la prueba nunca cruza el canal. El plan y el reto son de un solo uso, con fail-closed. Existe un diálogo a la vez y hay limitación ante la fatiga. Los controles 1 y 2 y `daemon-descendant` se siguen comprobando **en cada petición**. Para esta clase de cliente, el factor sustituye solo al control 3.

| Qué pide la app | Factor | Reutilización |
|---|---|---|
| **Presencia de sesión**: capa `cockpit` para las operaciones no reservadas del catálogo (Deshacer de trabajo propio o sin atribuir, operaciones de UI-2) | Una vez, con el plan "activar escritura para la app de escritorio" (ejecutable y `(pid, inicio)`) | Ligada a la conexión y al `(pid, inicio)` del proceso. Caduca al cerrarse la conexión, tras 30 min sin peticiones de escritura o a las 8 h (⚠️ **ASSUMPTION**: valores a fijar en la revisión de seguridad) |
| **Comandos reservados** de ADR-GRP-005 § 6 y ADR-GRD-007 § 1 (añadir o retirar un repo, instalar la protección, `mcp.enable`…), **confirmación de trabajo ajeno** (ADR-TMC-005 § 3), **excepción consciente** (D10) y **confirmaciones D5/D8** | Uno **por acción**, ligado al resumen del plan canónico | Ninguna: es el flujo de ADR-GRD-008 § 3, con la ventana, el anuncio y la auditoría de D5 cuando la acción relaja |

- **Desde la app, el factor es obligatorio** para todas esas acciones, también para las que en la terminal son "preferentes" o solo D5 (ADR-GRD-008 § 5). La app no tiene control 3, así que no hay nada a lo que caer.
- **Acción compuesta**: "Añadir y proteger" puede ir como un solo plan con un solo factor si el plan canónico describe las dos partes. La forma la fija la Dev Spec.
- **Cancelar** durante la ventana D5 no es reservado (ADR-GRP-005 § 6). La app lo ofrece aunque no tenga presencia de sesión.
- **Auditoría**: cada intento entra en la auditoría de ADR-GRD-008 § 4 con la clase de cliente `desktop`, y la presencia de sesión también. Se publica en `reserved.audit`, que la app y la TUI muestran.
- **Disponibilidad**: el daemon anuncia si el factor existe con una capacidad de ADR-GRP-016 (nombre en la Dev Spec, p. ej. `presence.os-factor`).

### 5. Degradación: UI-1 sin factor del SO

El módulo de presencia de ADR-GRD-008 § 6 aún no existe en el código: lo gatea SPIKE-GRD-002. Mientras no exista, y siempre que el SO no lo ofrezca (Windows 10, Hello sin configurar, Linux sin `.policy`, sesión sin interfaz):

- La app **solo lee**. Cada acción de escritura se muestra con su explicación y con el **comando de terminal equivalente** (`raptor …` con la ruta absoluta del binario que sirve al daemon) y un botón "Copiar". Nunca lo ejecuta. Copiar usa el portapapeles del webview, sin plugin.
- El bloqueo de Guardrails se explica igual que en la TUI. La excepción consciente se ofrece como `raptor guard exec -- git …`, que pasa por los controles de la terminal.
- **Windows**: la app depende de Windows Hello (TS-GRP-007) y **Linux**, de polkit. Hasta entonces, solo lectura. **Pendiente: etapa de validación multiplataforma**.

### 6. Empaquetado, arranque del daemon y firma

- **La app incluye `raptor` y `raptor-mcp`** de su misma versión dentro del bundle (`bundle.externalBin`). Un usuario sin terminal obtiene todo con una instalación.
- **El daemon lo arranca la biblioteca cliente**, con el arranque bajo demanda de ADR-GRP-005 § 3 (argv y entorno fijos, sin shell) sobre el `raptor` del bundle. No se usa el plugin `shell`. Si ya hay un daemon en marcha, de cualquier instalación (Homebrew, npm, script), la app se conecta a él. **Nunca lo reemplaza**: `daemon.replace` solo lo acepta el daemon desde su propio binario actualizado (SEC-13). Si faltan capacidades, la app degrada con el motivo.
- **Autoarranque desde la app** (`raptor daemon enable` del bundle): solo con la app en `/Applications` o `~/Applications` y sin traslocación, porque el artefacto guarda la ruta estable (Enmienda US-GRP-004 de ADR-GRP-005). SEC-14 se aplica a esa ruta.
- **Alphas semanales** (v0.1.0-alpha.N, cada viernes desde `main`): se añade `GitRaptor-<versión>-aarch64-apple-darwin.dmg` **sin firmar** (firma ad hoc) al borrador de prerelease de ADR-GRP-014. Solo Apple silicon (⚠️ **ASSUMPTION**: Intel después). El updater está desactivado. Gatekeeper exige aprobar la app en Ajustes → Privacidad y seguridad, y la nota de la release lo explica.
- **v0.1.0**: firma Developer ID con *hardened runtime* y notarización, con los mismos secretos de ADR-GRP-014 § 4. Sin firma no hay comprobación de Team ID (§ 3) y la identidad de app queda en la comprobación de ruta (riesgo R-3). El updater de Tauri, si se adopta, corre en el backend con su propia clave de firma, nunca en el webview, y entra con el permiso `updater` como única excepción a la lista del § 2.

## Alternativas consideradas

| Opción | Por qué no |
|---|---|
| Relajar el control 3 para la app (bastaría la ascendencia limpia) | Un agente abre la app con `open -a` y obtiene ascendencia limpia (A-2). Debilitaría BR-AUTH-001 y NFR-02 |
| La app escribe en el repo o lee el grafo en su proceso (`crates/git` enlazado) | Duplica la frontera de ADR-GRP-009 y rompe "el daemon es la única fuente de verdad" (ADR-GRP-005 § 1). Escribir fuera del ejecutor saltaría el snapshot previo (NFR-01) |
| Una capa nueva `desktop` en el contrato | Multiplica las tablas de ADR-CKP-002 y Guardrails para el mismo poder de `cockpit`. La diferencia está en cómo se prueba la presencia, no en lo que se permite |
| Factor por cada escritura, sin sesión | Pedir Touch ID en cada "Deshacer" dispara la fatiga de diálogos y el humano aprueba por reflejo. La sesión equipara la app a la TUI, que tampoco pide factor para la capa `cockpit` |
| Que la app pida el factor ella misma y envíe la prueba | La prueba cruzaría el canal y un proceso del mismo usuario podría reenviarla. Contradice ADR-GRD-008 § 3 |
| Exigir el CLI instalado aparte | Deja fuera a los usuarios sin terminal de D7 |

## Consecuencias

- ✅ Ninguna regla de ADR-GRP-005 § 6 se debilita. Sin factor, la app no escribe, y con factor tiene una prueba más fuerte que la de la terminal (condición 2 de ADR-GRD-008 § 1).
- ✅ Hay un solo camino de escritura (ejecutor + snapshot), y la app hereda NFR-01, Guardrails y la auditoría sin código propio.
- ⚠️ **"Deshacer" de un botón y "añadir y proteger sin terminal" dependen del factor de macOS**. SPIKE-GRD-002 (parte macOS) y el módulo de presencia del daemon pasan a la ruta crítica de UI-1. **Mitigación**: UI-1 entra primero en solo lectura con comandos copiables, y las escrituras se activan cuando llega la capacidad.
- ⚠️ La presencia de sesión es una ventana en la que un agente con permiso de Accesibilidad podría conducir la app. Es la misma clase que `tmux send-keys` sobre la TUI (ADR-GRD-007 § 2). Lo reservado sigue pidiendo factor por acción, y todo lo de la capa `cockpit` es recuperable (NFR-01) y queda auditado.
- ⚠️ Los clientes de Windows y Linux tardan más. Es coherente con "macOS primero".

### Riesgos residuales

| Id | Riesgo | Estado |
|---|---|---|
| R-1 | Un agente con permiso de Accesibilidad conduce la ventana durante una presencia de sesión | Aceptado, en la clase de ADR-GRD-007 § 2. Lo reservado exige factor por acción |
| R-2 | Una dependencia npm comprometida en el webview pide escrituras de la capa `cockpit` durante la sesión | Mitigado: no hay comando genérico (§ 1), las escrituras son recuperables y auditadas, y lo reservado exige factor. Lockfile y `pnpm audit` en CI |
| R-3 | Sin firma (alphas), un proceso del usuario reemplaza el binario de la app y pasa la comprobación de ruta | Aceptado solo en las alphas, como el "código plantado" de ADR-GRD-007 § 2. Se cierra con la firma y el Team ID (§ 6) |
| R-4 | El humano aprueba por costumbre un diálogo que provocó un agente | Mitigado por ADR-GRD-008 (texto armado por el daemon con el plan, limitación ante la fatiga, un diálogo a la vez) |

## Validación (funciones de fitness)

| # | Comprobación | Gate |
|---|---|---|
| V1 | `cargo metadata`: `gitraptor-desktop` no depende de `gitraptor-git` ni de `gitraptor-policy`, y de `gitraptor-core` solo usa el módulo `client` (comprobación estática como V5 de ADR-CKP-003) | CI, job `desktop` |
| V2 | La configuración de Tauri y las capabilities: sin los plugins prohibidos del § 2, con la CSP exacta, `withGlobalTauri: false` y sin esquema de URL. Es un test que lee `tauri.conf.json` y `capabilities/` | CI, job `desktop` |
| V3 | Grafo de un repo de 100K commits por el canal: primera pintura y deltas dentro de NFR-05 y del presupuesto de ADR-GRP-011 | Banco de INF-GRP-002, en macOS |
| V4 | Tests del daemon con el doble de prueba del factor: app sin presencia → la escritura se rechaza; con la sesión activa → se acepta; con un reservado → factor por acción; con un agente en la ascendencia → `mcp`; con una identidad sin verificar → sin lectura completa | CI, tests de `crates/core` |
| V5 | El frontend no usa `dangerouslySetInnerHTML` ni renderiza markdown de texto no confiable (regla de ESLint) | CI, job `desktop` |

## Enmiendas requeridas en otros artefactos

No se aplican desde este ADR. Las aplica el orquestador al aceptarlo.

| Artefacto | Cambio |
|---|---|
| ADR-GRP-005 § 6 (control 3) | Para la clase de cliente `desktop` verificada, el control 3 se cumple con el factor del SO (§ 4 de este ADR) |
| ADR-CKP-002 § 4 (capa) | La capa `cockpit` también se concede a la app con presencia de sesión |
| ADR-MCP-001 § 2 (perfil) | Lectura completa para la app con identidad verificada y ascendencia limpia (§ 3) |
| ADR-GRD-008 § 5 | Fila nueva: desde la app, todas las acciones del § 4 son obligatorias con factor. **Otra sesión edita ADR-GRD-008: coordinar** |
| ADR-GRP-014 § 2 y § 3 | Artefacto `.dmg` de la app en las alphas y su firma en la release |
| AGENTS.md (Alcance) | La app de escritorio deja de ser Fase 3 (D7) |

## Pendiente de Rene

1. **Cuenta de Apple Developer** (99 USD al año) para firmar y notarizar la app y el CLI en v0.1.0. Sin ella, R-3 sigue abierto en v0.1.0.
2. **Updater de Tauri en v0.1.0**: comprobación automática o solo a petición del usuario. Cualquiera de las dos abre la red desde la app, y hay que contrastarlo con NFR-03. La recomendación es "solo a petición".
3. **Prioridad de SPIKE-GRD-002 (macOS) y del módulo de presencia** dentro de UI-1, o aceptar que UI-1 se entregue en solo lectura con comandos copiables.
4. **Canal de la app**: un cask de Homebrew en el tap propio, además del `.dmg`.

## Revisión de seguridad pendiente

Antes de `accepted`, el security-expert revisa: la lectura completa sin terminal (§ 3), la presencia de sesión y sus plazos (§ 4), la comprobación de identidad de app sin firma (R-3), el patrón de aislamiento de Tauri (§ 2) y la CSP. Sus NFRs de seguridad se incorporan aquí tal cual.

## Referencias

- [BRD-GRP-001, D7 y BR-27](../../business/gitraptor-documento-de-negocio.md)
- [ADR-GRD-008 — Factor de autenticación del SO](./ADR-GRD-008-factor-autenticacion-fuera-de-banda.md)
- [ADR-GRP-005 — Forma del motor y canal local](./ADR-GRP-005-forma-motor-proceso-segundo-plano.md)
- [Tauri v2 — Capabilities, CSP e isolation](https://v2.tauri.app/security/)
