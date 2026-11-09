---
id: TS-DSK-001
title: "Andamiaje de apps/desktop: proyecto Tauri del monorepo, frontera de seguridad y ventana B1 vacía"
type: ts
status: implemented
feature: desktop
domain: GRP
priority: high
complexity: medium
created: 2026-11-08
updated: 2026-11-08
related:
  context: [CTX-DSK-001]
  adrs: [ADR-GRP-017, ADR-GRP-001, ADR-GRP-002, ADR-GRP-003, ADR-GRP-004, ADR-CKP-003]
  nfrs: [NFR-02, NFR-03, NFR-09, NFR-10, NFR-13]
  stories: [US-DSK-003, US-DSK-004, US-DSK-005, US-DSK-018]
  specs: [DS-TS-DSK-001]
ado:
  id: null
  url: null
tags: [desktop, tauri, react, vite, nx, andamiaje, csp, capabilities, ipc, tokens-web, grafito-glass, i18n, ci, ui-1, dep-dsk-6, dep-dsk-7]
---

## TS-DSK-001 — Andamiaje de apps/desktop: proyecto Tauri del monorepo, frontera de seguridad y ventana B1 vacía

**Valor**: cada historia de la app (US-DSK-001 a 018) solo añade su vista y sus comandos, sobre una base que ya cumple la frontera de ADR-GRP-017 y que el CI comprueba en cada PR.

### Descripción

**Como** Arquitecto
**Quiero** `apps/desktop` creado como proyecto Nx con su backend Tauri en el Cargo workspace, la superficie de seguridad cerrada, el estado del frontend, i18n, los tokens «grafito glass» y la ventana B1 sin datos
**Para** que la app sea un cliente más del canal desde su primer commit (ADR-GRP-017 § 1 y § 2, V1, V2, V5 y V7), con la estructura de ADR-GRP-002 (Enmienda 2026-10-28), ADR-GRP-003 y ADR-GRP-004, y sin que el resto del monorepo pague su compilación

> **Depende de**: ninguna historia. **Habilita**: US-DSK-003 (conexión y estados del motor, que llega con TS-DSK-002) y todas las demás US-DSK. **Decisión del orquestador (2026-11-08), validada por Arquitecto**: el andamiaje no habla con el daemon; la primera llamada al canal la trae TS-DSK-002.

### Alcance Técnico

- **Crear** el proyecto `apps/desktop` (paquete pnpm `@gitraptor/desktop`, Vite + React + TypeScript) y `apps/desktop/src-tauri` (crate `gitraptor-desktop`, Tauri v2), con versiones fijadas.
  - miembro del Cargo workspace, fuera de `default-members`
  - targets Nx `build`, `lint`, `typecheck`, `test`, más `tauri:dev` y `tauri:build`
  - licencia FSL-1.1-ALv2 en los dos manifiestos
- **Limitar** las dependencias del backend a la biblioteca cliente del canal (`gitraptor-api` con su feature de cliente), sin `gitraptor-core`, `gitraptor-git` ni `gitraptor-policy`.
- **Cerrar** la superficie de Tauri de ADR-GRP-017 § 2.
  - capability única de la ventana `main` con solo los comandos propios
  - CSP exacta, endurecimiento, sin red y sin plugins
  - sin esquema de URL, asociación de archivos ni argumentos de línea de órdenes
  - ventana fuera de capturas de pantalla
- **Definir** la lista cerrada de comandos IPC del andamiaje y la convención para que cada historia añada el suyo (parámetros tipados, campos desconocidos rechazados, registro, entrada en la capability y test).
- **Estructurar** el frontend en `src/{app,client,state,ui,i18n,shell,styles}` con TanStack Query para el estado del motor y Zustand para el de la ventana (ADR-GRP-004, Enmienda 2026-10-28).
- **Configurar** i18n en/es con catálogos tipados y la detección del idioma del SO.
- **Generar** la salida CSS de «grafito glass» (claro y oscuro) desde `packages/design-tokens` y consumirla en la app, con el control "generado al día" del workflow `tokens`.
- **Pintar** la ventana B1 sin datos: barra de título, barra lateral, centro y detalle vacíos, con los atajos ⌘B y ⌥⌘B, el cambio de tema y los cortes de anchura de DSF-DSK-001 § 2.4.
- **Añadir** el job de CI `desktop` en ubuntu, filtrado por ruta, y excluir `gitraptor-desktop` de los jobs generales de Rust.
- **Fuera de alcance**: conectar con el daemon o arrancarlo (TS-DSK-002, US-DSK-003); los `externalBin` y el `.dmg` (TS-DSK-002); cualquier dato del motor, el grafo y el saneador SEC-12 compartido (US-DSK-004 y 005); los tipos generados desde `crates/api` (primera historia que los use); TanStack Router (US-DSK-006, primera vista navegable); el buscador ⌘K (US-DSK-018); el updater (v0.1.0); Storybook (UI-2); Windows y Linux en máquina real (**Pendiente: etapa de validación multiplataforma**).

### Plan de Verificación

#### Pruebas Automatizadas

- **V1**: un test lee `cargo metadata` y falla si `gitraptor-desktop` alcanza `gitraptor-core`, `gitraptor-git` o `gitraptor-policy` por cualquier dependencia normal.
- **V2**: un test lee la configuración de Tauri y las capabilities y falla ante un plugin prohibido, una CSP distinta de la exacta, un flag de endurecimiento cambiado, `devUrl` fuera de desarrollo, un esquema de URL o un permiso `core:*` no listado.
- **Lista cerrada**: un test compara los comandos registrados con los permitidos en la capability y con la lista del frontend; cada comando tiene su test de campo desconocido rechazado.
- **V5**: ESLint falla con `dangerouslySetInnerHTML`, `innerHTML`, `insertAdjacentHTML` o una librería de markdown, y con `src/ui` importando Tauri o `src/client`.
- **V7 (parte del andamiaje)**: un test estático falla si el backend lee variables de entorno o enlaza un logger que lea `RUST_LOG`.
- **Tokens**: el control de `tokens` falla con la salida CSS desactualizada; los tests del paquete comprueban los dos temas y que `crates/theme` no cambia.
- **Ventana**: Vitest + Testing Library + axe sobre la ventana: regiones, atajos, tema, i18n en/es y ninguna violación de accesibilidad.
- **CI**: el job `desktop` corre en una PR que toca sus rutas y reporta éxito sin compilar en una que no.

#### Verificación Manual / Sandbox

- `pnpm nx run @gitraptor/desktop:tauri:dev` en macOS: la ventana se abre, ⌘B y ⌥⌘B pliegan los paneles, el tema cambia y persiste al reabrir, y la ventana no aparece en una captura de pantalla.
- Inspector desactivado en un build de release (`tauri:build` sin bundle) en macOS.

Implementado en: PR #327. Pendiente de la verificación manual en macOS: `tauri dev` con atajos, `tauri build` sin inspector y la captura negra de `contentProtected` (la prueba automática salió no concluyente).
