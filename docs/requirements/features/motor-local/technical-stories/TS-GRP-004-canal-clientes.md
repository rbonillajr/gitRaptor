---
id: TS-GRP-004
title: "Canal local de clientes y contrato de mensajes"
type: ts
status: ready
feature: motor-local
domain: GRP
priority: high
complexity: high
created: 2026-10-03
updated: 2026-10-05
related:
  adrs: [ADR-GRP-005, ADR-GRP-011, ADR-GRP-013, ADR-GRP-012]
  stories: [US-GRP-001, US-GRP-002, US-GRP-003, US-GRP-004, US-GRP-005, US-GRP-006, US-GRP-007, US-GRP-008, US-GRP-009, US-GRP-010, US-GRP-011, US-GRP-012, US-GRP-013, US-GRP-014, US-GRP-015, US-GRP-016, TS-GRP-003]
  specs: [DS-TS-GRP-004]
ado:
  id: null
  url: null
tags: [motor-local, ipc, json-rpc, socket, named-pipe, contrato, seguridad, comandos-reservados, prompt-injection]
---

## TS-GRP-004: Canal local de clientes y contrato de mensajes

**Valor**: la CLI/TUI y el MCP consultan y siguen al motor por un canal que solo el usuario puede abrir y que nunca toca la red.

### Descripción

**Como** Arquitecto
**Quiero** el canal local entre el daemon y sus clientes con su contrato versionado y la biblioteca cliente compartida
**Para** que todas las historias expongan su estado por un único contrato seguro, sin puertos de red (NFR-03) y sin que un agente use operaciones reservadas al desarrollador (BR-AUTH-001)

> Dev Spec: [`dev-specs/TS-GRP-004-dev-spec.md`](../dev-specs/TS-GRP-004-dev-spec.md) | Aprobada (2026-10-04)
>
> **Depende de**: TS-GRP-003. **ADRs**: ADR-GRP-005 (§ 3 arranque bajo demanda, § 5 canal, § 6 comandos reservados), ADR-GRP-011 § 3 (tiempos en el evento), ADR-GRP-013 § 6 (actor expuesto), ADR-GRP-012 (procesos de agente para la ascendencia). **Seguridad**: SEC-01, SEC-02, SEC-03, SEC-08, SEC-10, SEC-12, SEC-13 y SEC-14; la Dev Spec queda bloqueada hasta que se cumplan las condiciones del gate de seguridad de `docs/architecture/non-functional.md`.
>
> **Complejidad alta**: superficie de seguridad en tres SO, con controles distintos por plataforma.

### Alcance Técnico

- **Crear** en `crates/api` el contrato JSON-RPC 2.0 con handshake de versión, consultas y comandos, y stream de eventos por suscripción en orden de secuencia.
- **Implementar** el transporte local: socket Unix en la carpeta de ejecución del perfil con umask 077 antes de `bind`, verificación de propietario y modo de la carpeta (abortar si no cuadran) y del usuario del par; named pipe como primera instancia, con acceso limitado al SID del usuario y rechazo de clientes remotos; el cliente verifica el SID del servidor y conecta solo con nivel de identificación (SEC-01).
- **Garantizar** que no existe ninguna escucha de red (NFR-03).
- **Implementar** mensajes delimitados con tamaño y profundidad máximos, campos desconocidos rechazados, batches acotados y timeout de handshake (SEC-02).
- **Validar** rutas antes de tocar el FS (rechazo de UNC, dispositivos y ADS) y después canonicalizarlas contra los repos observados (BR-VAL-002); refs con `check-ref-format` (SEC-02).
- **Implementar** colas acotadas por suscriptor con desconexión y evento "resync", límites de conexiones y suscripciones por cliente y rate limit de consultas, sirviendo desde memoria (SEC-08).
- **Implementar** la autorización de los comandos reservados solo en el daemon, incluidos retirar el registro de otro agente y parar el daemon: identificador no reutilizable del llamante (pidfd, audit token o handle), ascendencia, terminal de control y líder de sesión sin agente en su ascendencia (ADR-GRP-005 § 6, SEC-03, SEC-13).
- **Tomar** el worktree del registro de un agente del cwd del llamante, nunca de un parámetro; validar nombres declarados y prohibir los reservados.
- **Registrar** cada comando reservado, aceptado o rechazado, en el registro de auditoría append-only (ADR-GRP-013 § 1).
- **Implementar** en la CLI la confirmación explícita de los comandos reservados como paso de UX, sin valor de control para el daemon.
- **Crear** la biblioteca cliente que comparten `raptor` y `raptor-mcp`, con arranque bajo demanda del daemon vía gestor de servicios o con entorno limpio por allowlist (SEC-10), espera del handshake y sustitución de un daemon con protocolo incompatible solo desde el binario instalado (SEC-13).
- **Marcar** en el contrato como no confiable todo texto que viene del repo o de un agente, y acotar longitud y campos de lo que se devuelve a `raptor-mcp` (SEC-12).
- **Excluir** del canal accesible por `raptor-mcp` los comandos reservados y `daemon enable`/`disable` (SEC-14).
- **Incluir** en cada evento de cambio el bloque de tiempos por etapa y el helper de reloj monótono común (ADR-GRP-011 § 3).
- **Definir** el actor expuesto con solo dos variantes, agente con su origen o "sin atribuir", sin variante "humano" (ADR-GRP-013 § 6).
- **Fuera de alcance**: los métodos concretos de cada historia (los añade su Dev Spec sobre este contrato); la allowlist de herramientas del MCP (F-001-05); la presentación (F-001-02); el registro del autoarranque (US-GRP-004).

### Plan de Verificación

#### Pruebas Automatizadas

