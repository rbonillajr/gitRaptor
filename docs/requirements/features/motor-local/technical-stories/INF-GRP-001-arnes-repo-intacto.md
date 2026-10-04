---
id: INF-GRP-001
title: "Arnés de verificación \"repo intacto\" en los tres SO"
type: inf
status: ready
feature: motor-local
domain: GRP
priority: critical
complexity: medium
created: 2026-10-03
updated: 2026-10-04
related:
  adrs: [ADR-GRP-009, ADR-GRP-006, ADR-GRP-005, ADR-GRP-012, ADR-GRP-010, ADR-GRP-007, ADR-GRD-001]
  stories: [US-GRP-001, US-GRP-002, US-GRP-003, US-GRP-004, US-GRP-005, US-GRP-006, US-GRP-007, US-GRP-008, US-GRP-009, US-GRP-010, US-GRP-011, US-GRP-012, US-GRP-013, US-GRP-014, US-GRP-015, US-GRP-016, TS-GRP-002, TS-GRP-003, TS-GRP-004]
  depends_on: [TS-GRP-002]
  suites_with: [TS-GRP-003, TS-GRP-004, US-GRP-002, US-GRP-004, US-GRP-007]
  specs: [DS-INF-GRP-001]
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

> Dev Spec: [`dev-specs/INF-GRP-001-dev-spec.md`](../dev-specs/INF-GRP-001-dev-spec.md) | Núcleo implementado (rama `feat/INF-GRP-001-intact-repo-harness`). Se cierra con la primera ejecución verde del gate en los tres SO (decisión del orquestador, 2026-10-04, validada por PO)
>
> **Depende de**: TS-GRP-002 (solo el núcleo). Las suites incrementales no son dependencias de este INF: cada una entra con su historia dueña (ver "Estructura"). **ADRs**: ADR-GRP-009 (apartado Validación, que la Dev Spec debe seguir punto por punto), ADR-GRP-006 (carpetas del perfil), ADR-GRP-005 (artefactos del autoarranque), ADR-GRP-012 (`~/.claude` intacto). **Seguridad**: SEC-04, SEC-05, SEC-09 y SEC-11 de `docs/architecture/non-functional.md`; el repo canario y la auditoría de `exec` son condición para que ADR-GRP-005 y ADR-GRP-009 pasen a `accepted`.

### Estructura: núcleo y suites incrementales

Para no crear una dependencia circular (el arnés no puede esperar a las historias cuyo merge bloquea), el INF se divide en:

- **Núcleo** (este INF, depende solo de TS-GRP-002): huella del repo y de fuera del repo, ejecución de control, allowlist de argv y guarda. Se ejercita sobre la capa de lectura de TS-GRP-002, sin daemon.
- **Suites incrementales**: cada una se escribe y entra **con su historia dueña**, dentro de su Dev Spec, sobre el núcleo ya existente:

| Suite | Historia dueña | Qué añade |
|---|---|---|
| Proceso | TS-GRP-003 | Bloqueo, logs y estado solo en el perfil; entorno del daemon (SEC-10); secretos plantados sobre perfil y logs (SEC-05) |
| Canal | TS-GRP-004 | Socket o pipe dentro del perfil; ruta UNC sin conexiones SMB (SEC-11); secretos y canarios ausentes del stream IPC (SEC-05) |
| Observación | US-GRP-002 | Escenarios con el observador en marcha: worktrees enlazados, rebase y merge en curso, fsmonitor, `gc` concurrente; repo canario (SEC-09) y auditoría dinámica de `exec` con el motor observando |
| Continuidad y autoarranque | US-GRP-004 | `raptor daemon enable` y `disable` (excepción PQ-1); observación sin clientes y tras suspensión |
| `~/.claude` | US-GRP-007 | Archivos hostiles en `~/.claude` y canario de prompt (SEC-04) |

**Bloqueo de merge**: el núcleo bloquea el merge de toda historia del motor desde que existe. Cada suite bloquea **solo el merge de su historia dueña** y, una vez dentro, el de las posteriores como regresión; nunca el de una historia anterior que no la necesita.

### Alcance Técnico (núcleo)

