# Requisitos No Funcionales — Motor local (outline)

> Outline generado en modo Draft. Cada fila se expande con `/aadd-expand NFR-<n>` en este mismo archivo. La sección "Security NFRs" queda marcada para expandirla con el agente `security-expert`.

## Metadata

- **Modo**: draft
- **Estado**: draft
- **Dominio**: GRP · **Feature**: motor-local (F-001-01)
- **Fecha**: 2026-10-03
- **Autor**: Arquitecto (AADD)
- **Relacionados**: CTX-GRP-001, BR-GRP-001 (BR-CONS-001, BR-CONS-005, BR-AUTH-001, BR-AUTH-002, BR-VAL-003), ADR-GRP-001, ADR-GRP-002, ADR-GRP-005..013, BRD-GRP-001 § 7 (NFR-01..12)
- **Nota de formato**: no hay tipo canónico `nfr` en el esquema AADD. Por eso este archivo lleva la sección Metadata de la plantilla `template-rnfs.md` y no frontmatter con `id`; así no dispara el gate de tipos ni deja un artefacto inválido en el índice.

## Outline — atributos de calidad aplicados al motor

| NFR | Atributo | Objetivo para el motor | ADR que lo cubre | Cómo se verifica | Status |
|-----|----------|------------------------|------------------|------------------|--------|
| NFR-01 | Cero pérdida de datos | Repo observado idéntico antes y después; fuera del repo solo cambia el perfil (BR-CONS-001, BR-AUTH-002) | ADR-GRP-009, ADR-GRP-006 | Arnés INF-GRP-001: huella de working tree y `.git` antes y después, en escenarios de riesgo, en los tres SO; bloquea el merge | draft |
| NFR-03 | 100% local | Sin tráfico de red del motor; IPC solo local; sin telemetría salvo opt-in | ADR-GRP-005, ADR-GRP-012, ADR-GRP-007 | Test de integración sin red, que falla si el motor abre un socket de red; revisión de dependencias | draft |
| NFR-04 | Frescura | < 500 ms de extremo a extremo; motor ≤ 300 ms, Cockpit ≤ 100 ms y margen de 100 ms. ⚠️ **ASSUMPTION**: p95 | ADR-GRP-011, ADR-GRP-010 | Banco INF-GRP-002 con timestamps por etapa en el evento; gate de p95 en CI | draft |
| NFR-05 | Escala | 10 o más worktrees activos y repos de más de 100K commits sin degradarse | ADR-GRP-010, ADR-GRP-006 | SPIKE-GRP-002 (viabilidad) e INF-GRP-002 (repo sintético de 100K commits con 10 worktrees) | draft |
| NFR-06 | Portabilidad | Mismo comportamiento en Windows, macOS y Linux, x64 y arm64; autoarranque y perfil por SO | ADR-GRP-005, ADR-GRP-006, ADR-GRP-010 | Matriz de CI en los tres SO para INF-GRP-001 e INF-GRP-002; arm64 al menos en el build | draft |
| NFR-07 | Compatibilidad con Git | Git del sistema 2.38 o superior; sin él, "Esperando Git" y no se observa nada (Q28) | ADR-GRP-009 | Tests con Git ausente, Git anterior a 2.38 y Git instalado después (US-GRP-014) | draft |
| NFR-08 | Sin APIs privadas | Detección con información pública del SO; cualquier lectura de archivos de terceros, condicionada a PQ-2 | ADR-GRP-012 | Revisión de las señales del adaptador; el adaptador se desactiva solo ante un formato que no reconoce | draft |
| NFR-11 | Licencias | Sin AGPL en el núcleo; dependencias permisivas (almacén embebido incluido) | ADR-GRP-006 (almacén); transversal | Chequeo de licencias de dependencias en CI con una lista de licencias permitidas | draft |
| BR-CONS-005 | Continuidad | 0 huecos mientras la máquina está encendida; los huecos inevitables se reconcilian como "sin atribuir" | ADR-GRP-005, ADR-GRP-010, ADR-GRP-013 | Tests de reinicio del proceso, suspensión simulada y desbordamiento del watcher; en Linux, prueba de límite de vigilancia agotado | draft |
| HUELLA | Huella en la máquina | ⚠️ **ASSUMPTION**: en reposo, CPU < 1% y memoria residente < 150 MB con 10 worktrees (el BRD no lo fija; Known Risk 3) | ADR-GRP-010 | Medición en INF-GRP-002 | draft |

## Security NFRs (outline — expandir con `security-expert`)

> Marcada para expansión. La superficie afecta a la IPC local, a la lectura de archivos de terceros, a los datos del repo y al perfil, así que según el workflow del Arquitecto la expansión se delega en `security-expert`, que redacta cada fila como NFR verificable con su referencia de checklist (`security-verification`: matriz EH, API Top 10 aplicado a IPC, Secrets, Dependencies).

| ID | Superficie | Requisito (outline) | ADR | Verificación prevista | Status |
|----|-----------|---------------------|-----|-----------------------|--------|
| SEC-01 | IPC local: permisos | El socket Unix solo es accesible por el usuario (0600 en un directorio 0700). El named pipe tiene DACL del SID del usuario y rechaza clientes remotos. No hay escuchas TCP | ADR-GRP-005 | Test por SO que intenta conectar con otro usuario o en remoto | draft |
| SEC-02 | IPC local: entradas | Validación de esquema de cada mensaje; tamaño máximo de mensaje; rutas canonizadas, contra path traversal y enlaces simbólicos; refs validadas | ADR-GRP-005 | Fuzzing del decodificador y tests de rutas maliciosas | draft |
| SEC-03 | Allowlist de repos | Solo se observan y consultan repos añadidos. Añadir o retirar repos y corregir atribuciones son acciones del desarrollador (Q40, BR-CONS-002), con chequeo del proceso que llama (PQ-6) | ADR-GRP-005 | Tests de rechazo desde un cliente que desciende de un proceso de agente | draft |
| SEC-04 | Archivos de terceros (`~/.claude`) | Solo lectura, nunca escritura. Del contenido se extraen solo metadatos (ids, horas, rutas); no se guardan prompts, código ni salidas de herramientas. Límites de tamaño y parseo tolerante | ADR-GRP-012 | Test que comprueba que `~/.claude` queda intacto y que el perfil no contiene texto de prompts | draft |
| SEC-05 | Secretos en repos | El motor no registra ni expone contenido de archivos, solo rutas y metadatos. Logs sin variables de entorno ni argumentos de procesos de terceros (P3 abierta) | ADR-GRP-013, ADR-GRP-012 | Escaneo de secretos sobre el perfil y los logs tras una suite con secretos plantados | draft |
| SEC-06 | Perfil | Directorio 0700 y archivos 0600 en macOS y Linux; en Windows, ACL heredada de `%LOCALAPPDATA%` del usuario. Ningún dato del motor fuera del perfil | ADR-GRP-006 | Test de permisos por SO | draft |
| SEC-07 | Cadena de suministro | Auditoría de vulnerabilidades y de licencias de dependencias en CI; versiones fijadas | Transversal | Gate de CI | draft |
| SEC-08 | Robustez ante clientes | Backpressure para clientes lentos; un cliente no puede degradar la observación de los demás | ADR-GRP-005, ADR-GRP-011 | Test con un cliente que no lee el stream | draft |