- **Bajo demanda**: con el daemon parado, `raptor` y `raptor-mcp` lo arrancan y completan el handshake; dos clientes a la vez dejan un solo daemon.
- **Versión**: un cliente con protocolo más nuevo, que es el binario instalado, para el daemon viejo y lanza el nuevo; la misma petición desde otro ejecutable se trata como parada reservada (SEC-13).
- **Acceso (SEC-01)**: la carpeta del socket y el socket solo son accesibles por el usuario; una carpeta pre-creada 0755 impide arrancar; un cliente de otro usuario es rechazado; en Windows el pipe solo admite el SID del usuario, rechaza clientes remotos y el cliente rechaza un pipe ocupado por otro proceso.
- **Red**: con el daemon en marcha no hay ningún puerto en escucha abierto por el proceso.
- **Entradas (SEC-02)**: fuzzing del decodificador; un mensaje por encima del máximo, un campo desconocido, una ruta fuera de los repos observados, una ruta UNC (0 conexiones SMB) o una ref `--upload-pack=x` se rechazan sin detener el daemon.
- **Comandos reservados (SEC-03)**: añadir o retirar un repo, corregir y parar el daemon, enviados por un cliente JSON-RPC directo descendiente de un agente simulado, se rechazan; ídem con pty bajo el agente; un registro con worktree ajeno se rechaza; un agente que retira su propio registro es aceptado y uno que intenta retirar el de otro agente es rechazado; cada intento aparece en la auditoría.
- **Robustez (SEC-08)**: un cliente que no lee y 100 conexiones simultáneas no sacan del presupuesto de ADR-GRP-011 a los demás.
- **Arranque limpio (SEC-10)**: un cliente con entorno hostil arranca un daemon que no lo hereda.
- **Salida (SEC-12)**: una rama con escapes OSC sale marcada como no confiable en el contrato; las respuestas para `raptor-mcp` no traen campos fuera de la allowlist.
- **Stream**: los eventos llegan en orden de secuencia y cada evento de cambio trae el bloque de tiempos, comparable con el reloj del cliente.
- **Actor**: el esquema del contrato no admite la variante "humano".
- **Ruta larga en macOS**: con una ruta del perfil por encima del límite del socket, el cliente conecta igualmente.

#### Verificación Manual / Sandbox

- Desde una sesión de Claude Code real, intentar añadir un repo y parar el daemon con la CLI y con un cliente JSON-RPC directo, y comprobar que se rechazan; repetir desde la terminal del desarrollador y comprobar que pide confirmación.

### Enmienda de alcance (2026-10-04, Dev Spec)

**Decisión del orquestador (2026-10-04), validada por el PO y el Arquitecto.**

- **Pasa a US-GRP-009** (se añade allí como criterio, no se pierde):
  - El registro de un agente con el worktree tomado del cwd del llamante.
  - Las pruebas "un registro con worktree ajeno se rechaza" y "un agente que retira su propio registro es aceptado y uno que retira el de otro es rechazado".
  - Motivo: todavía no existen registros. Aquí quedan el validador de nombres declarados y reservados (`channel::validate::declared_agent_name`) y la declaración de `registration.withdraw` como comando reservado.
  - En macOS falta además un wrapper seguro para leer el cwd de otro proceso.
- **Métodos declarados por adelantado**: `repo.add` (US-GRP-001), `repo.retire` (US-GRP-006), `attribution.correct` y `attribution.withdraw-correction` (US-GRP-010) y `registration.withdraw` (US-GRP-009) existen en el contrato como comandos reservados. El daemon los valida, los autoriza, los audita y responde "no implementado" con la historia que los implementa.
- **Verificación manual**: se acota a `raptor daemon stop`, porque la CLI no tiene `repo add` hasta US-GRP-001.
- **Añadidos**:
  - `raptor daemon status`: diagnóstico de solo lectura que arranca el motor bajo demanda, sin compromiso de presentación (F-001-02).
  - `audit.list`: consulta de la auditoría, fuera del MCP.
  - DEP-CKP-6: instantánea con `run_id` y `seq`, y suscripción desde `seq + 1`.
  - DEP-MCP-3: rechazo de los descendientes del propio daemon.
- **Windows y Linux**: las pruebas de pipe, SID y clientes remotos, y todas las de Linux, quedan como "Pendiente: etapa de validación multiplataforma".
- **Fuzzing**: prueba de propiedades del decodificador en lugar de `cargo-fuzz`, que necesita nightly.

### Enmienda (2026-10-05, Cockpit: N1 a N7 de ADR-CKP-003 § 4)

**Decisión del orquestador (2026-10-05), validada por el Arquitecto.** Aplica la enmienda E6 de [ADR-CKP-003](../../../../architecture/decisions/ADR-CKP-003-arquitectura-tui.md) para el hito M1 (lo que necesita INF-CKP-001). El detalle está en la [Dev Spec](../dev-specs/TS-GRP-004-dev-spec.md) § 6.

- **Protocolo 5**, con ventana de compatibilidad: el daemon atiende también a clientes del 4.
- **N1 y N2**: ámbitos global y por repo con secuencia contigua propia (`scope.snapshot`, `scope.subscribe`) y `scope.resync` con su causa.
- **N3**: resumen de atención por repo ("no disponible" hasta que lo publiquen sus dueños) y estado del autoarranque.
- **N4**: `repo.locate`.
- **N5**: solicitante y capa en el handshake.
- **N6**: nombres no confiables con tope propio, impuesto también al decodificar.
- **N7**: códigos tipados, con mensajes en/es en la CLI.
- **L-06**: el cliente comprueba la carpeta del socket.
- **Criterios añadidos**: los de la tabla § 6.2 de la Dev Spec, cada uno con su prueba.
- **Fuera**: N8 a N10 (Should, sin dueño en M1). N11 ya lo cubre TS-CKP-002.
