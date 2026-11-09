---
mode: bulk
status: expanded
generated: 2026-11-08
updated: 2026-11-09
generator: architect
domain: GRP
feature: desktop
total_artifacts: 5
expanded: 5
approved: 0
related:
  context: [CTX-DSK-001]
  adrs: [ADR-GRP-017, ADR-GRP-014, ADR-GRP-002, ADR-GRP-003, ADR-GRP-004]
---

# Technical Stories — INDEX: App de escritorio

> Índice. Cada historia vive en su archivo, dentro de [`technical-stories/`](./technical-stories/).

## Índice

| ID | Tipo | Título | Valor (1 línea) | ADR | Habilita | Depende de | Complejidad | Status |
|----|------|--------|-----------------|-----|----------|-----------|-------------|--------|
| [TS-DSK-001](./technical-stories/TS-DSK-001-andamiaje-app-escritorio.md) | TS | Andamiaje de apps/desktop: proyecto Tauri del monorepo, frontera de seguridad y ventana B1 vacía | Cada historia de la app solo añade su vista y sus comandos sobre una base que ya cumple ADR-GRP-017 (V1, V2, V5, V7) y que el CI comprueba | ADR-GRP-017, ADR-GRP-002, ADR-GRP-003, ADR-GRP-004 | Todas las US-DSK; TS-DSK-002 | — | Medium | implemented (PR #327); [Dev Spec](./dev-specs/TS-DSK-001-andamiaje-app-escritorio.md) |
| [TS-DSK-002](./technical-stories/TS-DSK-002-arranque-motor-binarios-bundle.md) | TS | Arranque del motor desde la app y binarios del motor dentro del bundle | La app arranca el motor con el `raptor` de su bundle, nunca reemplaza el de otra instalación, y la alpha publica el `.dmg` (DEP-DSK-5) | ADR-GRP-014 (Enmienda 2026-11-08, DEP-DSK-5), ADR-GRP-017 § 6 | US-DSK-003 (y con ella todas las US-DSK) | TS-DSK-001 | Medium | ready; [Dev Spec](./dev-specs/TS-DSK-002-arranque-motor-binarios-bundle.md) (spec-ready) |
| [TS-DSK-003](./technical-stories/TS-DSK-003-primitivas-design-system-escritorio.md) | TS | Primitivas del design system de la app de escritorio y catálogo de desarrollo | Las pantallas de UI-1 se componen con 14 primitivas accesibles, en claro y oscuro y en en/es, sin dependencias de UI nuevas; el catálogo solo existe en desarrollo | ADR-GRP-003 (Enmienda 2026-11-09), ADR-GRP-004, ADR-GRP-017 | US-DSK-003 a US-DSK-008 | TS-DSK-001 | Medium | implemented; [Dev Spec](./dev-specs/TS-DSK-003-primitivas-design-system-escritorio.md) (spec-ready) |
| [SPIKE-DSK-001](./technical-stories/SPIKE-DSK-001-identidad-firmada-app.md) | SPIKE | Identidad firmada de la app verificada por el daemon sin romper `unsafe_code = forbid` | Sin él no hay presencia de sesión: cada escritura desde la app pide Touch ID | ADR-GRP-017 § 3 (A-1) | Presencia de sesión: US-DSK-006, 010 a 014, 016 | — | Medium | Research Pending |
| [SPIKE-DSK-002](./technical-stories/SPIKE-DSK-002-lanzamiento-por-agente.md) | SPIKE | Detectar un `open -a` lanzado por un agente (`oapp`, `keySenderPIDAttr`) | Cierra o acepta el hueco R-6 de ADR-GRP-017 | ADR-GRP-017 § 3 (R-6) | Perfil de la app lanzada por un agente | — | Medium | Research Pending |
