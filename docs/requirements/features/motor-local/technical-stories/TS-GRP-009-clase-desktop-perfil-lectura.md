---
id: TS-GRP-009
title: "Clase de cliente de la app de escritorio y perfil de lectura mcp con un agente en su ascendencia"
type: ts
status: ready
feature: motor-local
domain: GRP
priority: high
complexity: high
created: 2026-11-08
updated: 2026-11-09
related:
  adrs: [ADR-GRP-017, ADR-GRP-005, ADR-GRP-016, ADR-MCP-001, ADR-CKP-003]
  stories: [US-DSK-003, US-DSK-004, TS-DSK-002, SPIKE-DSK-001, SPIKE-DSK-002, US-GRP-007]
  specs: [DS-US-DSK-003, DS-TS-GRP-009]
ado:
  id: null
  url: null
tags: [desktop, canal, perfil-mcp, ascendencia, identidad-por-ruta, dep-dsk-2, seguridad, v4]
---

## TS-GRP-009: Clase de cliente de la app de escritorio y perfil de lectura mcp con un agente en su ascendencia

**Valor**: una ventana de la app que abrió un agente ve lo mismo que ese agente por el MCP, y lo decide el daemon, no la app.

> Dev Spec: [DS-TS-GRP-009](../dev-specs/TS-GRP-009-dev-spec.md). Precisa la clase (la deduce el daemon del ejecutable, sin cambiar `hello`), el contrato (método `connection.desktop` en lugar de una capacidad, ADR-GRP-016) y la fila 4 (completo solo con los controles 1 a 3). Decisión del orquestador (2026-11-09), validada por Arquitecto.

### Descripción

