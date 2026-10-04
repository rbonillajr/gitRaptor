---
id: INF-GRP-001
title: "Arnés de verificación \"repo intacto\" en los tres SO"
type: inf
status: Dev Spec Pending
feature: motor-local
domain: GRP
priority: critical
complexity: medium
created: 2026-10-03
updated: 2026-10-03
related:
  adrs: [ADR-GRP-009, ADR-GRP-006, ADR-GRP-005, ADR-GRP-012]
  stories: [US-GRP-001, US-GRP-002, US-GRP-003, US-GRP-004, US-GRP-005, US-GRP-006, US-GRP-007, US-GRP-008, US-GRP-009, US-GRP-010, US-GRP-011, US-GRP-012, US-GRP-013, US-GRP-014, US-GRP-015, US-GRP-016, TS-GRP-002]
  specs: []
ado:
  id: null
  url: null
tags: [motor-local, ci, arnes, repo-intacto, br-cons-001, nfr-01]
---

## INF-GRP-001: Arnés de verificación "repo intacto" en los tres SO

**Valor**: ninguna historia del motor se mergea si provoca una sola escritura en el repo o fuera del perfil.

### Descripción

**Como** Arquitecto
**Quiero** un arnés que compare la huella del repo y de la máquina antes y después de observar, como gate de CI en Windows, macOS y Linux
**Para** que BR-CONS-001 y NFR-01 se verifiquen con el criterio binario de la frontera estricta de ADR-GRP-009: cero diferencias imputables al motor

> Dev Spec: `dev-specs/INF-GRP-001-arnes-repo-intacto.md` | Pendiente
>
> **Depende de**: TS-GRP-002. **ADRs**: ADR-GRP-009 (apartado Validación, que la Dev Spec debe seguir punto por punto), ADR-GRP-006 (carpetas del perfil), ADR-GRP-005 (artefactos del autoarranque), ADR-GRP-012 (`~/.claude` intacto).

### Alcance Técnico

- **Crear** el arnés de huella del directorio Git común, de los worktrees enlazados y de cada working tree: rutas, tipo, tamaño, hash de contenido y mtime de archivos y directorios, sin `atime`.
- **Crear** la huella fuera del repo: configuración global y de sistema de Git, `~/.gnupg`, `~/.claude`, otros repos de la máquina y configuración del perfil; solo pueden cambiar los datos del motor en el perfil.
- **Permitir** los artefactos del autoarranque solo en el escenario que ejecuta `raptor daemon enable` (excepción PQ-1 de ADR-GRP-005).
- **Implementar** la ejecución de control: cada escenario corre con y sin motor y solo se imputa al motor la diferencia entre ambas huellas.
- **Implementar** los escenarios mínimos de ADR-GRP-009 (Validación, punto 4), siempre con repos temporales generados y nunca con este repo.
- **Implementar** la auditoría del registro de argv contra la allowlist y la comprobación estática de que solo el módulo de invocación lanza procesos.
- **Configurar** el arnés como gate de CI en runners de Windows, macOS y Linux que bloquea el merge de cualquier historia del motor.
- **Ubicar** el arnés en las pruebas del workspace de Cargo sin crear un crate fuera de ADR-GRP-002; la ruta exacta la fija la Dev Spec.
- **Fuera de alcance**: el banco de frescura y escala (INF-GRP-002); la base de CI del monorepo, salvo que no exista (⚠️ **ASSUMPTION**: viene del spike del stack; si no, este INF la provisiona).

### Plan de Verificación

#### Pruebas Automatizadas

- **Sensibilidad**: una build de prueba que crea y borra un lock en el directorio Git durante una lectura hace fallar el arnés; otra que escribe un archivo fuera del perfil también.
- **Control**: un escenario con el daemon fsmonitor del usuario ya arrancado y un `gc --auto` provocado por un commit del agente no se imputa al motor.
- **Escenarios**: worktrees enlazados; rebase y merge en curso; HEAD separado; fsmonitor, untracked cache y split index activados; hooks presentes; LFS con filtros; firmas con `log.showSignature`; destino de trace2; `gc` concurrente; repo rechazado por `safe.directory`. Todos en verde en los tres SO.
- **Autoarranque**: `raptor daemon enable` y `disable` crean y eliminan exactamente los artefactos de la tabla de ADR-GRP-005 § 3, y nada más fuera del perfil.
- **Guarda**: el arnés se niega a ejecutarse sobre una ruta dentro del repo de GitRaptor.
- **Gate**: un PR con un efecto imputable al motor queda bloqueado en CI.

#### Verificación Manual / Sandbox

- Revisar el informe de diferencias de una ejecución fallida provocada a propósito y comprobar que nombra el escenario, la ruta y el tipo de cambio.
