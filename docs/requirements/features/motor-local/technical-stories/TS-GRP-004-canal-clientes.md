---
id: TS-GRP-004
title: "Canal local de clientes y contrato de mensajes"
type: ts
status: Dev Spec Pending
feature: motor-local
domain: GRP
priority: high
complexity: high
created: 2026-10-03
updated: 2026-10-03
related:
  adrs: [ADR-GRP-005, ADR-GRP-011, ADR-GRP-013, ADR-GRP-012]
  stories: [US-GRP-001, US-GRP-002, US-GRP-003, US-GRP-004, US-GRP-005, US-GRP-006, US-GRP-007, US-GRP-008, US-GRP-009, US-GRP-010, US-GRP-011, US-GRP-012, US-GRP-013, US-GRP-014, US-GRP-015, US-GRP-016, TS-GRP-003]
  specs: []
ado:
  id: null
  url: null
tags: [motor-local, ipc, json-rpc, socket, named-pipe, contrato, seguridad]
---

## TS-GRP-004: Canal local de clientes y contrato de mensajes

**Valor**: la CLI/TUI y el MCP consultan y siguen al motor por un canal que solo el usuario puede abrir y que nunca toca la red.

### Descripción

**Como** Arquitecto
**Quiero** el canal local entre el daemon y sus clientes con su contrato versionado y la biblioteca cliente compartida
**Para** que todas las historias expongan su estado por un único contrato seguro, sin puertos de red (NFR-03) y sin que un agente use operaciones reservadas al desarrollador (BR-AUTH-001)

> Dev Spec: `dev-specs/TS-GRP-004-canal-clientes.md` | Pendiente
>
> **Depende de**: TS-GRP-003. **ADRs**: ADR-GRP-005 (§ 3 arranque bajo demanda, § 5 canal, § 6 comandos reservados), ADR-GRP-011 § 3 (tiempos en el evento), ADR-GRP-013 § 6 (actor expuesto), ADR-GRP-012 (procesos de agente para la ascendencia).
>
> **Complejidad alta**: superficie de seguridad en tres SO, con controles distintos por plataforma.

### Alcance Técnico

- **Crear** en `crates/api` el contrato JSON-RPC 2.0 con handshake de versión, consultas y comandos, y stream de eventos por suscripción en orden de secuencia.
- **Implementar** el transporte local: socket Unix en la carpeta de ejecución del perfil con verificación del usuario del par, y named pipe con acceso limitado al SID del usuario y rechazo de clientes remotos.
- **Garantizar** que no existe ninguna escucha de red (NFR-03).
- **Implementar** mensajes delimitados con tamaño máximo y validación estricta de entradas, con rutas canonicalizadas contra los repos observados (BR-VAL-002).
- **Implementar** en el daemon el rechazo de los comandos reservados cuando el llamante desciende de un proceso de agente detectado (ADR-GRP-005 § 6, PQ-6).
- **Implementar** en la CLI el envío de los comandos reservados solo con terminal interactiva y confirmación explícita.
- **Crear** la biblioteca cliente que comparten `raptor` y `raptor-mcp`, con arranque bajo demanda del daemon, espera del handshake y sustitución de un daemon con protocolo incompatible.
- **Incluir** en cada evento de cambio el bloque de tiempos por etapa y el helper de reloj monótono común (ADR-GRP-011 § 3).
- **Definir** el actor expuesto con solo dos variantes, agente con su origen o "sin atribuir", sin variante "humano" (ADR-GRP-013 § 6).
- **Fuera de alcance**: los métodos concretos de cada historia (los añade su Dev Spec sobre este contrato); la allowlist de herramientas del MCP (F-001-05); la presentación (F-001-02); el registro del autoarranque (US-GRP-004).

### Plan de Verificación

#### Pruebas Automatizadas

- **Bajo demanda**: con el daemon parado, `raptor` y `raptor-mcp` lo arrancan y completan el handshake; dos clientes a la vez dejan un solo daemon.
- **Versión**: un cliente con protocolo más nuevo para el daemon viejo y lanza el nuevo.
- **Acceso**: la carpeta del socket y el socket solo son accesibles por el usuario; un cliente de otro usuario es rechazado; en Windows el pipe solo admite el SID del usuario y rechaza clientes remotos.
- **Red**: con el daemon en marcha no hay ningún puerto en escucha abierto por el proceso.
- **Entradas**: un mensaje por encima del máximo, un campo desconocido en un comando o una ruta fuera de los repos observados se rechazan sin detener el daemon.
- **Comandos reservados**: añadir o retirar un repo y corregir, lanzados como descendientes de un proceso de agente simulado, se rechazan; sin terminal la CLI no los envía.
- **Stream**: los eventos llegan en orden de secuencia y cada evento de cambio trae el bloque de tiempos, comparable con el reloj del cliente.
- **Actor**: el esquema del contrato no admite la variante "humano".
- **Ruta larga en macOS**: con una ruta del perfil por encima del límite del socket, el cliente conecta igualmente.

#### Verificación Manual / Sandbox

- Desde una sesión de Claude Code real, intentar añadir un repo con la CLI y comprobar que se rechaza; repetir desde la terminal del desarrollador y comprobar que pide confirmación.
