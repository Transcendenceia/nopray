<p align="center">
  <img src="./assets/icon.svg" width="144" height="144" alt="Icono pixel-art de NOPRAY" />
</p>

<h1 align="center">NOPRAY</h1>

<p align="center">
  <strong>N</strong>ative <strong>O</strong>pcode <strong>P</strong>rotocol: <strong>R</strong>each the <strong>A</strong>lmighty <strong>Y</strong>ourself<br/>
  <em>«No Reces, Habla Directo con Dios» · "Don't Pray — Talk Direct to God"</em>
</p>

<p align="center">
  <code>DMCS-1.1</code> · Sin compilador · Sin ensamblador · Sin enlazador · Sin bibliotecas · Cada byte mapeado y auditado
</p>

<p align="center">
  <a href="./README.md">🇬🇧 Read in English</a> · <strong>🇪🇸 Español</strong> · <a href="./references/MANUAL.md">Manual completo (ES)</a> · <a href="./SKILL.md">Skill para Agentes</a> · <a href="./references/BENCHMARKS.md">Benchmarks Empíricos</a>
</p>

---

## ¿Qué es NOPRAY?

**NOPRAY** es una **skill para agentes de IA, estándar (`DMCS-1.1`) y herramienta de auditoría en Rust** para crear, auditar, modificar y portar ejecutables nativos escribiendo **directamente los bytes de código máquina** (ranuras fijas en hexadecimal + cabeceras ELF/binarias), **sin lenguaje de programación, sin compilador, sin ensamblador, sin enlazador y sin bibliotecas externas** (`libc`, `libX11`, `libpulse` y `libalsa` no existen aquí).

Escribir bytes crudos una sola vez es un truco curioso. Lograr que **otro agente o un humano pueda entenderlos, auditarlos y modificarlos meses después** es el verdadero reto de ingeniería. NOPRAY lo resuelve con tres pilares:

1. **Ranuras virtuales fijas (*fixed slots* en `nopray.toml`):** cada rutina vive en una dirección fija rellena con `NOP` de la arquitectura (`0x90` en x86-64, `1f 20 03 d5` en AArch64). Editar una rutina nunca desplaza las direcciones del resto.
2. **Intenciones por instrucción (`#= ...`):** cada línea de bytes declara qué instrucción quiso codificar el autor. `nopray audit` desensambla los bytes reales y caza errores de codificación (`I001`/`I002`/`I003`) antes de ejecutar nada.
3. **Una herramienta que nunca ensambla (`tools/nopray`):** escrita en Rust, `nopray` coloca los bytes en sus ranuras, los decodifica de vuelta, comprueba flujo de control, lista blanca de syscalls, permisos W^X, contratos de registros (`C011`/`C012`) y punteros interiores de datos (`C017`), y regenera `build/MAP.md` y filtros `seccomp`. **Jamás elige ni codifica una instrucción.**

---

## Demos reales y capturas de pantalla (cero bibliotecas)

Todos los ejemplos viven en [`examples/`](./examples/) y **se reconstruyen byte a byte y se auditan automáticamente** en cada ejecución de `cargo test`.

### 1. VM Bare-Metal `Qwen3-TTS-12Hz-0.6B` — `32 768 B` (`32 KiB`) Síntesis de Voz Neuronal **sin Linux**
- **Ficha auditada:** `DMCS-1.1` · Nivel **`L3` W^X** · **Multiboot 1 ELF32 → Modo Largo 64 bits + AVX2/FMA/F16C** · **0 syscalls Linux** (¡sin kernel ni SO!) · **12 ranuras hex escritas a mano** · **2 806 instrucciones x86-64 decodificadas** · `nopray audit --strict`: **`0 errores, 0 avisos`**.
- **Pipeline completo en bare metal:** arranca en QEMU/KVM (`-enable-kvm -cpu host -smp 1`), construye tablas de página de 2 MiB (`0..4 GiB`), salta de 32 a 64 bits (`jmp far 0x08:boot64`), activa `XCR0=7` (AVX2+FMA+F16C), carga `weights.nqts` (`1.042 GiB`, `DMCS-L3`) cero-copia vía `-initrd`, ejecuta **Talker 28L + Code Predictor 5L** + **Speech Tokenizer 12Hz V2 + BigVGAN/SnakeBeta**, emite el WAV 24 kHz S16LE por `isa-debugcon` (`0xE9` `rep outsb`) y apaga limpio.
- **Audio real verificado (*«Hola Marlon, esto es código máquina puro con nopray, sin Linux y sin compilador.»*):**
  - 🔊 **Audio bare-metal NOPRAY (`6.40 s`, `301 KiB`):** [`assets/audio/nopray_qwen3_tts_baremetal_es.wav`](./assets/audio/nopray_qwen3_tts_baremetal_es.wav) — **`89.32 dB` SNR / `0.9999999994` coseno** vs NumPy FP32 (`153 359 / 153 600` muestras PCM bit-exactas, diferencia máx. `±1 LSB`).
  - 🔊 **Referencia oficial `qwen-tts` PyTorch (`6.72 s`, `316 KiB`):** [`assets/audio/official_qwen3_tts_pytorch_es.wav`](./assets/audio/official_qwen3_tts_pytorch_es.wav).
