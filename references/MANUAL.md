# Manual NOPRAY (`DMCS-1.1`): software escrito directamente en código máquina

> **N**ative **O**pcode **P**rotocol: **R**each the **A**lmighty **Y**ourself.
> «Habla directo con Dios sin rezar»: del lenguaje natural a los bytes que ejecuta la CPU,
> sin lenguaje de programación, compilador, ensamblador, enlazador ni bibliotecas.

## 1. La idea

Los lenguajes de programación son una prótesis cognitiva para personas: nadie recuerda opcodes,
desplazamientos de salto ni cabeceras ELF. Un agente de IA sí puede razonar sobre esos detalles,
así que NOPRAY explora el camino directo:

```text
Intención en lenguaje natural ──agente──▶ bytes de máquina mapeados (slots .hex + nopray.toml)
                                            │ nopray build/audit/annotate (no codifica nada)
                                            ▼
                                  binario nativo + mapa + auditoría ──▶ CPU
```

El objetivo no es solo crear programas, sino que **otro agente pueda modificarlos meses
después**: por eso cada byte tiene dueño, propósito, contrato e intención declarada.

## 2. Qué se ha demostrado (y dónde)

| Programa | Tamaño | Destino | Estado |
|---|---|---|---|
| `calc` (calculadora X11) | 4 096 B | x86-64 Linux, CachyOS + Hyprland/Xwayland | Ejecutado en vivo con capturas; mapa exacto; nivel L1 (RWX, solo uso local) |
| `nopray3d` (raycaster 60 FPS) | 8 192 B | x86-64 Linux | Ejecutado en vivo con capturas; mapa exacto; W^X |
| `nopray-doom` (FPS 3D + PCM 24 kHz) | 32 768 B | x86-64 Linux, CachyOS + Hyprland + PipeWire | Ejecutado en vivo con capturas y flujo PCM activo (`2 844` instrucciones, `2 844` intenciones, 0 fallos); X11 `640×400` + audio nativo PipeWire/PulseAudio (5 voces, mute `N`); nivel L2 W^X, 6 syscalls |
| `nopray-httpd` v2 (web SSR) | 40 960 B | x86-64 Linux detrás de proxy inverso (Caddy) | Mapa exacto byte a byte; nivel L2 W^X; perfil seccomp de 13 syscalls |
| `hello-aarch64` | 384 B | AArch64 Linux (Raspberry Pi OS arm64) | Ejecutado con `qemu-aarch64`; falta probarlo en una Pi real |
| `hello-riscv64` | 384 B | RISC-V 64 Linux | Ejecutado con `qemu-riscv64` |

Los recuentos de instrucciones salen de `nopray audit` (`660` en `calc`, `768` en `nopray3d`,
`879` en `nopray-httpd` v2 y `2 844` en `nopray-doom`, sin el relleno final de cada ranura) y
sustituyen a los manuales anteriores (`679`/`785`), que no eran reproducibles.

## 3. El método

1. **Ranuras fijas (*fixed slots*).** Cada rutina vive en una dirección fija con holgura de NOP.
   Cambiar una función no desplaza ninguna otra, así que los `call` y punteros del resto siguen
   valiendo. Es un enlazador hecho a mano, pero declarado en `nopray.toml` y verificado.
2. **Registro base de estado.** Un registro preservado (`r15`, `x28`) apunta al estado global;
   el mapa muestra qué ranura lee y escribe cada campo.
3. **Intenciones.** Cada instrucción escrita lleva `#= lo que quise codificar`; la auditoría la
   compara con lo que los bytes realmente decodifican. Es la red de seguridad contra el error
   típico de codificar a mano (registro, desplazamiento o campo equivocados).
4. **Herramienta que no programa.** `nopray` coloca bytes, los decodifica, comprueba el mapa y
   la política de seguridad y genera `MAP.md`. Nunca elige ni codifica instrucciones.

Detalle operativo: [WORKFLOW.md](WORKFLOW.md). Formato: [MANIFEST.md](MANIFEST.md).

## 4. Mapa de posibilidades

