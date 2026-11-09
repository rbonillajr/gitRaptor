---
id: TS-DSK-002
title: "Arranque del motor desde la app y binarios del motor dentro del bundle"
type: ts
status: ready
feature: desktop
domain: GRP
priority: high
complexity: medium
created: 2026-11-08
updated: 2026-11-08
related:
  context: [CTX-DSK-001]
  adrs: [ADR-GRP-014, ADR-GRP-017, ADR-GRP-005, ADR-CKP-003, ADR-GRP-016]
  nfrs: [NFR-02, NFR-03, NFR-06, SEC-10, SEC-13, SEC-14]
  stories: [US-DSK-003, US-GRP-004, TS-DSK-001]
  specs: []
ado:
  id: null
  url: null
tags: [desktop, dep-dsk-5, revision-seguridad, arranque-bajo-demanda, externalbin, sidecar, dmg, alpha, autoarranque, sec-13, sec-14, b-3]
---

## TS-DSK-002 — Arranque del motor desde la app y binarios del motor dentro del bundle

**Valor**: un usuario sin terminal instala la app, la abre y tiene el motor en marcha, con la misma versión que la app y sin que la app reemplace nunca el motor de otra instalación.

### Descripción

**Como** Arquitecto
**Quiero** que la app arranque el motor con el `raptor` de su propio bundle, por la vía del arranque bajo demanda y sin enlazar el motor, y que la alpha semanal publique el `.dmg` con `raptor` y `raptor-mcp` dentro
**Para** cerrar DEP-DSK-5 con las reglas de [ADR-GRP-014, Enmienda 2026-11-08 (DEP-DSK-5)](../../../../architecture/decisions/ADR-GRP-014-pipeline-release-distribucion.md#enmienda-2026-11-08-dep-dsk-5-arranque-del-motor-desde-la-app), ADR-GRP-017 § 1 y § 6 (V1 y B-3), ADR-GRP-005 § 3 (SEC-10) y SEC-13

> **Depende de**: TS-DSK-001 (proyecto y job `desktop`). **Habilita**: US-DSK-003 (estados del motor, "Arrancar motor", "Cómo actualizar" y el autoarranque desde Ajustes) y, a través de ella, todas las US-DSK. **Decisión del orquestador (2026-11-08), validada por Arquitecto**: el trabajo no tiene una sola historia dueña con resultado observable (la app lo usa entera y el pipeline de release no es de ninguna historia), así que va como enabler; las pantallas son de US-DSK-003.

### Alcance Técnico

- **Añadir** a la CLI el subcomando de diagnóstico del arranque para clientes sin terminal (`raptor daemon ensure`), que arranca el motor si no corre, solo reemplaza un daemon lanzado desde su misma ruta y devuelve en una línea JSON dónde está el canal, quién lo sirve y el estado del autoarranque.
- **Implementar** en `apps/desktop/src-tauri` el lanzador de la app: localiza el `raptor` del bundle junto al ejecutable, lo verifica y lo ejecuta con argv y entorno fijos, sin plugin `shell`, como implementación del punto de arranque de la biblioteca cliente.
  - entorno fijo también en debug: el perfil de prueba llega por el `Launch` inyectado en los tests, nunca por el entorno de la app (M-4, Ajuste tras la revisión de seguridad (2026-11-08), validado por security-expert)
- **Implementar** la verificación del daemon antes del handshake (B-3 de ADR-GRP-017): mismo usuario y ejecutable `raptor` instalado.
  - en la biblioteca cliente del canal y en el crate del SO con `unsafe` acotado, nunca en `gitraptor-desktop` (B-1, Ajuste tras la revisión de seguridad (2026-11-08), validado por security-expert)
  - límite: reutilización del pid entre la lectura del par y la resolución de su ruta; la verificación por *audit token* la decide SPIKE-DSK-001
- **Exponer** al webview el resultado del arranque y de la compatibilidad de versiones como estados tipados, con comandos de la lista cerrada (convención de TS-DSK-001) para "Arrancar motor" y para registrar el autoarranque con el `raptor` del bundle.
  - el autoarranque se registra o se quita solo tras confirmarlo el humano en un diálogo nativo que abre el backend (B-2, Ajuste tras la revisión de seguridad (2026-11-08), validado por security-expert)
- **Configurar** los binarios del motor como `externalBin` del bundle solo en el build de release, con el nombre por target triple que exige Tauri.
- **Añadir** a `release.yml` el job que construye `GitRaptor-<versión>-aarch64-apple-darwin.dmg` con los binarios del archivo `aarch64-apple-darwin` de la misma corrida, y a `select-assets.sh` el `.dmg` de las alphas.
- **Fuera de alcance**: las pantallas y textos de los estados (US-DSK-003); la firma Developer ID, la notarización, el cask y el updater (v0.1.0, ADR-GRP-014 Enmienda 2026-11-08); la clase de cliente `desktop` y la identidad de la app ante el daemon (DEP-DSK-2, spike de `SecCode`); Intel, Windows y Linux (**Pendiente: etapa de validación multiplataforma**).

### Plan de Verificación

#### Pruebas Automatizadas

- **CLI**: tests de `raptor daemon ensure` sobre un perfil temporal: motor parado (lo arranca), motor en marcha (no lanza nada), motor antiguo de su misma ruta (lo reemplaza), motor antiguo de otra ruta (no lo reemplaza y lo informa), protocolo más nuevo (incompatible), y forma exacta de la línea JSON.
- **Lanzador**: tests de `src-tauri` con un `raptor` falso: argv exacto, entorno vacío salvo la lista fija, binario ausente, binario escribible por otros o fuera de la carpeta del ejecutable (rechazado) y salida no JSON (error tipado).
- **B-3**: canal servido por un ejecutable que no es `raptor`, o de otro usuario, rechazado sin handshake.
- **V1 sigue verde**: el backend no enlaza `gitraptor-core` tras añadir el lanzador, y no contiene `unsafe` (B-1).
- **Entorno**: el lanzador pasa el mismo entorno en debug y en release (M-4).
- **Confirmación**: con el diálogo simulado en "No", el comando de autoarranque no ejecuta `raptor` (B-2).
- **Release**: `select-assets.test.sh` cubre el `.dmg` de la alpha; el job comprueba que el `.dmg` contiene `raptor` y `raptor-mcp` con la versión del tag.

#### Verificación Manual / Sandbox

- En macOS, con la alpha instalada en `/Applications` y sin otro `raptor`: abrir la app arranca el motor y `raptor daemon status` del bundle lo muestra.
- Con un `raptor` de Homebrew o de `install.sh` sirviendo un daemon de versión anterior: la app no lo reemplaza y muestra la acción para actualizarlo.
- App abierta desde la imagen montada o con traslocación de Gatekeeper: el autoarranque no se ofrece.
