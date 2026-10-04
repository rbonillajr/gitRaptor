---
id: ADR-GRP-006
title: Perfil de GitRaptor — ubicación por SO, clave de repo y almacenamiento
type: adr
status: proposed
date: 2026-10-03
created: 2026-10-03
updated: 2026-10-03
deciders: [Rene Bonilla]
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-007, ADR-GRP-008, ADR-GRP-013, CTX-GRP-001, BR-GRP-001]
tags: [motor-local, perfil, sqlite, rusqlite, directories, almacenamiento, clave-de-repo, p9, privacidad, seguridad, permisos]
---

# ADR-GRP-006 — Perfil de GitRaptor: ubicación por SO, clave de repo y almacenamiento

## Contexto

Todos los datos propios del motor (lista de repos observados, registro de agentes, atribuciones, historial de eventos y estado de sesiones) viven en el **perfil de GitRaptor**, separados por repo (Q21, tabla de BR-CONS-001). Fuera del repo, el perfil es lo único que el motor escribe (Q17). El requerimiento deja abierta la pregunta P9: dónde vive el perfil en cada SO y cómo se separan los datos de cada repo.

Hechos que condicionan la decisión:

- Retirar un repo no borra sus datos y volver a añadirlo los recupera (Q25, BR-AUTH-001).
- Si el perfil se pierde, el motor sigue funcionando y lo no observado queda "sin atribuir" (Q26, BR-EDGE-005). R13 acepta que el perfil concentre los datos de todos los repos.
- Los datos son de cada máquina y no viajan (Q31, BR-EDGE-007).
- Una corrección reatribuye todos los eventos de una sesión desde su inicio (Q37), así que el almacén tiene que permitir consultas por sesión y actualizaciones transaccionales (ADR-GRP-013).
- Escala de NFR-05: 10 o más worktrees y repos de más de 100K commits.
- Privacidad (NFR-03) y licencias aptas (NFR-11).
- El único escritor del perfil es el daemon (ADR-GRP-005).

## Decisión

### 1. Ubicación: carpeta estándar de cada SO

Se usa el crate `directories` para resolver las carpetas estándar del usuario (decisión de Rene Bonilla, 2026-10-03, PQ-4; no `~/.gitraptor`). El perfil es el **conjunto de carpetas exclusivas de GitRaptor** en esas ubicaciones:

| Carpeta del perfil | macOS | Linux | Windows |
|--------------------|-------|-------|---------|
| Datos (almacén) | `~/Library/Application Support/<app>` | `$XDG_DATA_HOME/<app>` | `%LOCALAPPDATA%\<app>\data` |
| Configuración de nivel perfil (solo lectura para el motor) | `~/Library/Application Support/<app>` | `$XDG_CONFIG_HOME/<app>` | `%LOCALAPPDATA%\<app>\config` |
| Estado (bloqueo de instancia, logs) | `~/Library/Application Support/<app>` | `$XDG_STATE_HOME/<app>` | `%LOCALAPPDATA%\<app>\state` |
| Ejecución (socket del canal) | `~/Library/Application Support/<app>` | `$XDG_RUNTIME_DIR/<app>` (si no existe, la de estado) | No aplica (named pipe) |

