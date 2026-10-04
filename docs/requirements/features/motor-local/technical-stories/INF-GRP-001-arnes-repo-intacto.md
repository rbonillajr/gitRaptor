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
  adrs: [ADR-GRP-009, ADR-GRP-006, ADR-GRP-005, ADR-GRP-012, ADR-GRP-010, ADR-GRP-007]
  stories: [US-GRP-001, US-GRP-002, US-GRP-003, US-GRP-004, US-GRP-005, US-GRP-006, US-GRP-007, US-GRP-008, US-GRP-009, US-GRP-010, US-GRP-011, US-GRP-012, US-GRP-013, US-GRP-014, US-GRP-015, US-GRP-016, TS-GRP-002]
  specs: []
ado:
  id: null
  url: null
tags: [motor-local, ci, arnes, repo-intacto, br-cons-001, nfr-01, seguridad, repo-canario, auditoria-exec]
---

## INF-GRP-001: Arnés de verificación "repo intacto" en los tres SO

**Valor**: ninguna historia del motor se mergea si provoca una sola escritura en el repo o fuera del perfil.

### Descripción

**Como** Arquitecto
**Quiero** un arnés que compare la huella del repo y de la máquina antes y después de observar, como gate de CI en Windows, macOS y Linux
**Para** que BR-CONS-001 y NFR-01 se verifiquen con el criterio binario de la frontera estricta de ADR-GRP-009: cero diferencias imputables al motor

> Dev Spec: `dev-specs/INF-GRP-001-arnes-repo-intacto.md` | Pendiente
>
> **Depende de**: TS-GRP-002. **ADRs**: ADR-GRP-009 (apartado Validación, que la Dev Spec debe seguir punto por punto), ADR-GRP-006 (carpetas del perfil), ADR-GRP-005 (artefactos del autoarranque), ADR-GRP-012 (`~/.claude` intacto). **Seguridad**: SEC-04, SEC-05, SEC-09 y SEC-11 de `docs/architecture/non-functional.md`; el repo canario y la auditoría de `exec` son condición para que ADR-GRP-005 y ADR-GRP-009 pasen a `accepted`.

### Alcance Técnico

- **Crear** el arnés de huella del directorio Git común, de los worktrees enlazados y de cada working tree: rutas, tipo, tamaño, hash de contenido y mtime de archivos y directorios, sin `atime`.
- **Crear** la huella fuera del repo: configuración global y de sistema de Git, `~/.gnupg`, `~/.claude`, otros repos de la máquina y configuración del perfil; solo pueden cambiar los datos del motor en el perfil.
- **Permitir** los artefactos del autoarranque solo en el escenario que ejecuta `raptor daemon enable` (excepción PQ-1 de ADR-GRP-005).
- **Implementar** la ejecución de control: cada escenario corre con y sin motor y solo se imputa al motor la diferencia entre ambas huellas.
- **Implementar** los escenarios mínimos de ADR-GRP-009 (Validación, punto 4), siempre con repos temporales generados y nunca con este repo.
- **Implementar** la auditoría del registro de argv contra la allowlist y la comprobación estática de que solo el módulo de invocación lanza procesos.
- **Crear** el repo canario de SEC-09: filtros `clean`, `textconv`, `core.fsmonitor`, hooks y `gpg.program` que apuntan a un script que deja un marcador.
- **Implementar** la auditoría dinámica de `exec` por SO (eslogger en macOS, ETW en Windows, strace en Linux): todo proceso hijo del motor pertenece a la allowlist y `gix` nunca lanza `git`.
- **Implementar** los tests de SEC-04 (archivos hostiles en `~/.claude`, canario de prompt) y de SEC-05 (secretos plantados con gitleaks o trufflehog sobre perfil, logs y captura del stream IPC).
- **Implementar** los escenarios de SEC-11: `gitdir` manipulado, repo de otro propietario, `.gitraptor/settings.json` como symlink a un secreto y ruta UNC.
- **Configurar** el arnés como gate de CI en runners de Windows, macOS y Linux que bloquea el merge de cualquier historia del motor.
- **Ubicar** el arnés en las pruebas del workspace de Cargo sin crear un crate fuera de ADR-GRP-002; la ruta exacta la fija la Dev Spec.
- **Fuera de alcance**: el banco de frescura y escala (INF-GRP-002); la base de CI del monorepo, salvo que no exista (⚠️ **ASSUMPTION**: viene del spike del stack; si no, este INF la provisiona).

### Plan de Verificación

#### Pruebas Automatizadas

- **Sensibilidad**: una build de prueba que crea y borra un lock en el directorio Git durante una lectura hace fallar el arnés; otra que escribe un archivo fuera del perfil también.
- **Control**: un escenario con el daemon fsmonitor del usuario ya arrancado y un `gc --auto` provocado por un commit del agente no se imputa al motor.
- **Escenarios**: worktrees enlazados; rebase y merge en curso; HEAD separado; fsmonitor, untracked cache y split index activados; hooks presentes; LFS con filtros; firmas con `log.showSignature`; destino de trace2; `gc` concurrente; repo rechazado por `safe.directory`. Todos en verde en los tres SO.
- **Repo canario (SEC-09)**: con archivos de stat sucio y el motor observando, el marcador nunca aparece.
- **Auditoría de `exec`**: en los tres SO, la traza de procesos hijos solo contiene argv de la allowlist; ningún `git` lanzado por `gix`.
- **`~/.claude` hostil (SEC-04)**: línea de 100 MB, symlink a `/dev/zero` y FIFO no bloquean ni tumban el daemon; hash de `~/.claude` idéntico; el canario de `claude -p` y del transcript no aparece en perfil, logs ni stream IPC.
- **Secretos (SEC-05)**: `.env`, token en la URL del remoto y `http.extraHeader` plantados; 0 hallazgos del escáner.
- **Rutas no confiables (SEC-11)**: `gitdir` hacia `$HOME` no se vigila; repo de otro uid queda "no disponible"; settings como symlink a `~/.ssh/id_rsa` da diagnóstico sin contenido; ruta UNC sin conexiones SMB.
- **Autoarranque**: `raptor daemon enable` y `disable` crean y eliminan exactamente los artefactos de la tabla de ADR-GRP-005 § 3, y nada más fuera del perfil.
- **Guarda**: el arnés se niega a ejecutarse sobre una ruta dentro del repo de GitRaptor.
- **Gate**: un PR con un efecto imputable al motor queda bloqueado en CI.

#### Verificación Manual / Sandbox

- Revisar el informe de diferencias de una ejecución fallida provocada a propósito y comprobar que nombra el escenario, la ruta y el tipo de cambio.