- **Frente a frente en el mismo AMD Ryzen 7 5825U:** **1 vCPU bare-metal NOPRAY (`41.22 s` síntesis / `53.04 s` cold→WAV / `2.37 GiB` RSS / `32 KiB` ELF / `0` syscalls)** superó a **todas** las configuraciones oficiales de PyTorch CPU — incluida **`8 hilos FP32`** (`54.15 s` / `63.51 s` / `5.52 GiB` RSS / `1.375 GiB` `.venv` / `134 879+` syscalls solo al importar y cargar) y **`8 hilos BF16`** (`411.52 s`). Detalle en [`examples/qwen3-tts-x86_64-baremetal/`](./examples/qwen3-tts-x86_64-baremetal/) y [`references/BENCHMARKS.md`](./references/BENCHMARKS.md#part-3--bare-metal-neural-tts-32-kib-nopray-kvm-vs-official-qwen-tts-pytorch).

### 2. Transcendence DOOM — `32 768 B` (`32 KiB`) FPS 3D x86-64 + Audio PCM 24 kHz en vivo
- **Ficha auditada:** `DMCS-1.1` · Nivel **`L2` W^X** · `PT_GNU_STACK rw-` · **6 syscalls totales** (`socket`, `connect`, `read`, `write`, `poll`, `exit`) · **2 844 instrucciones x86-64 decodificadas** · **2 844 intenciones `#=` (0 discrepancias)** · `nopray audit --strict`: **`0 errores, 0 avisos`**.
- **Gráficos:** protocolo X11/Xwayland crudo (`/tmp/.X11-unix/X0`, `640×400` BGRA en 4 franjas `PutImage`), 7 texturas procedurales `64×64` y 6 sprites `32×32` RGBA generados al arranque en `.bss`, suelo y techo en perspectiva en punto fijo `16.16`, raycaster DDA SSE2 de 640 columnas con `zbuffer`, niebla SWAR de 256 niveles e iluminación por fogonazo, cañón de plasma bi-raíl animado, IA de Sentinels + bolas de plasma, puertas blindadas (`E`), HUD completo y radar minimapa (`M`).
- **Audio PCM directo:** protocolo nativo **PipeWire / PulseAudio** sobre `/run/user/<uid>/pulse/native` (`24 kHz S16LE mono`, cero `libpulse`, cero subprocesos y **cero syscalls adicionales**), con sintetizador de 5 voces en tiempo real (disparo plasma, explosiones con ruido LFSR + sub-grave, arpegio de recogida/puerta, aviso de daño y pulso de bajo industrial en Mi menor) y **mute instantáneo sin clics** (`N` o tecla multimedia `Mute`).

| En vivo en CachyOS / Hyprland (`PCM ON`) | Combate: fogonazo y flash blanco de impacto | Mute instantáneo (`MUTED  N`) |
| :---: | :---: | :---: |
| ![Transcendence DOOM en vivo](./assets/screenshots/doom-live-cachyos.png) | ![Transcendence DOOM disparo](./assets/screenshots/doom-combat-flash.png) | ![Transcendence DOOM mute](./assets/screenshots/doom-muted-hud.png) |

### 3. Otros binarios de referencia verificados

| `nopray3d` — `8 192 B` Raycaster W^X 60 FPS | `nopray3d` — Sombreado de muros | `calc` — `4 096 B` (1 página OS) GUI X11 |
| :---: | :---: | :---: |
| ![NOPRAY 3D Gran Salón](./assets/screenshots/nopray3d-grand-hall.png) | ![NOPRAY 3D Muros](./assets/screenshots/nopray3d-crimson-gold.png) | ![Calculadora NOPRAY 4 KiB](./assets/screenshots/calc-live.png) |

---

### 4. AgujaESP-NOPRAY · prueba de arranque ESP32-S3

Una [imagen de 160 bytes escrita a mano](examples/agujaesp-nopray-s3/README.es.md) arranca desde la flash simulada en Espressif QEMU y escribe por UART. Incluye mapa, traza de MMIO y prueba de rechazo de checksum. **Solo emulación; sin Wi‑Fi, alta en la tailnet ni OTA.**

## La realidad de realidades: resultados empíricos sin maquillaje

Comparamos **NOPRAY** frente a **C (`gcc 14.2 -O2 -static`)**, **Rust (`rustc --release`)**, **Go (`CGO_ENABLED=0`)**, **Node.js (`v24`)** y **Python (`3.14`)** en ejecución real (`72/72` celdas bajo carga moderada, limitada y pesada) y en autoría, mantenimiento y portado multi-modelo (`70/70` celdas, `9 990/9 990` pruebas ocultas y auditoría forense de 12 ejecuciones). Detalle íntegro en [`references/BENCHMARKS.md`](./references/BENCHMARKS.md).

- **Dónde NOPRAY arrasa:**
  - **Arranque:** `330 µs` (`2×` más rápido que C estático, `2.4×` que Rust, `7.6×` que Go, `131×` que Python, `305×` que Node.js; con `CPUQuota=50%`, **`868×`** más rápido que Node.js).
  - **Huella y RAM viva:** **`1.2 KiB`** en disco y **`80 KiB` de RSS real** (`9.2×` menos RAM que C estático, `14×` menos que Rust, `824×` menos que Node.js).
  - **Superficie de syscalls:** solo **2–3 syscalls** distintas (frente a `13–14` en C, `21` en Rust, `34` en Python y `44` en Node.js).
- **En cómputo sostenido:**
  - Empata con C/Rust en criba de 300 millones (`3.11 s`) y hash FNV-1a de 512 MiB (`1.22 s` vs `1.20 s`).
  - En código con muchas ramas (`Collatz 10⁷`), el código ingenuo pierde (`3.69 s` vs `3.04 s` C), pero un bucle artesanal sin ramas (`shr + cmovb` alineado a 16 B en `nopray-opt`) **bate a todos**: **`2.456 s`** frente a Rust `2.535 s` y C `3.039 s` (`−33%`).
- **El coste real que no escondemos:**
  - **Escribir y mantener cuesta ~2× más que en C:** `555 s` y `2.90M` tokens frente a `272 s` y `1.45M` tokens en C (`3.5×` hasta el primer verde en Fase A).
  - **Portar de x86-64 a AArch64 cuesta reescribir** (`361 s` y `1.53M` tokens), mientras C, Rust y Go recompilan gratis.
  - **Lo que la auditoría NO prueba:** la pureza del proceso (sin compilador ni DSLs de campos de bits) es una declaración del autor, y sin compilador no hay comprobación automática de límites, canarios de pila ni ASLR. Para criptografía, TLS o sistemas grandes mantenidos por humanos, **Rust o C siguen siendo la herramienta correcta**.

---

## Inicio rápido

```bash
git clone https://github.com/Transcendenceia/nopray.git
cd nopray

# 1. Suite completa (reconstruye y audita byte a byte TODOS los ejemplos)
(cd tools/nopray && cargo test)

# 2. Compilar sin compilador y auditar en modo estricto Transcendence DOOM
./tools/nopray.sh build examples/nopray-doom-x86_64-cachyos
./tools/nopray.sh audit --strict examples/nopray-doom-x86_64-cachyos

# 3. Ejecutar Transcendence DOOM en Linux X11 / Xwayland (+ audio PipeWire PCM)
./examples/nopray-doom-x86_64-cachyos/build/nopray-doom
```

### Ayudas para agentes de IA (la herramienta nunca codifica instrucciones)

- **Campos simbólicos:** `e8 {rel32 @write_all}`, `{a64.imm26 @loop : 00 00 00 14}`: el agente escribe el opcode y la herramienta rellena solo el desplazamiento o la dirección declarada, con comprobación de rango.
- **Pistas de `I001`:** el error dice qué operando difiere y, si existe, qué bit invertido de tus bytes decodifica como tu intención.
- **`nopray context APP RANURA`:** contrato, llamadores a re-probar, estado, espacio libre y vecinas de una ranura, sin cargar todo el programa.
- **`origin`/`evidence` por ranura:** la auditoría informa del porcentaje escrito a mano; una importación no se presenta como port.

La skill enseña un método, no un catálogo de demos. [`SKILL.md`](./SKILL.md) es la puerta de entrada a [`book/`](./book/), nueve capítulos escritos como un libro de texto (en inglés): el mapa, la codificación, hablar con un sistema operativo, bare metal, portar, rendimiento, cambiar un programa, exposición y evidencia en hardware. Cada capítulo trae principio, procedimiento, trampas, ejemplos resueltos de este repositorio, problemas con criterios de aceptación y estado del conocimiento. Las aplicaciones de arriba son sus ejercicios resueltos. El libro crece después de cada trabajo ([`book/EDITIONS.md`](./book/EDITIONS.md)).

Documentación detallada en español: [`references/MANUAL.md`](./references/MANUAL.md). Licencia MIT — © 2026 [TranscendenceIA](https://github.com/Transcendenceia).
