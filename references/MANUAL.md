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
| `qwen3-tts-baremetal` (VM TTS neuronal 0.6B sin Linux) | 32 768 B | x86-64 bare metal (Multiboot 1 → Long Mode + AVX2/FMA/F16C, `/dev/kvm`) | Ejecutado en vivo sobre Ryzen 7 5825U (`/dev/kvm`): sintetiza voz en español 24 kHz S16LE (`6.40 s`, `89.32 dB` SNR, `153 359/153 600` muestras bit-exactas vs NumPy FP32) con Qwen3-TTS-12Hz-0.6B completo (`2 806` instrucciones, 0 errores/avisos, 0 syscalls Linux, nivel L3 W^X); **41.22 s** en 1 vCPU frente a **54.15 s** en PyTorch oficial 8 hilos FP32 (**2.32×** menos RAM) |
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
| Inferencia neuronal bare metal x86-64 (sin Linux) | VMs Multiboot 1 → 64 bits con AVX2/FMA/F16C y pesos `DMCS-L3` vía `-initrd` | **Demostrado en vivo** (`qwen3-tts-baremetal`, 32 KiB ELF: Talker 28L + Code Predictor 5L + Speech Tokenizer 12Hz V2 + BigVGAN en `/dev/kvm`, superando a PyTorch oficial 8 hilos FP32 con 1 solo vCPU) |
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
   - Prohibición explícita de DSL/mini-ensambladores de campos de bits: los desplazamientos y direcciones se resuelven sobre opcodes escritos a mano (desde el 27/11/2026, con los campos simbólicos de la propia herramienta).
3. **Sustrato de audio PCM verificado en vivo (26/11/2026):** protocolo nativo PipeWire/PulseAudio v15 sobre socket Unix (`/run/user/<uid>/pulse/native`, `SO_PEERCRED` sin cookie, drenado no bloqueante de `PA_COMMAND_REQUEST` y mute sin clics) demostrado en `nopray-doom` y documentado en [GRAPHICS-SUBSTRATES.md](GRAPHICS-SUBSTRATES.md).
4. **Hito bare-metal x86-64 e inferencia neuronal `DMCS-L3` (26/11/2026):** `tools/nopray` incorpora `slot.bits` (`16|32|64`), ELF32 Multiboot 1 (`EM_386` bajo `isa = "x86-64"`, `os = "bare-metal"`), trazado CFG de `jmp far` 32→64 bits y alias `jz`/`jnz`/`jc`/`jnc`/`rep outsb`. Verificado en vivo con [examples/qwen3-tts-x86_64-baremetal/](../examples/qwen3-tts-x86_64-baremetal/) (32 KiB, `2 806` instrucciones, 0 errores/avisos, 0 syscalls Linux), batiendo en 1 vCPU a la instalación oficial `qwen-tts` PyTorch (tanto 1 hilo como 8 hilos FP32/BF16) sobre el mismo Ryzen 7 5825U ([BENCHMARKS.md](BENCHMARKS.md)).
5. **Ergonomía para agentes (27/11/2026):** ataca los dos errores medidos en el benchmark de autoría sin que la herramienta codifique instrucciones.
   - **Campos simbólicos** `{kind @símbolo}` en las ranuras: el agente escribe el opcode y la herramienta rellena solo el campo de desplazamiento o dirección (`rel8/16/32`, `abs8/16/32/64`, `a64.imm26/imm19/imm14/adr/adrp/add_lo12`, `rv.jal/branch/pcrel_hi20/pcrel_lo12_i/s/hi20/lo12_i/s`), con rango, alineación e ISA comprobados y rechazo si el campo de la palabra escrita no está a cero. Sustituyen los `relocate.py`/`layout.py` de cada ejecución y cierran la clase de error de `nopray-jb-r3`. La suite reescribe todas las `call` de DOOM, el `adr` y `e_entry` de hello-aarch64 y el par `auipc`/`addi` de hello-riscv64 con campos, y los bytes salen idénticos.
   - **`I001` es error y explica:** nombra el operando que difiere (los números como diferencia) y, si existe, el bit invertido de los bytes escritos que decodifica como la intención (`byte +0 bit 3 (0xe1 -> 0xe9)`). Solo decodifica vecinos a distancia de un bit; no traduce mnemónicos.
   - **`nopray context APP RANURA`:** contrato, llamadores a re-probar, estado tocado, espacio libre, campos que entran y salen, vecinas y hallazgos, en texto o JSON.
   - **`origin`/`evidence` por ranura:** `authored`, `imported`, `reauthored`; `import` sella `imported`, `P001` exige evidencia en lo reescrito y la auditoría imprime el porcentaje escrito a mano. En AgujaESP: la sonda es 100 % autoría y el port PHY declara sus 52 bytes reescritos con evidencia.
6. **La skill como libro (27/11/2026):** la metodología pasa a [`book/`](../book/), nueve capítulos al estilo de un libro de texto. Cada uno trae principio, procedimiento, trampas, ejemplos resueltos con evidencia, problemas con criterios de aceptación y estado del conocimiento. `SKILL.md` es la entrada: qué estudiar según la tarea, los mandamientos y el ciclo del agente. [`book/EDITIONS.md`](../book/EDITIONS.md) fija cómo crece el libro después de cada trabajo, y un test (`tests/doc_links.rs`) comprueba cada enlace y ancla relativos.
7. **Próximos pasos:** primer firmware ESP32-C3 (UART + LED), en emulador y después en placa con copia del firmware; decodificadores Thumb (RP2040) y Xtensa (ESP32-S3) con sus tipos de campo; ejecución diferencial `nopray equiv` para los ports; empaquetador `nopray pack esp`; y primera app verificada sobre Wayland nativo `wl_shm`.
