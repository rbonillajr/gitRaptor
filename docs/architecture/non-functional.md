# Requisitos No Funcionales — Motor local

> Requisitos no funcionales del motor local (F-001-01) con el ADR que los cubre y cómo se verifican. La sección "Security NFRs" incorpora la revisión del `security-expert` del 2026-10-03 y las enmiendas aplicadas a los ADRs.

## Metadata

- **Modo**: expandido (desde el outline en modo draft)
- **Estado**: expanded
- **Dominio**: GRP · **Feature**: motor-local (F-001-01)
- **Fecha**: 2026-10-03
- **Actualizado**: 2026-10-03 (revisión de seguridad)
- **Autor**: Arquitecto (AADD); Security NFRs: `security-expert`
- **Relacionados**: CTX-GRP-001, BR-GRP-001 (BR-CONS-001, BR-CONS-005, BR-AUTH-001, BR-AUTH-002, BR-VAL-003), ADR-GRP-001, ADR-GRP-002, ADR-GRP-004, ADR-GRP-005..013, BRD-GRP-001 § 7 (NFR-01..12), TS-GRP-001..004, INF-GRP-001, INF-GRP-002, SPIKE-GRP-001, SPIKE-GRP-002
- **Nota de formato**: no hay tipo canónico `nfr` en el esquema AADD. Por eso este archivo lleva la sección Metadata de la plantilla `template-rnfs.md` y no frontmatter con `id`; así no dispara el gate de tipos ni deja un artefacto inválido en el índice.

## Atributos de calidad aplicados al motor

| NFR | Atributo | Objetivo para el motor | ADR que lo cubre | Cómo se verifica | Status |
|-----|----------|------------------------|------------------|------------------|--------|
| NFR-01 | Cero pérdida de datos | Repo observado idéntico antes y después; fuera del repo solo cambia el perfil (BR-CONS-001, BR-AUTH-002). El motor no toma locks ni ejecuta programas configurados por el usuario | ADR-GRP-009, ADR-GRP-006, ADR-GRP-010 | Arnés INF-GRP-001: huella de working tree y `.git` antes y después, con ejecución de control, en escenarios de riesgo y en los tres SO; bloquea el merge | expanded |
| NFR-02 | Seguridad del MCP | `raptor-mcp` es cliente del daemon: sin shell (argv fijo), entradas validadas, allowlist de operaciones (sin comandos reservados) y respuestas limitadas al repo del llamante | ADR-GRP-005, ADR-GRP-009, ADR-GRP-013 | SEC-01..SEC-14 (abajo); revisión OWASP / MCP Top 10 antes de cada release; la spec del MCP (F-001-05) completa el contrato | expanded |
| NFR-03 | 100% local | Sin tráfico de red del motor; IPC solo local; sin telemetría salvo opt-in; sin conexiones SMB por rutas UNC | ADR-GRP-005, ADR-GRP-012, ADR-GRP-007, ADR-GRP-009 | Test de integración sin red, que falla si el motor abre un socket de red; `lsof -i`/`netstat` sin puertos (SEC-01); ruta UNC con captura de red: 0 conexiones (SEC-02); revisión de dependencias | expanded |
| NFR-04 | Frescura | < 500 ms de extremo a extremo; motor ≤ 300 ms, Cockpit ≤ 100 ms y margen de 100 ms. ⚠️ **ASSUMPTION**: p95 | ADR-GRP-011, ADR-GRP-010 | Banco INF-GRP-002 con timestamps por etapa en el evento; gate de p95 en CI; con un cliente lento el p95 de los demás se mantiene (SEC-08) | expanded |
| NFR-05 | Escala | 10 o más worktrees activos y repos de más de 100K commits sin degradarse | ADR-GRP-010, ADR-GRP-006 | SPIKE-GRP-002 (viabilidad) e INF-GRP-002 (repo sintético de 100K commits con 10 worktrees) | expanded |
| NFR-06 | Portabilidad | Mismo comportamiento en Windows, macOS y Linux, x64 y arm64; autoarranque, canal y perfil por SO | ADR-GRP-005, ADR-GRP-006, ADR-GRP-010 | Matriz de CI en los tres SO para INF-GRP-001 e INF-GRP-002; arm64 al menos en el build | expanded |
| NFR-07 | Compatibilidad con Git | Git del sistema 2.38 o superior; sin él, "Esperando Git" y no se observa nada (Q28). El motor respeta la config que gobierna escrituras y credenciales y neutraliza solo la que ejecuta programas al leer | ADR-GRP-009 | Tests con Git ausente, Git anterior a 2.38 y Git instalado después (US-GRP-014); repo canario de SEC-09 | expanded |
| NFR-08 | Sin APIs privadas | Detección con información pública del SO; lectura de archivos de terceros condicionada a PQ-2 y limitada a metadatos | ADR-GRP-012 | Revisión de las señales del adaptador; el adaptador se desactiva solo ante un formato que no reconoce; tests de SEC-04 | expanded |
| NFR-09 | UX de la TUI | No aplica al motor: lo cubre la CLI/TUI. El motor expone estados y diagnósticos estructurados, sin texto de presentación | ADR-GRP-004 | Fuera de motor-local | expanded |
| NFR-10 | i18n | El motor expone diagnósticos y estados como códigos tipados; el texto en/es lo pone el cliente | ADR-GRP-007, ADR-GRP-005 | Snapshot del contrato de `crates/api` sin cadenas de presentación | expanded |
| NFR-11 | Licencias | Sin AGPL en el núcleo; dependencias permisivas (almacén embebido incluido) | ADR-GRP-006 (almacén); transversal | `cargo-deny` con lista de licencias permitidas en CI (SEC-07) | expanded |
| NFR-12 | Calidad | Integración contra repos reales con agentes simulados y caos (kill del daemon a mitad de una operación) | ADR-GRP-005, ADR-GRP-013 | INF-GRP-001 y tests de caída de ADR-GRP-005 y ADR-GRP-013 (huecos "sin atribuir", SEC-13) | expanded |
| BR-CONS-005 | Continuidad | 0 huecos mientras la máquina está encendida; los huecos inevitables se reconcilian como "sin atribuir" y guardan su causa | ADR-GRP-005, ADR-GRP-010, ADR-GRP-013 | Tests de reinicio del proceso, suspensión simulada y desbordamiento del watcher; en Linux, prueba de límite de vigilancia agotado | expanded |
| HUELLA | Huella en la máquina | ⚠️ **ASSUMPTION**: en reposo, CPU < 1% y memoria residente < 150 MB con 10 worktrees (el BRD no lo fija; Known Risk 3) | ADR-GRP-010 | Medición en INF-GRP-002 | expanded |