- **Crear** el arnés de huella del directorio Git común, de los worktrees enlazados y de cada working tree: rutas, tipo, tamaño, hash de contenido y mtime de archivos y directorios, sin `atime`.
- **Crear** la huella fuera del repo: configuración global y de sistema de Git, `~/.gnupg`, `~/.claude`, otros repos de la máquina y configuración del perfil; solo pueden cambiar los datos del motor en el perfil.
- **Prever** una lista de excepciones por escenario, que la suite de US-GRP-004 usa para permitir los artefactos del autoarranque solo cuando ejecuta `raptor daemon enable` (excepción PQ-1 de ADR-GRP-005).
- **Admitir** en esa lista la excepción de Guardrails (Enmienda 2026-10-04, ADR-GRD-001 § 7): tras una instalación explícita, solo la clave local de hooks y la carpeta de Guardrails del directorio común; tras desinstalar, cero diferencias. La usa INF-GRD-001.
- **Implementar** la ejecución de control: cada escenario corre con y sin motor y solo se imputa al motor la diferencia entre ambas huellas.
- **Implementar** los escenarios mínimos de ADR-GRP-009 (Validación, punto 4) que se pueden ejercitar sobre la capa de lectura, siempre con repos temporales generados y nunca con este repo. Los que necesitan el observador van en la suite de US-GRP-002.
- **Implementar** la auditoría del registro de argv contra la allowlist y la comprobación estática de que solo el módulo de invocación lanza procesos.
- **Crear** el repo canario de SEC-09 (filtros `clean`, `textconv`, `core.fsmonitor`, hooks y `gpg.program` que apuntan a un script que deja un marcador) y ejercitarlo con la capa de lectura.
- **Implementar** la auditoría dinámica de `exec` sobre la capa de lectura: todo proceso hijo pertenece a la allowlist y `gix` nunca lanza `git`. Las suites la reutilizan. **Enmienda 2026-10-04** (decisión del orquestador, validada por el Arquitecto): el gate portable de los tres SO es una auditoría por trampas, sin privilegios (shim de `git` y trampas en el `PATH`, que son copias del binario de test). eslogger (macOS), strace (Linux) y ETW (Windows, pendiente) quedan como auditoría profunda.
- **Implementar** los escenarios de SEC-11 que no necesitan el canal: `gitdir` manipulado, repo de otro propietario y `.gitraptor/settings.json` como symlink a un secreto.
- **Configurar** el núcleo como gate de CI en runners de Windows, macOS y Linux y el mecanismo para que cada suite se registre como gate de su historia dueña.
- **Fuera del núcleo**: las suites de la tabla "Estructura", que entran con TS-GRP-003, TS-GRP-004, US-GRP-002, US-GRP-004 y US-GRP-007.
- **Ubicar** el arnés en el crate de soporte de pruebas `crates/testkit` (`gitraptor-testkit`), que solo se consume como `[dev-dependencies]` y que ADR-GRP-002 incorpora a su estructura (**enmienda 2026-10-04**: decisión del orquestador, validada por el Arquitecto; sustituye a "sin crear un crate fuera de ADR-GRP-002"). Las suites se registran nombrando sus tests `repo_intact::…`; el detalle está en la Dev Spec.
- **Fuera de alcance**: el banco de frescura y escala (INF-GRP-002); la base de CI del monorepo, salvo que no exista (⚠️ **ASSUMPTION**: viene del spike del stack; si no, este INF la provisiona).

### Plan de Verificación

#### Pruebas Automatizadas

Las del núcleo se cumplen al cerrar este INF; las marcadas con la suite se cumplen al cerrar su historia dueña.

- **Sensibilidad**: una build de prueba que crea y borra un lock en el directorio Git durante una lectura hace fallar el arnés; otra que escribe un archivo fuera del perfil también.
- **Control**: un escenario con el daemon fsmonitor del usuario ya arrancado y un `gc --auto` provocado por un commit del agente no se imputa al motor.
- **Escenarios** (núcleo sobre la capa de lectura; con el observador, suite de US-GRP-002): worktrees enlazados; rebase y merge en curso; HEAD separado; fsmonitor, untracked cache y split index activados; hooks presentes; LFS con filtros; firmas con `log.showSignature`; destino de trace2; `gc` concurrente; repo rechazado por `safe.directory`. Todos en verde en los tres SO.
- **Repo canario (SEC-09)**: con archivos de stat sucio, en lecturas (núcleo) y con el motor observando (suite de US-GRP-002), el marcador nunca aparece.
- **Auditoría de `exec`**: en los tres SO, la traza de procesos hijos solo contiene argv de la allowlist; ningún `git` lanzado por `gix`.
- **`~/.claude` hostil (SEC-04, suite de US-GRP-007)**: línea de 100 MB, symlink a `/dev/zero` y FIFO no bloquean ni tumban el daemon; hash de `~/.claude` idéntico; el canario de `claude -p` y del transcript no aparece en perfil, logs ni stream IPC.
- **Secretos (SEC-05, suites de TS-GRP-003 y TS-GRP-004)**: `.env`, token en la URL del remoto y `http.extraHeader` plantados; 0 hallazgos del escáner.
- **Rutas no confiables (SEC-11)**: `gitdir` hacia `$HOME` no se vigila; repo de otro uid queda "no disponible"; settings como symlink a `~/.ssh/id_rsa` da diagnóstico sin contenido; ruta UNC sin conexiones SMB (suite de TS-GRP-004).
- **Autoarranque (suite de US-GRP-004)**: `raptor daemon enable` y `disable` crean y eliminan exactamente los artefactos de la tabla de ADR-GRP-005 § 3, y nada más fuera del perfil.
- **Excepción de Guardrails (suites de INF-GRD-001)**: tras una instalación explícita, la huella solo difiere en la clave local de hooks y en la carpeta de Guardrails; cualquier otra diferencia falla. Tras desinstalar, cero diferencias. Fuera de esos escenarios, la excepción no aplica.
- **Guarda**: el arnés se niega a ejecutarse sobre una ruta dentro del repo de GitRaptor.
- **Gate**: un PR con un efecto imputable al motor queda bloqueado en CI por el núcleo o por la suite de su historia; una historia anterior no queda bloqueada por una suite que todavía no existe.

#### Verificación Manual / Sandbox

- Revisar el informe de diferencias de una ejecución fallida provocada a propósito y comprobar que nombra el escenario, la ruta y el tipo de cambio.

### Notas de integración

Nota de integración (Time Machine, INF-TMC-001): la huella 'repo intacto' y el repo canario se reutilizan desde INF-TMC-001. El canario se amplía con los casos de SEC-TMC-02: `core.fsmonitor`, `core.worktree` hacia fuera del repo, `includeIf` hostil, `filter.*`, `commit.gpgSign` global e `init.templateDir` con hooks.
