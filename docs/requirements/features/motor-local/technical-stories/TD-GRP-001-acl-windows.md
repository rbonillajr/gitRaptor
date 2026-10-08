---
id: TD-GRP-001
title: "Verificación de ACL en Windows: git.exe (SEC-10) y perfil (SEC-06)"
type: td
status: partially-implemented
feature: motor-local
domain: GRP
priority: high
complexity: medium
created: 2026-10-05
updated: 2026-10-08
related:
  adrs: [ADR-GRP-006, ADR-GRP-009, ADR-GRP-002]
  stories: [TS-GRP-001, TS-GRP-002]
  specs: [DS-TD-GRP-001]
ado:
  id: null
  url: null
tags: [motor-local, windows, acl, dacl, sid, seguridad, deuda-tecnica, resolucion-git, perfil]
---

## TD-GRP-001: Verificación de ACL en Windows: git.exe (SEC-10) y perfil (SEC-06)

**Valor**: el motor arranca en Windows con un Git del sistema verificado y un perfil privado, sin aceptar ubicaciones "porque sí".

### Descripción

**Como** Arquitecto
**Quiero** comprobar el propietario y la DACL del `git.exe` candidato y de las carpetas del perfil con la API de seguridad de Windows
**Para** cerrar los dos pendientes *fail-closed* del 2026-10-04 que impiden soportar Windows

TS-GRP-001 y TS-GRP-002 se cerraron con dos pendientes de Windows:

- **SEC-10 (TS-GRP-002)**: la resolución de Git rechaza todo `git.exe` con `AclUnverified`, porque no comprueba las ACE. En la primera pasada de tests en la Windows real explica unos 65 fallos (causa A).
- **SEC-06 (TS-GRP-001)**: el perfil abre con el aviso `AclNotVerified` sin comprobar que otros usuarios no tienen acceso.

Ambos son requisito antes de cualquier release en Windows (INF-GRP-003).

> Dev Spec: dev-specs/TD-GRP-001-dev-spec.md ([enlace](../dev-specs/TD-GRP-001-dev-spec.md))
>
> **Depende de**: TS-GRP-001, TS-GRP-002 (hechas). **ADRs**: ADR-GRP-009 § 4 (validación del ejecutable), ADR-GRP-006 § 1 (permisos SEC-06), ADR-GRP-002 (crate `winsys`, única excepción a `forbid(unsafe_code)`).

### Alcance Técnico

- **`git.exe` (SEC-10)**: propietario de confianza (usuario actual, SYSTEM, Administrators o TrustedInstaller) y ningún ACE de escritura para otros SID, en el archivo, en su carpeta y en cada carpeta superior hasta la raíz del volumen. Ninguna ubicación se acepta por estar en `%ProgramFiles%`.
- **Perfil (SEC-06)**: las carpetas existentes solo dan acceso al usuario, SYSTEM y Administrators; si no, el motor no arranca y no las "arregla". Las carpetas nuevas se crean con una DACL protegida.
- *Fail-closed*: una ACL ilegible, una DACL NULL o un tipo de ACE desconocido se rechazan.
- Llamadas Win32 en el crate aislado `crates/winsys`, único con `unsafe` permitido.
- **Fuera de alcance**: verificar el árbol completo de la instalación de Git (`mingw64`, DLL); la ruta de instalación desde el registro (ADR-GRP-009 § 4); las carpetas superiores del perfil (`%LOCALAPPDATA%`), que tampoco se comprueban en Unix.

### Plan de Verificación

#### Pruebas Automatizadas

- En la Windows real: `C:\Program Files\Git\cmd\git.exe` se acepta.
- Se rechazan un `git.exe` con escritura para `Users`, una carpeta con `FILE_ADD_FILE` para `Users`, una carpeta superior con `FILE_DELETE_CHILD` para `Everyone` y un propietario `Users`.
- Una carpeta del perfil con lectura para `Users` hace fallar la apertura sin que cambie su ACL; una carpeta creada lleva la DACL protegida.

#### Verificación Manual / Sandbox

- Conteo de passed/failed de `cargo test` en la Windows real antes y después; los fallos por `AclUnverified`/`AclNotVerified` desaparecen.
- Revisión de `nassa-security:security-expert` sobre el crate `winsys` y las reglas de DACL.

## Estado de la implementación (2026-10-08)

Implementado en: PR #72.

Estado: implementación parcial. Pendiente:
- Ruta de instalación de Git leída del registro (ADR-GRP-009 § 4).
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