## Security NFRs

> **Modelo de amenaza**: proceso del mismo usuario **no confiable** (incluye un **agente comprometido por prompt injection** — OWASP LLM01 / MCP Top 10), repo o worktree manipulado, archivos de terceros (`~/.claude`) y otros usuarios locales. Sin superficie de red. Matriz EH: aplican las filas 15 (credenciales → SEC-05), 18 (acceso no autenticado → SEC-01/03) y 19 (dependencias → SEC-07); el resto (móvil, TLS, headers, CORS) no aplica por ausencia de red.

| ID | Requisito verificable | Amenaza (STRIDE) | ADR | Verificación (test) | Severidad |
|----|----------------------|------------------|-----|---------------------|-----------|
| SEC-01 | **Canal exclusivo del usuario.** Socket 0600 dentro de directorio 0700 creados con umask 077 antes de `bind`; al arrancar, verificar propietario (uid) y modo del directorio preexistente y **abortar** si no cuadran (no "arreglar" con chmod); comprobar uid del par. Windows: `FILE_FLAG_FIRST_PIPE_INSTANCE`, `PIPE_REJECT_REMOTE_CLIENTS`, DACL solo SID del usuario; el cliente verifica el SID del servidor y conecta con `SECURITY_SQOS_PRESENT \| SECURITY_IDENTIFICATION`. Ninguna escucha TCP/UDP | S, I, E | 005, 006 | Por SO: cliente de otro usuario rechazado; directorio pre-creado 0755 → daemon no arranca; pipe ocupado por otro proceso → el cliente rechaza; cliente remoto rechazado; `lsof -i`/`netstat` sin puertos | Alta |
| SEC-02 | **Entradas del IPC.** JSON-RPC con `deny_unknown_fields`, tamaño y profundidad máximos por mensaje, batches desactivados o acotados, timeout de handshake. Rutas: absolutas; rechazar UNC, `\\?\`, nombres de dispositivo y ADS **antes** de tocar el FS; canonicalizar y comprobar pertenencia a un worktree observado. Refs: validar con reglas de `check-ref-format` y siempre tras `--` | T, D, E, I | 005, 009 | `cargo-fuzz` del decodificador; corpus de rutas maliciosas (traversal, symlink hacia fuera, UNC con captura de red: 0 conexiones SMB); ref `--upload-pack=x` rechazada | Alta |
| SEC-03 | **Comandos reservados autorizados en el daemon** (añadir/retirar repos, corregir/retirar corrección, **parar el daemon**). El daemon no acepta ninguna marca del cliente como prueba de confirmación: identifica al proceso con un identificador no reutilizable (pidfd / audit token / handle), revisa su ascendencia y comprueba por su cuenta que el cliente tiene terminal de control y que el líder de su sesión no desciende de un agente. El MCP no expone estas operaciones. El registro de un agente toma el worktree del **cwd del proceso llamante**, no de un parámetro. Cada comando reservado queda en un registro de auditoría append-only visible en los clientes | E, S, R | 005, 012, 013 | Cliente JSON-RPC directo (sin la CLI) descendiente de un agente simulado → rechazado; ídem con pty (`script`) bajo el agente; registro con worktree ajeno → rechazado; evasión residual (doble fork/`setsid`) documentada como riesgo aceptado | Alta |
| SEC-04 | **Archivos de terceros (`~/.claude`).** Solo lectura, `O_NOFOLLOW`, solo archivos regulares dentro de `~/.claude/projects`, lectura no bloqueante (rechazar FIFOs/dispositivos), tope por línea y por escaneo, timeout. Extraer solo campos de una allowlist (herramienta, ruta, hora, id de sesión, cwd); las rutas extraídas solo se comparan, nunca se abren. Procesos: cargar solo el ejecutable (y argv[1] si es el script de entrada), sin `cmd`/`environ` completos (`ProcessRefreshKind` de sysinfo) | I, D, T | 012 | Hash de `~/.claude` idéntico antes/después; línea de 100 MB, symlink a `/dev/zero` o FIFO no bloquean ni tumban el daemon; con `claude -p "CANARY"` y un transcript con canario, el canario no aparece en perfil, logs ni stream IPC | Alta |
| SEC-05 | **Secretos.** El motor no persiste, registra ni expone contenido de archivos, valores de config, entorno ni argv de terceros. `git config` solo con `--get` de claves de una allowlist tipada (retirar `--list` y `--get-regexp`). URLs de remotos guardadas sin userinfo. Logs estructurados y panic hook que redacta | I | 009, 012, 013, 005 | Suite con secretos plantados (`.env`, token en URL del remoto, `http.extraHeader`) y gitleaks/trufflehog sobre perfil, logs y captura del stream IPC: 0 hallazgos | Alta |
| SEC-06 | **Perfil.** Directorios 0700 y archivos 0600 (incluidos `-wal`, `-shm`, logs, lock, cuarentena) creados con umask restrictiva; propietario verificado al arrancar; en Windows, ACL heredada sin ACE de otros usuarios. SQL siempre parametrizado. Ningún dato del motor fuera del perfil. `GITRAPTOR_PROFILE_DIR` solo en builds de test | I, T | 006 | Test de permisos por SO (incl. `-wal`/`-shm`); lint de CI contra SQL armado con cadenas; build release ignora la variable de sobreescritura | Media |
| SEC-07 | **Cadena de suministro.** `Cargo.lock` y lock de pnpm versionados, builds `--locked`; `cargo-deny` (advisories, licencias sin AGPL, fuentes solo crates.io) y `cargo-audit` bloquean en High/Critical; seguir avisos de SQLite bundled (no se actualiza con el SO) y de gix; binarios firmados/notarizados con checksums; npm con provenance | T, E | Transversal (001, 002, 006) | Gate de CI | Alta |
| SEC-08 | **Robustez frente a clientes.** Cola acotada por suscriptor: el cliente lento se desconecta con evento "resync" y nunca bloquea al productor; límites de conexiones y suscripciones por cliente; rate limit de consultas; las consultas se sirven del estado en memoria, sin lanzar `git` por petición | D | 005, 011 | Un cliente que no lee + 100 conexiones simultáneas: el p95 de los demás clientes sigue dentro del presupuesto de ADR-011 | Media |
| SEC-09 | **Cero ejecución de código configurable.** Por ninguna vía (gix ni Git CLI) se ejecutan filtros clean/smudge/process, textconv, diff externo, fsmonitor, hooks, gpg, helpers de credenciales ni pager. **`status`/`diff` siempre con gix sin filtros**; salen de la allowlist del CLI. Se mantiene el mínimo de Git 2.38 (Q28, NFR-07): la alternativa con CLI y `GIT_ATTR_SOURCE` al árbol vacío, que exige Git ≥ 2.40, queda **descartada**. gix configurado para no invocar el binario `git`. `log` prohíbe placeholders `%G*` | E, T | 009 | Repo canario con `filter.*.clean`, `diff.*.textconv`, `core.fsmonitor`, hooks y `gpg.program` apuntando a un script que deja un marcador: el marcador nunca aparece. Auditoría dinámica de `exec` en INF-GRP-001 (eslogger/ETW/strace): todo proceso hijo pertenece a la allowlist | Alta |
| SEC-10 | **Entorno controlado.** El daemon arrancado bajo demanda no hereda el entorno del cliente: se arranca vía gestor de servicios (`launchctl kickstart`, `systemctl --user start`) o con entorno limpio. Los hijos `git` reciben un entorno construido por allowlist (no denylist), PATH sin entradas relativas. El ejecutable de Git (también la ruta explícita de la config de perfil) debe ser absoluto, archivo regular, propiedad del usuario o de root y no escribible por grupo/otros | E, T | 005, 009 | Arranque con `GIT_EXEC_PATH`, `LD_PRELOAD`/`DYLD_INSERT_LIBRARIES`, `PATH=.:…` o `XDG_CONFIG_HOME` hostiles: sin efecto; `git` escribible por todos → rechazado | Alta |
| SEC-11 | **Repos, worktrees y rutas no confiables.** Mismo criterio de propiedad que `safe.directory` en gix y en el CLI, nunca `-c safe.directory=*`; repo de otro propietario → "no disponible". Worktree enlazado solo si su `gitdir` es bidireccional y su raíz no es `/`, `$HOME` ni un ancestro del repo; tope de watches por repo. No tocar rutas UNC/de red sin acción explícita del usuario. `.gitraptor/settings.json`: solo archivo regular, sin enlaces que salgan del worktree, tope de tamaño, diagnósticos sin fragmentos de contenido; `baseBranch` validada como ref | E, I, D | 009, 010, 007 | Repo de otro uid → "no disponible"; `gitdir` manipulado hacia `$HOME` → no se vigila; settings como symlink a `~/.ssh/id_rsa` → diagnóstico sin contenido; ruta UNC → 0 conexiones SMB | Media |
| SEC-12 | **Salida hacia terminales y agentes.** Todo texto procedente del repo o de un agente (rutas, ramas, nombres de agente declarados, diagnósticos) se limpia de caracteres de control y escapes ANSI/OSC antes de mostrarse en CLI/TUI. Respuestas MCP estructuradas, con longitud máxima, sin mensajes de commit ni contenido, limitadas al repo del llamante (LLM01, prompt injection indirecta) | T, I | 005, 013 | Rama o archivo con `\x1b]52;…` u `\x1b]0;…` sale escapado; snapshot de respuestas MCP sin campos fuera de la allowlist | Media |
| SEC-13 | **No repudio de huecos.** Parar el daemon es comando reservado (SEC-03), incluida la petición de parada por "versión incompatible", que solo se acepta del binario instalado. El hueco guarda la causa y, si lo provocó un comando, el cliente que lo pidió. Una parada o caída con sesión activa se marca y se muestra en los clientes | R, D | 005, 013 | Agente simulado ejecuta `raptor daemon stop` → rechazado; `kill -9` con sesión activa → hueco "caída durante sesión activa" | Media |
| SEC-14 | **Autoarranque.** Ruta absoluta del binario, entre comillas en HKCU Run; `daemon enable` se niega si el binario vive en la caché de npx o en una carpeta temporal; no invocable desde el MCP; `disable` elimina exactamente lo que creó `enable` | E, T | 005 | Ruta con espacios en Windows; `enable` desde npx → rechazado | Baja |

### Hallazgos de la revisión y dónde se cubren

| Hallazgo | Severidad | ADR enmendado | SEC |
|----------|-----------|---------------|-----|
| H1 · Confirmación TTY en el cliente, ascendencia por PID | High | ADR-GRP-005 § 6 | SEC-03 |
| H2 · `status`/`diff` del CLI ejecutan filtros `clean` | High | ADR-GRP-009 § 1, § 3 | SEC-09 |
| H3 · `config --list`/`--get-regexp` exponen tokens | High | ADR-GRP-009 § 3 | SEC-05 |
| H4 · Denylist de entorno y arranque que hereda el entorno del cliente | High | ADR-GRP-005 § 3, ADR-GRP-009 § 3, ADR-GRP-006 § 1 | SEC-10, SEC-06 |
| M1 · gix puede lanzar `git` | Medium | ADR-GRP-009 § 1 (⚠️ pendiente de comprobar en la versión fijada del crate) | SEC-09 |
| M2 · `gitdir` manipulado | Medium | ADR-GRP-010 § 2 | SEC-11 |
| M3 · `sysinfo` carga `cmd`/`environ` | Medium | ADR-GRP-012 (S1, Privacidad) | SEC-04 |
| M4 · Lector de transcripts sin límites | Medium | ADR-GRP-012 (Adaptador) | SEC-04 |
| M5 · Permisos de canal y perfil | Medium | ADR-GRP-005 § 5, ADR-GRP-006 § 1 | SEC-01, SEC-06 |
| M6 · Repudio parando el daemon | Medium | ADR-GRP-005 § 4, ADR-GRP-013 § 1, § 5 | SEC-13 |
| M7 · Registro de agente y nombres suplantables | Medium | ADR-GRP-005 § 6 | SEC-03 |
| M8 · Escapes de terminal y prompt injection vía MCP | Medium | ADR-GRP-005 § 5 (contrato de salida de `crates/api`), ADR-GRP-013 § 6; pendiente ADR-GRP-004 y spec del MCP | SEC-12 |
| M9 · UNC en Windows | Medium | ADR-GRP-005 § 5, ADR-GRP-006 § 1, ADR-GRP-009 § 2, ADR-GRP-010 § 2 | SEC-02, SEC-11 |
| M10 · Ruta de Git y `.gitraptor/settings.json` sin validar | Medium | ADR-GRP-007, ADR-GRP-009 § 4 | SEC-10, SEC-11 |
| L1 · Autoarranque | Low | ADR-GRP-005 § 3 | SEC-14 |
| L2 · `log` con `%G*` | Low | ADR-GRP-009 § 3 | SEC-09 |
| L3 · SQLite bundled | Low | ADR-GRP-006 § 4 | SEC-07 |
| I1 · Matriz EH (filas 15, 18, 19) | Info | — | Modelo de amenaza |
| I2 · Retención ilimitada de rutas en el perfil | Info | ADR-GRP-006 (riesgo aceptado fuera del MVP) | — |
| I3 · Alineación con la política de seguridad de ASSA | Info | — | `[POR VERIFICAR]` (context § 5) |
| I4 · Supuesto "la shell de Claude Code no tiene TTY" | Info | ADR-GRP-012, ADR-GRP-005 | Deja de ser crítico |

## Gate de seguridad

- **Veredicto**: **aprobado con condiciones**; no bloquea globalmente. Sin hallazgos Critical.
- **Estado tras las enmiendas (2026-10-03)**: H1, H2, H3 y H4 **quedan cubiertos en texto** en ADR-GRP-005, ADR-GRP-009 y ADR-GRP-006, sin cambiar decisiones de fondo. Los Medium y Low también tienen su cobertura en los ADRs de la tabla anterior.
- **Bloqueadas hasta que se cumplan las condiciones**: las Dev Specs de `crates/git` (TS-GRP-002; ADR-GRP-009: H2, H3, H4, M1) y del daemon, el canal y los comandos reservados (TS-GRP-003, TS-GRP-004; ADR-GRP-005: H1, H4, M6).
- **Condiciones para pasar ADR-GRP-005 y ADR-GRP-009 a `accepted`**:
  - SEC-01, SEC-02, SEC-03, SEC-09 y SEC-10 en su Validación (cubierto en texto).
  - INF-GRP-001 con el repo canario (SEC-09) y la auditoría dinámica de `exec` (eslogger, ETW, strace) implementados.
  - M1 comprobado en la versión fijada de `gix`.