**Como** Arquitecto
**Quiero** que el daemon reconozca la app de escritorio como clase de cliente propia y le asigne el perfil de lectura `mcp` cuando haya un agente en la ascendencia del proceso, resolviéndolo él mismo por la ascendencia y el ejecutable del par, nunca por lo que el cliente declare
**Para** cumplir la fila 1 de la tabla de [ADR-GRP-017 § 3](../../../../architecture/decisions/ADR-GRP-017-app-escritorio-cliente-daemon.md#3-perfil-y-capa-que-recibe-la-app) y la fila "agente en la ascendencia: perfil `mcp`" de V4, y cerrar el hueco G1 de [DS-US-DSK-003](../../desktop/dev-specs/US-DSK-003-estado-del-motor.md)

**Estado actual (verificado en el código el 2026-11-08).** El daemon da el perfil `mcp` solo a un par cuyo ejecutable se llama `raptor-mcp` o a un cliente que se declara `mcp` (`crates/core/src/channel/conn.rs:535-546`); no existe la clase de la app. Una app abierta por un agente recibe hoy el perfil completo. Por eso, **hasta que esta historia se mezcle, la restricción que la app se aplica a sí misma por el actor del solicitante (DS-US-DSK-003 D6) es cosmética, no un control**: un binario de la app modificado o cualquier proceso que hable con el socket se la salta. **La lectura no es más amplia que la de un agente que ejecuta `raptor` hoy**: los métodos ofrecidos dependen solo del perfil (`conn.rs:790-794`), y un agente que lanza la CLI obtiene el mismo perfil completo, porque no se llama `raptor-mcp` ni se declara `mcp`. La app no abre una vía de lectura nueva; esta historia la estrecha para la app.

**Decisión del orquestador (2026-11-08), validada por Arquitecto**: la identidad por ruta de las alphas va aquí, porque decide el perfil de lectura de una app con ascendencia limpia (filas 3 y 4 de la tabla). La identidad firmada se queda en SPIKE-DSK-001, y el factor y la presencia en SPIKE-GRD-002.

### Alcance Técnico

- **Añadir** al contrato del canal la clase de cliente de la app de escritorio como cambio aditivo (ADR-GRP-016), sin subir el protocolo; un cliente que no la conoce ve las mismas formas que hoy. Precisión (2026-11-09): la clase la deduce el daemon del ejecutable del par y `hello` no cambia de forma; lo nuevo es el método `connection.desktop` (protocolo 9), que se descubre en `hello.methods` y no necesita capacidad (DS-TS-GRP-009 D1 y D2). Decisión del orquestador (2026-11-09), validada por Arquitecto.
- **Implementar** en el daemon la asignación del perfil de lectura de una conexión de esa clase, con la ascendencia resuelta por el propio daemon:
  - agente en la ascendencia del par: perfil `mcp` (fila 1);
  - ascendencia limpia, sin `daemon-descendant` e identidad por ruta verificada: perfil completo (fila 3);
  - ascendencia limpia sin identidad: las reglas actuales de un cliente sin terminal de ADR-MCP-001 § 2 (fila 4): perfil completo solo si pasan los controles 1 a 3; si no, `mcp` (DS-TS-GRP-009 D3);
  - una identidad que no se puede verificar: perfil `mcp` (falla cerrada).
- **Implementar** la identidad por ruta de las alphas sin firmar: el ejecutable del par, leído del kernel y nunca del cliente, es el binario de la app dentro de la carpeta de aplicaciones del sistema o del usuario, por ruta canónica, sin traslocación de Gatekeeper ni volumen montado y sin escritura para otros usuarios.
- **Garantizar** que un ejecutable que se hace pasar por la app (mismo nombre fuera de la ruta de la app) no da más de lo que daría declararse `cli`, y que una conexión no cambie de clase ni de perfil tras el handshake. El control real es la identidad para escribir, no para leer (Ajuste tras la revisión (2026-11-08), validado por Arquitecto / security-expert, I-01).
- **Publicar** a esa clase el solicitante resuelto (en el `requester` del `hello` de siempre) y el motivo del perfil (agente en la ascendencia, sin identidad), este último con el método `connection.desktop`, para que la app pinte la banda "abierta por un agente" con un dato del daemon en lugar de deducirlo.
- **Registrar** en el log del daemon la clase, el tipo de identidad y el perfil asignado de cada conexión de la app, sin rutas de repos ni texto del cliente.
- **Fuera de alcance**: la identidad firmada y el requisito de firma (SPIKE-DSK-001); el factor del SO, la presencia de sesión y cualquier escritura de la app (SPIKE-GRD-002, DEP-DSK-1); detectar una app lanzada por un agente con `open -a`, cuya ascendencia queda limpia (SPIKE-DSK-002, hueco R-6 aceptado para las alphas); restringir a un agente que ejecuta la CLI `raptor` (no lo pide ADR-GRP-017); los cambios de la app para declararse de la nueva clase y leer el motivo (los hace US-DSK-003 o la primera historia de la app tras la mezcla); Windows y Linux (**Pendiente: etapa de validación multiplataforma**).

### Plan de Verificación

#### Pruebas Automatizadas

- **Fila 1 de V4 (lectura)**: con el doble de prueba de la ascendencia, un par de la clase de la app con un agente en su ascendencia recibe el perfil `mcp`, no ve los métodos que el perfil `mcp` no ofrece y lleva el motivo en el handshake.
- **Filas 3 y 4**: ascendencia limpia con identidad por ruta, perfil completo; ascendencia limpia sin identidad, el resultado de un cliente sin terminal (completo solo con los controles 1 a 3; si no, `mcp`).
- **Identidad por ruta**: rechazada para un binario fuera de las carpetas de aplicaciones, traslocado, en un volumen montado, escribible por otros o con otro nombre; el resultado no cambia con lo que el cliente declare.
- **Sin escalada** (I-01): un impostor (un proceso que no es el binario de la app) que se declara de su clase obtiene exactamente el solicitante, y nunca más perfil ni métodos (salvo `connection.desktop`, que solo describe su conexión), que los que obtendría declarándose `cli` desde el mismo proceso (DS-TS-GRP-009 D6); nunca la identidad por ruta ni una capa de escritura. Un intento de cambiar de clase tras el handshake se rechaza. La clase solo estrecha la lectura; lo que protege es la identidad exigida para escribir. Ajuste tras la revisión (2026-11-08), validado por Arquitecto / security-expert.
- **Compatibilidad**: la CLI, la TUI, `raptor-mcp` y el cliente de los hooks reciben las mismas formas y perfiles que antes (suites del canal en `crates/core` en verde).

#### Verificación Manual / Sandbox

- En macOS, con la alpha instalada en la carpeta de aplicaciones: abrirla desde el Finder da la flota completa; ejecutar el binario de la app desde una sesión de Claude Code da la banda "abierta por un agente" y ningún repo, y el log del daemon registra el perfil `mcp` con su motivo.
- La misma app ejecutada desde la imagen montada no obtiene identidad por ruta.