| Categoría | Ejemplos | Estado |
|---|---|---|
| GUI X11/Xwayland | calculadoras, visores, FPS 3D texturizados, paneles | **Demostrado en vivo** (`calc`, `nopray3d`, `nopray-doom`) |
| Audio PCM en tiempo real (PipeWire / PulseAudio) | sintetizadores, efectos de juego, streams 24 kHz S16LE sin librerías | **Demostrado en vivo** (`nopray-doom`, protocolo nativo v15 sobre `/run/user/<uid>/pulse/native`, sin añadir syscalls ni subprocesos) |
| Servidores TCP/HTTP | webs estáticas o SSR, health-checks, proxies | **Demostrado** (`nopray-httpd`), exige nivel L2 + seccomp |
| CLI y utilidades Linux en otras CPU | herramientas para Raspberry Pi OS arm64, RISC-V | **Demostrado en emulador** (`hello-aarch64/riscv64`, portado `jstat` AArch64 en benchmark r2) |
| Wayland nativo (`wl_shm`), `/dev/fb0`, TTY, ALSA directo | apps sin X11, kioscos, consola | Documentado, **sin demostrar** |
| Microcontroladores ESP32-C3/C6 | firmware UART/GPIO sin SDK | Formato de imagen y watchdogs verificados en fuente; **sin imagen todavía** |
| RP2040/RP2350, Raspberry Pi bare metal | firmware sin SDK | Documentado con huecos **sin verificar** |
| Xtensa (ESP32/S3), AVR | | Fuera de alcance hasta tener decodificador |

Estado por plataforma: [TARGETS.md](TARGETS.md).

## 5. Evaluación honesta

**Dónde brilla**
- Programas pequeños y autónomos con superficie mínima: cero bibliotecas, lista exacta de
  syscalls, arranque instantáneo, pocos KiB.
- Auditoría completa del artefacto: cada instrucción, cada syscall, cada byte con dueño.
- Investigación y docencia sobre la frontera entre intención y máquina.

**Qué no resuelve**
- **No prueba cómo se fabricaron los bytes.** La pureza (niveles L0–L3) es una declaración del
  autor; un binario compilado con las cabeceras rehechas pasa la auditoría.
- **No da seguridad de memoria.** Sin compilador no hay comprobación de límites, canarios ni
  ASLR (dirección de carga fija). Lo expuesto a red necesita W^X, seccomp, sandbox y pruebas con
  entradas maliciosas.
- **La confianza se desplaza, no desaparece**: del árbol de dependencias al agente autor y a la
  revisión.
- Criptografía, TLS y sistemas grandes mantenidos por equipos humanos siguen siendo terreno de
  Rust y bibliotecas auditadas.

Modelo de amenazas y requisitos de despliegue: [SECURITY.md](SECURITY.md).

## 6. Cómo pedirle algo a un agente

> «Usa la skill `nopray`. Inspecciona el destino y crea `<aplicación>` en código máquina, nivel
> L2, exposición `<local|lan|internet>`. Entrega `nopray.toml`, slots con intenciones,
> `audit --strict` limpio, `MAP.md` y prueba de ejecución.»

Para modificar: «Cambia `<comportamiento>` en `<app>`; entrega `nopray diff` y vuelve a probar
las ranuras que llaman a la que cambió.»

## 7. Evidencia y próximos pasos

1. **Benchmarks cerrados (24–26/11/2026):** ejecución en 3 niveles (`72/72` celdas, `nopray-opt` sin ramas batiendo a `gcc -O3 -march=native`) y autoría/mantenimiento/portado multi-modelo (`70/70` celdas, `9 990` pruebas ocultas, auditoría forense de 12 ejecuciones NOPRAY) consolidados en [BENCHMARKS.md](BENCHMARKS.md).
2. **Estándar y herramienta afinados (`C017`, `C011`, intenciones y pureza):**
   - `WARN C017` detecta direcciones interiores no etiquetadas en `rodata`/`data` (cierra el punto ciego de `nopray-jb-r3`).
   - `C011` canonicaliza sub-registros y vectores x86-64 (`eax`→`rax`, `r11d`→`r11`, `xmmN`/`ymmN`→`zmmN`).
   - Las intenciones `#=` resuelven símbolos `[[state]]` y aceptan formas cortas Intel (`stosd`, `rep movsb`, `rep stosq`, `fld`, `fstp`); atraparon 5 errores reales de bits `REX.R/X/B` en `nopray-doom` antes de la primera ejecución.
   - Prohibición explícita de DSL/mini-ensambladores de campos de bits: los scripts auxiliares solo pueden resolver desplazamientos `@label` y direcciones declaradas sobre opcodes escritos a mano.
3. **Sustrato de audio PCM verificado en vivo (26/11/2026):** protocolo nativo PipeWire/PulseAudio v15 sobre socket Unix (`/run/user/<uid>/pulse/native`, `SO_PEERCRED` sin cookie, drenado no bloqueante de `PA_COMMAND_REQUEST` y mute sin clics) demostrado en `nopray-doom` y documentado en [GRAPHICS-SUBSTRATES.md](GRAPHICS-SUBSTRATES.md).
4. **Próximos pasos:** primer firmware ESP32-C3 (UART + LED), en emulador y después en placa con copia del firmware; decodificador Thumb para RP2040; empaquetador `nopray pack esp`; y primera app verificada sobre Wayland nativo `wl_shm`.