- **Windows siempre en `%LOCALAPPDATA%`**, nunca en `%APPDATA%` (roaming), ni para datos ni para configuración (decisión de Rene Bonilla, 2026-10-03, PQ-7). Se usan explícitamente las variantes locales del crate, porque la carpeta de configuración por defecto de Windows es la roaming.
- `<app>` es un identificador corto y estable (lo fija TS-GRP-001), para no superar el límite de la ruta del socket en macOS (ADR-GRP-005).
- **Sobreescritura para pruebas**: una variable de entorno (nombre provisional `GITRAPTOR_PROFILE_DIR`) sustituye todas las carpetas por subcarpetas de una sola raíz. Los tests la usan siempre con un directorio temporal. **Solo existe en builds de test**: el binario release la ignora, para que un proceso que controla el entorno no pueda redirigir el perfil (SEC-06, H4).
- **Permisos (SEC-06)**: carpetas 0700 y archivos 0600 en macOS y Linux, **incluidos `-wal`, `-shm`, logs, lock y archivos en cuarentena**, creados con umask restrictiva (077) para que no exista una ventana con permisos abiertos. Al arrancar, el daemon **verifica propietario y modo** de las carpetas preexistentes y no arranca si no cuadran (no las "arregla"). En Windows, se comprueba que la ACL heredada de `%LOCALAPPDATA%` no tiene ACE para otros usuarios.
- **Rutas**: las rutas que llegan al perfil (repos, worktrees) se validan antes de tocar el FS; en Windows se rechazan UNC, `\\?\`, dispositivos y ADS (SEC-02, M9). Ningún dato del motor se escribe fuera del perfil.

### 2. Organización dentro del perfil

- **Índice global** (un archivo en la carpeta de datos): repos observados con su clave, ruta canónica, estado (observado o retirado), fechas de alta y de retiro, y la pista del commit raíz.
- **Un almacén por repo** (un archivo por clave de repo en la carpeta de datos): worktrees, sesiones, registros de atribución, eventos, huecos y último estado conocido, según el modelo de ADR-GRP-013.
- **Configuración**: el nivel perfil vive en la carpeta de configuración (formato en ADR-GRP-007). La **configuración local personal de cada repo también vive en el perfil, indexada por la clave de repo** (decisión de Rene Bonilla, 2026-10-03, PQ-3; el detalle es de ADR-GRP-008). El motor solo la lee (Q23).

### 3. Clave de repo

- Al añadir un repo se genera un **identificador opaco** (UUID) y se indexa por la **ruta canónica del directorio Git común**. Todos los worktrees de un repo comparten ese directorio y, por tanto, la clave.
- La ruta canónica resuelve enlaces simbólicos y, en sistemas de archivos que no distinguen mayúsculas (macOS, Windows), se normaliza para que dos grafías de la misma ruta den la misma clave.
- Se guarda el **commit raíz como pista** (vacío si el repo no tiene commits), sin usarlo como clave.
- **Volver a añadir** un repo retirado encuentra la misma clave por su ruta y recupera sus datos (Q25). El intervalo retirado se registra como hueco (ADR-GRP-013).
- Dentro del almacén del repo, cada worktree se identifica por su ruta canónica y su nombre administrativo en Git.

### 4. Almacenamiento: SQLite embebido

- **SQLite vía `rusqlite`**, con SQLite compilado dentro del binario (sin depender de la versión del SO), en modo WAL y con sincronización completa. **SQL siempre parametrizado** (SEC-06). Como el SQLite embebido no se actualiza con el SO, sus avisos de seguridad se siguen en la cadena de suministro (`cargo-deny`/`cargo-audit`, SEC-07, L3). Las escrituras de un mismo lote de debounce van en una sola transacción, para no superar el presupuesto de persistencia de ADR-GRP-011.
- **Un único escritor**: el daemon (ADR-GRP-005). Los clientes nunca abren los archivos del perfil; consultan por el canal.
- **Versión de esquema** en cada archivo, con migraciones incluidas en el binario. Si un archivo tiene un esquema más nuevo que el binario, ese repo no se observa y el motor expone un diagnóstico; no se degrada el archivo.
- **Integridad**: comprobación rápida al abrir. Un archivo corrupto se aparta dentro del perfil (renombrado con marca de tiempo, sin borrarlo) y ese repo se trata como **perfil perdido** (Q26): almacén nuevo y todo lo anterior "sin atribuir". La corrupción de un repo no afecta a los demás.
- **Contenido**: solo metadatos (rutas, refs, ids de commit, horas, agentes, señales de atribución). Nunca contenido de archivos del usuario, prompts ni diffs (NFR-03).
- **Licencias** (NFR-11): SQLite es de dominio público; `rusqlite` y `directories` son MIT o MIT/Apache-2.0.

## Alternativas consideradas

| Eje | Alternativa | Por qué no |
|-----|-------------|------------|
| Ubicación | Carpeta única `~/.gitraptor` en los tres SO | Fácil de encontrar, pero no sigue XDG ni las convenciones de macOS y Windows. Rechazada por PQ-4. |
| Ubicación | `%APPDATA%` (roaming) en Windows | Con perfiles móviles de dominio, los datos y la configuración personal viajarían a otra máquina, en contra de Q31. Rechazada por PQ-7. |
| Clave | Ruta canónica como clave directa | Mezcla identidad y ubicación; cualquier cambio de política futura (p. ej. reenlazar un repo movido) obliga a reescribir claves. |
| Clave | Id del commit raíz | Falla en repos vacíos y con varias raíces, y dos clones del mismo proyecto chocarían. |
| Almacenamiento | Archivos append-only (JSONL) | La reatribución de Q37 y las consultas por sesión obligan a reescribir el archivo o a mantener índices propios. |
| Almacenamiento | KV embebido en Rust puro | Sin consultas: cada consulta por sesión, worktree o rango habría que programarla y mantenerla. |
| Almacenamiento | Un solo archivo SQLite para todos los repos | Una corrupción afecta a todos los repos a la vez (agrava R13) y retirar un repo no aísla sus datos. |

## Consecuencias

- ✅ Cierra P9: ubicación por SO, separación por repo y formato definidos.
- ✅ Retirar y volver a añadir conserva los datos (Q25); perder el perfil o un archivo se trata como hueco (Q26).
- ✅ La reatribución de Q37 es una consulta indexada por sesión, sin reescribir archivos (ADR-GRP-013).
- ✅ Los datos y la configuración personal no viajan entre máquinas (Q31, PQ-7).
- ✅ Los tests aíslan el perfil con la variable de sobreescritura, sin tocar el perfil real.
- ⚠️ **Un repo movido de carpeta obtiene una clave nueva** y su historial anterior queda en el perfil sin enlazar. **Mitigación**: el commit raíz guardado como pista permite ofrecer el reenlace en una fase posterior; en el MVP se comporta como un repo nuevo con lo anterior "sin atribuir".
- ⚠️ **La configuración local personal en el perfil contradice la tabla de BR-CONS-001**, que la ubica en "Repo, no versionada". **Pendiente para el PO**: actualizar esa fila (PQ-3, detalle en ADR-GRP-008).
- ⚠️ En Linux, el socket vive en `$XDG_RUNTIME_DIR`, fuera de las carpetas de datos. Se considera parte del perfil porque es una carpeta exclusiva de GitRaptor. **Pendiente para el PO**: confirmar que "perfil" abarca las carpetas exclusivas de la herramienta (datos, configuración, estado y ejecución).
- ⚠️ El perfil crece sin límite porque los eventos no se borran (ADR-GRP-013). **Mitigación**: solo se guardan metadatos; el tamaño se mide en INF-GRP-002 y una política de retención queda fuera del MVP.
- ⚠️ Sin copia de seguridad en el MVP (R13 aceptado por Q26).

## Validación

Todas las pruebas usan un perfil temporal (variable de sobreescritura) y repos temporales.

1. **Ubicación**: sin la variable, el perfil se resuelve a las carpetas de la tabla en los tres SO; en Windows nada se escribe bajo `%APPDATA%`.
2. **Permisos (SEC-06)**: carpetas 0700 y archivos 0600 en macOS y Linux, incluidos `-wal`, `-shm`, logs, lock y cuarentena; una carpeta preexistente 0755 o de otro propietario impide arrancar; en Windows, sin ACE de otros usuarios. Lint de CI contra SQL armado con cadenas. Un build release ignora `GITRAPTOR_PROFILE_DIR`.
3. **Clave**: dos worktrees del mismo repo dan la misma clave; dos clones del mismo proyecto dan claves distintas; un repo vacío se puede añadir; la misma ruta con otra grafía de mayúsculas en macOS y Windows da la misma clave.
4. **Retirar y volver a añadir**: los datos anteriores vuelven y el intervalo retirado aparece como hueco (US-GRP-006).
5. **Pérdida y corrupción**: borrar el perfil o corromper el archivo de un repo no detiene el motor; ese repo empieza de cero, lo anterior queda "sin atribuir" y los demás repos no cambian (US-GRP-005, US-GRP-015).
6. **Persistencia**: tras matar el daemon y reiniciarlo, los datos confirmados siguen disponibles (US-GRP-004).
7. **Privacidad**: el almacén de un repo de prueba con contenido marcado no contiene ese contenido.
8. **Repo intacto**: el arnés de INF-GRP-001 confirma que fuera del repo solo cambian las carpetas del perfil.
9. **Cadena de suministro (SEC-07)**: `cargo-deny` y `cargo-audit` bloquean en High/Critical, incluidos los avisos del SQLite embebido.

## Referencias

- **Reglas**: BR-CONS-001, BR-CONS-005, BR-AUTH-001, BR-EDGE-005, BR-EDGE-007.
- **Historias**: US-GRP-001, US-GRP-004, US-GRP-005, US-GRP-006, US-GRP-009, US-GRP-010, US-GRP-011, US-GRP-015.
- **Decisiones del context**: Q17, Q21, Q23, Q25, Q26, Q31; pregunta P9; riesgo R13.
- **Decisiones de producto**: PQ-3, PQ-4 y PQ-7 (Rene Bonilla, 2026-10-03).
- **NFR**: NFR-01, NFR-03, NFR-05, NFR-11.
- **ADRs**: ADR-GRP-005 (único escritor y canal), ADR-GRP-007 (formato de configuración), ADR-GRP-008 (configuración local en el perfil), ADR-GRP-013 (modelo persistido).
- **Enablers**: TS-GRP-001, INF-GRP-001.

## Revisión de seguridad (2026-10-03)

Enmienda tras la revisión del security-expert. No cambia ubicación, clave ni almacenamiento.

| Hallazgo | Cómo se cubre |
|---|---|
| M5 · Permisos sin cubrir `-wal`/`-shm`/logs/cuarentena y sin verificar propietario | Apartado 1: 0700/0600 con umask 077 en todos los archivos del perfil, verificación de propietario y modo al arrancar, ACL sin ACE de otros usuarios en Windows (SEC-06); el canal, en ADR-GRP-005 (SEC-01) |
| H4 (parte) · Override del perfil por entorno | Apartado 1: `GITRAPTOR_PROFILE_DIR` solo en builds de test (SEC-06) |
| M9 · UNC en Windows | Apartado 1: rutas validadas antes de tocar el FS (SEC-02) |
| L3 · SQLite bundled no se actualiza con el SO | Apartado 4: sus avisos se siguen con `cargo-deny`/`cargo-audit` (SEC-07) |

Validación ampliada: SEC-06 y SEC-07 (puntos 2 y 9).
