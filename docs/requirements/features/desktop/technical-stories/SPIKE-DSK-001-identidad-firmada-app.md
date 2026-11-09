---
id: SPIKE-DSK-001
title: "Verificar en el daemon la firma de la app de escritorio desde su audit token sin romper unsafe_code = forbid"
type: spike
status: ready
feature: desktop
domain: GRP
priority: high
complexity: medium
created: 2026-11-08
updated: 2026-11-08
blocked_by: []
related:
  adrs: [ADR-GRP-017, ADR-GRP-005, ADR-GRP-001, ADR-GRP-002, ADR-GRD-008, ADR-CKP-002]
  stories: [US-DSK-002, US-DSK-006, US-DSK-010, US-DSK-011, US-DSK-012, US-DSK-013, US-DSK-014, US-DSK-016, SPIKE-GRD-002, TS-GRP-004]
  specs: []
ado:
  id: null
  url: null
tags: [desktop, spike, identidad-firmada, seccode, audit-token, code-signing, macsys, unsafe-code, presencia-de-sesion, a-1, macos]
---

## SPIKE-DSK-001: Verificar en el daemon la firma de la app de escritorio desde su audit token sin romper `unsafe_code = "forbid"`

> **Estado (2026-11-08)**: sin empezar. Reservado desde la tabla "Dependencias de spike" de [ADR-GRP-017](../../../../architecture/decisions/ADR-GRP-017-app-escritorio-cliente-daemon.md) (pregunta 1, A-1). **Alcance: macOS** (Q-DSK-12). Windows y Linux: **Pendiente: etapa de validación multiplataforma**.

**Valor**: sin esta verificación ninguna app tiene identidad firmada y la presencia de sesión no existe (ADR-GRP-017 § 3): cada escritura local desde la app pide el factor por acción y "Deshacer de un toque" no es posible. El spike decide cómo se hace la verificación dentro de las reglas de `unsafe` del workspace antes de que la historia de presencia la implemente.

> Un SPIKE no lleva Dev Spec: su entregable es un Research Brief en `research/SPIKE-DSK-001-resultados.md` y un prototipo aislado en `spikes/app-code-identity/`, fuera del daemon. **Slug**: DSK. **Decisión del orquestador (2026-11-08), validada por Arquitecto**: la verificación vive en el daemon, pero solo existe por la app de escritorio y solo la consumen sus historias; si el resultado sirve también a la identidad del par de otros clientes (Enmienda TS-GRP-004 de ADR-GRP-005, punto 1), el Research Brief lo dice y motor-local lo reutiliza. **Depende de**: nada para el binding; la parte con firma Developer ID real espera la cuenta de Apple Developer (Q-DSK-16, acción de Rene). **Desbloquea**: la presencia de sesión de ADR-GRP-017 § 4 y, con ella, las escrituras locales sin factor por acción de US-DSK-006, 010, 011, 012, 013, 014 y 016; el fin de la identidad por ruta en v0.1.0 (R-6).

### Pregunta

¿Puede el daemon, a partir del **audit token** del par del socket, obtener su código con `SecCodeCopyGuestWithAttributes` y validarlo contra el requisito `anchor apple generic and identifier "<bundle-id>" and certificate leaf[subject.OU] = "<TEAMID>"`, más *hardened runtime* y la ausencia de `get-task-allow`, `disable-library-validation` y `allow-dyld-environment-variables`, con un binding compatible con `unsafe_code = "forbid"`, o hace falta una excepción auditada como `macsys`?

### Hipótesis

- **Binding**: existe un crate mantenido que expone `SecCodeCopyGuestWithAttributes`, `SecRequirementCreateWithString` y `SecCodeCheckValidityWithErrors` con API segura; si no, las llamadas caben en el crate aislado `macsys` de ADR-GRP-002 con unas pocas funciones, sin abrir `unsafe` en el resto del workspace.
- **Audit token**: el token del par que ya obtiene el canal (ADR-GRP-005 § 6.1) sirve como atributo de huésped, y validar por token, no por pid ni por ruta, cierra la carrera de reutilización de pid y el binario reescrito.
- **Flags**: *hardened runtime* y los entitlements se leen de la información de firma del mismo código validado, sin volver a abrir el archivo.
- **Coste**: la verificación tarda menos de 10 ms y cabe al conectar y en cada petición de escritura.
- **Fallos**: un binario con firma ad hoc, una copia en `~/Applications` con `DYLD_INSERT_LIBRARIES`, un bundle modificado tras firmar y un proceso con `get-task-allow` fallan la validación.

### Experimento

- **Inventario de bindings**: comparar los crates candidatos (bindings generados de `Security.framework` y envoltorios de alto nivel) por funciones cubiertas, mantenimiento, licencia y superficie de dependencias (SEC-GRD-15), y medir cuánto `unsafe` queda fuera del crate.
- **Prototipo**: un servidor de socket Unix mínimo que obtiene el audit token del par y valida el requisito, y un cliente empaquetado como `.app` con cuatro firmas: ad hoc, Developer ID sin *hardened runtime*, Developer ID con *hardened runtime* y Developer ID con `get-task-allow`.
- **Ataques**: cliente lanzado con `DYLD_INSERT_LIBRARIES`; bundle firmado al que se le cambia un recurso; binario homónimo en `~/Applications` lanzado con `open -a`; pid reutilizado entre la conexión y la petición; app traslocada por Gatekeeper.
- **Dirección inversa (B-1; Ajuste tras la revisión de seguridad (2026-11-08), validado por security-expert)**: con el mismo binding, la app verifica al daemon del socket por su audit token (`LOCAL_PEERTOKEN`) en vez de por `LOCAL_PEERPID`, que admite reutilización de pid; comprobar que un pid reutilizado entre `connect` y la verificación no pasa.
- **Encaje**: comprobar que la validación se puede llamar desde el hilo del canal sin bloquear el resto de conexiones y medir la latencia en frío y en caliente.
- **Sin cuenta de Apple Developer**: hacer lo anterior con un certificado de desarrollo local y dejar marcado lo que solo se confirma con Developer ID y notarización.

### Criterios de Éxito

- **Decisión de binding** escrita: crate elegido o funciones dentro de `macsys`, con el `unsafe` que queda, dónde y con qué test de frontera; si hace falta una excepción nueva, su texto para ADR-GRP-002.
- Las cuatro firmas y los cinco ataques dan el resultado esperado (solo pasa Developer ID con *hardened runtime* y sin los entitlements prohibidos).
- La app verifica al daemon por audit token con el mismo binding, o el informe dice por qué no (B-1).
- Latencia medida en el Mac de Rene, frente al presupuesto de 10 ms por petición.
- Lista de lo pendiente de confirmar con Developer ID y notarización reales.
- Si la respuesta es "no se puede sin `unsafe` fuera de `macsys`" ni con una excepción aceptable, el Research Brief lo dice y ADR-GRP-017 § 3 queda con la identidad por ruta y el factor por acción también en v0.1.0.

### Time-box

3 días de trabajo, en macOS (Apple silicon). Si se agota sin binding viable, se cierra con el resultado negativo y la degradación del último criterio.
