---
id: TS-GRP-001
title: "Almacén de datos del motor en el perfil"
type: ts
status: In Review
feature: motor-local
domain: GRP
priority: high
complexity: medium
created: 2026-10-03
updated: 2026-10-04
related:
  adrs: [ADR-GRP-006, ADR-GRP-013, ADR-GRP-005]
  stories: [US-GRP-001, US-GRP-004, US-GRP-005, US-GRP-006, US-GRP-007, US-GRP-009, US-GRP-010, US-GRP-011, US-GRP-015]
  specs: [DEV-TS-GRP-001]
ado:
  id: null
  url: null
tags: [motor-local, perfil, almacenamiento, sqlite, clave-de-repo, seguridad]
---

## TS-GRP-001: Almacén de datos del motor en el perfil

**Valor**: todas las historias con datos persisten en un único sitio, separado por repo, que sobrevive a reinicios y aísla la corrupción.

### Descripción

**Como** Arquitecto
**Quiero** el almacén del perfil con su ubicación por SO, la clave de repo y la persistencia transaccional por repo
**Para** que el motor guarde repos, sesiones, eventos, atribuciones y huecos sin escribir nunca fuera del perfil (Q17, Q21)

> Dev Spec: [`dev-specs/TS-GRP-001-dev-spec.md`](../dev-specs/TS-GRP-001-dev-spec.md) | Implementada en `crates/core` (módulo `profile`), en revisión. Pendiente: comprobación de la ACL en Windows (SEC-06).
>
> **Depende de**: — (arranca el día uno). **ADRs**: ADR-GRP-006 (§ 1 a § 4), ADR-GRP-013 (§ 1, entidades), ADR-GRP-005 (único escritor). **Seguridad**: SEC-06 y SEC-07 de `docs/architecture/non-functional.md`.

### Alcance Técnico

- **Implementar** la resolución de las carpetas del perfil (datos, configuración, estado y ejecución) por SO, con Windows siempre en la variante local (ADR-GRP-006 § 1, PQ-7).
- **Fijar** el identificador corto de la carpeta del perfil para no superar el límite de ruta del socket en macOS (ADR-GRP-005).
- **Implementar** la sobreescritura de la raíz del perfil por variable de entorno, para que toda prueba use un perfil temporal, **solo en builds de test**; el binario release la ignora (SEC-06).
- **Crear** las carpetas y los archivos del perfil con permisos limitados al usuario y umask restrictiva desde su creación, incluidos `-wal`, `-shm`, logs, lock y cuarentena (SEC-06).
- **Verificar** al arrancar el propietario y el modo de las carpetas preexistentes y no arrancar si no cuadran; en Windows, comprobar que la ACL no tiene ACE de otros usuarios.
- **Garantizar** SQL siempre parametrizado.
- **Crear** el índice global de repos observados con su clave opaca, ruta canónica, estado y pista del commit raíz (ADR-GRP-006 § 3).
- **Implementar** la clave de repo por directorio Git común, normalizada para enlaces simbólicos y sistemas sin distinción de mayúsculas.
- **Crear** un almacén embebido por repo con las entidades de ADR-GRP-013 § 1 e índices por sesión y por worktree.
- **Implementar** la escritura por lote en una sola transacción, con el daemon como único escritor.
- **Implementar** la versión de esquema con migraciones incluidas en el binario y el rechazo, con diagnóstico, de un almacén más nuevo que el binario.
- **Implementar** la comprobación de integridad al abrir: un archivo corrupto se aparta sin borrarse y ese repo empieza como perfil perdido (Q26).
- **Fuera de alcance**: reglas de resolución de la atribución (US-GRP-009, US-GRP-010), reconciliación y huecos (US-GRP-005), lectura de la configuración (US-GRP-013), retención, copia de seguridad y reenlace de repos movidos.

### Plan de Verificación

#### Pruebas Automatizadas

- **Ubicación**: sin la variable de sobreescritura, las carpetas coinciden con la tabla de ADR-GRP-006 § 1 en los tres SO; en Windows no se escribe nada bajo la carpeta roaming.
- **Permisos (SEC-06)**: carpetas 0700 y archivos 0600 en macOS y Linux, incluidos `-wal` y `-shm`; una carpeta preexistente 0755 o de otro propietario impide arrancar; en Windows, sin ACE de otros usuarios.
- **Override**: un build release ignora la variable de sobreescritura del perfil.
- **SQL**: un lint de CI rechaza SQL armado con cadenas.
- **Dependencias (SEC-07)**: `cargo-deny` y `cargo-audit` bloquean avisos High/Critical, incluido el SQLite embebido.
- **Clave**: dos worktrees del mismo repo dan la misma clave; dos clones del mismo proyecto dan claves distintas; un repo sin commits se puede añadir; la misma ruta con otra grafía de mayúsculas en macOS y Windows da la misma clave.
- **Retirar y volver a añadir**: la clave y los datos anteriores se recuperan (Q25).
- **Corrupción**: un almacén corrupto se aparta con marca de tiempo, ese repo empieza vacío y los demás repos no cambian.
- **Esquema más nuevo**: el repo no se abre, se emite un diagnóstico y el archivo queda intacto.
- **Persistencia**: tras matar el proceso a la fuerza, todo lote confirmado sigue disponible.
- **Privacidad**: el almacén de un repo con contenido marcado no contiene ese contenido (NFR-03).
- **Repo intacto**: el arnés de INF-GRP-001 confirma que fuera del repo solo cambian las carpetas del perfil.

#### Verificación Manual / Sandbox

- Inspeccionar en una máquina de cada SO las carpetas del perfil creadas y sus permisos.
