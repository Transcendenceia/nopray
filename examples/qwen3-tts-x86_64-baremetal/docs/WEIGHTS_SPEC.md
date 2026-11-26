# WEIGHTS_SPEC.md — `weights.nqts` v1 (DMCS-L3, bare-metal x86-64 AVX2)

Estado: **diseño v1 (2026-11-26)**, escrito por OpenClaw `claude-opus-5-5`. Fuente: `refs/qwen3-tts/{config.json,tokenizer_config.json,modeling_qwen3_tts*.py}` + `artifacts/{talker,tokenizer}_tensors.txt`.
Productor: `scripts/pack_weights.py` (Track A). Consumidor: slots hex de la VM (Track B) + `scripts/numpy_ref.py`.
Los números de bytes de este documento se calcularon a partir de los shapes reales; el empaquetador debe reproducirlos **exactamente** (`assert`).

## 0. Principios

1. **Little-endian, sin parsing de strings en la VM.** Cada tensor tiene un `id` numérico fijo (§3); la VM localiza un tensor como `base + dir[id].off`.
2. **Todo offset y todo plano de datos alineado a 64 B** (línea de caché; `vmovaps`/`vmovdqa` de 32 B siempre válidos). Padding = `0x00`.
3. **Cero trabajo en la carga:** todo lo derivable offline (proyección de texto, RVQ, SnakeBeta, RoPE, layer-scale) llega precomputado. La VM no calcula `exp`, `sin`, `cos` ni divisiones sobre pesos.
4. **Archivo = imagen en RAM:** QEMU `-initrd weights.nqts` (módulo Multiboot 1) se usa **in situ**, sin copiar. `mod_start` debe ser múltiplo de 4 KiB (QEMU lo alinea a página); si no lo fuera, la VM aborta con código `E_ALIGN` por COM1.

## 1. Tipos de dato (`dtype`)

| código | nombre | almacenamiento | uso |
|---|---|---|---|
| 0 | `F32` | IEEE-754 binary32 | normas, biases, vectores, RoPE, RVQ, Snake |
| 1 | `F16` | binary16 (`vcvtph2ps`, F16C) | text embedding proyectado, Decoder conv/linear |
| 2 | `Q8_0S` | Q8_0 *split-plane* (abajo) | todas las matrices lineales Talker/CP + heads/embeddings codec |
| 3 | `I32` | int32 | tablas de ids (speaker/lang/tokens especiales) |

### `Q8_0S` (Q8_0 semántico, layout AVX2)
Semántica idéntica a Q8_0 de ggml: bloques de 32 pesos a lo largo de la dimensión de reducción `K`, `d = max|w|/127` (fp32), `q = round_half_even(w/d)` clamp `[-127,127]`, `w ≈ d·q`. Si `max|w| = 0` → `d = 0`, `q = 0`.
Diferencia: en vez de intercalar bloques de 36 B (rompen la alineación 64), se almacenan **dos planos**:
- `qs`: `int8[R][K]` fila-mayor, en `off` (alineado 64; `K ∈ {1024,2048,3072}` ⇒ cada fila también lo está).
- `sc`: `f32[R][K/32]` en `off2` (alineado 64).
Coste = `R·K·1.125` B, igual que Q8_0. Kernel de referencia (por bloque): `vpmovsxbw`→`vpmaddwd` con la activación cuantizada a Q8 (o `vcvtdq2ps`+`vfmadd231ps` contra activación F32) × `sc`. La conversión de pesos BF16→F32 previa a cuantizar es exacta.
Orientación: `W` se guarda como en PyTorch `[out, in]` (`R = out`, `K = in`); `y = W·x`.

## 2. Formato del archivo

```
0x00000000  Header           4096 B
0x00001000  Directorio       65536 B  (1024 entradas × 64 B)
0x00011000  Datos            tensores en orden de id ascendente, cada plano alineado a 64
EOF         (sin trailer; el CRC global está en el header)
```

### 2.1 Header (4096 B; campos u32 salvo indicación)

| off | campo | valor |
|---|---|---|
| 0x000 | `magic[8]` | `"NQTS\x00\x01\r\n"` (bytes `4E 51 54 53 00 01 0D 0A`) |
| 0x008 | `version` | `1` |
| 0x00C | `header_size` | `4096` |
| 0x010 | `dir_off` (u64) | `0x1000` |
| 0x018 | `dir_count` | nº de entradas usadas (≤1024) |
| 0x01C | `align` | `64` |
| 0x020 | `data_off` (u64) | `0x11000` |
| 0x028 | `file_size` (u64) | tamaño total exacto |
| 0x030 | `flags` | bit0 `TEXT_EMB_Q8` (0 = F16 por defecto), bit1 `LAYERSCALE_FOLDED`=1, bit2 `RVQ_PREPROJ`=1 |
| 0x034 | `crc32_data` | CRC-32 (IEEE) de `[data_off, file_size)` — verificado por el host, opcional en VM |
| 0x040 | **dims Talker** | `hidden=1024, n_layers=28, n_heads=16, n_kv=8, head_dim=128, inter=3072, codec_vocab=3072, text_vocab=151936, rms_eps=1e-6 (f32), rope_theta=1e6 (f32)` |
| 0x080 | **dims CP** | `hidden=1024, n_layers=5, n_heads=16, n_kv=8, head_dim=128, inter=3072, vocab=2048, n_groups=16, rms_eps=1e-6, rope_theta=1e6` |
| 0x0C0 | **dims Decoder** | `codebook_dim=512, codebook_size=2048, n_q=16, latent=1024, decoder_dim=1536, tf_hidden=512, tf_layers=8, tf_heads=16, tf_head_dim=64, tf_inter=1024, sliding_window=72, rms_eps=1e-5, rope_theta=1e4, upsample_ratios=[2,2], upsample_rates=[8,5,4,3], total_upsample=1920, sample_rate=24000` |
| 0x140 | **tokens** | `tts_pad=151671, tts_bos=151672, tts_eos=151673, im_start=151644, im_end=151645, assistant=77091, codec_pad=2148, codec_bos=2149, codec_eos=2150, codec_think=2154, codec_nothink=2155, think_bos=2156, think_eos=2157, lang_spanish=2054, spk_serena=3066, spk_ryan=3061` |
| 0x1C0 | **RoPE** | `rope_tk_max_pos=4096, rope_tk_half=64, rope_dec_max_pos=4096, rope_dec_half=32` |
| 0x200 | **runtime caps** | `kv_max_tokens=1024, max_frames=512 (≈41 s)` — la VM dimensiona KV/activaciones con esto |
| 0x240–0xFFF | reservado | ceros (incluye `sha256` del `model.safetensors` origen en 0x800, 32 B) |

Todo el header es de tamaño fijo: la VM lee dims por offset absoluto, sin bucles de parsing.

### 2.2 Entrada de directorio (64 B)

| off | tipo | campo |
|---|---|---|
| 0 | u32 | `id` (§3) |
| 4 | u16 | `dtype` |
| 6 | u16 | `ndim` |
| 8 | u32[4] | `shape` (orden PyTorch, sin usar = 1) |
| 24 | u64 | `off` (desde inicio de archivo; plano principal) |
| 32 | u64 | `off2` (plano de escalas `Q8_0S`, si no 0) |
| 40 | u64 | `nbytes` (plano principal, sin padding) |
| 48 | u32 | `name_hash` FNV-1a 32 del nombre safetensors (diagnóstico) |
| 52 | u32 | `crc32` del/los plano(s) |
| 56 | u64 | reservado = 0 |

El directorio está **indexado por id** (`dir[id]`): entrada vacía = todo ceros. Así la VM no busca.

## 3. Mapa de ids (fijo, aritmético)

`id = base + layer·stride + slot`.

**Talker (base 0, stride 16, layers 0..27 ⇒ ids 0..447)** · **CP (base 448, stride 16, layers 0..4 ⇒ 448..527)**

| slot | tensor | dtype | shape |
|---|---|---|---|
| 0 | `input_layernorm.weight` | F32 | [1024] |
| 1 | `self_attn.q_proj` | Q8_0S | [2048,1024] |
| 2 | `self_attn.k_proj` | Q8_0S | [1024,1024] |
| 3 | `self_attn.v_proj` | Q8_0S | [1024,1024] |
| 4 | `self_attn.q_norm.weight` | F32 | [128] |
| 5 | `self_attn.k_norm.weight` | F32 | [128] |
| 6 | `self_attn.o_proj` | Q8_0S | [1024,2048] |
| 7 | `post_attention_layernorm.weight` | F32 | [1024] |
| 8 | `mlp.gate_proj` | Q8_0S | [3072,1024] |
| 9 | `mlp.up_proj` | Q8_0S | [3072,1024] |
| 10 | `mlp.down_proj` | Q8_0S | [1024,3072] |
| 11–15 | reservado | — | — |

Orden de datos dentro de la capa = orden de slot ⇒ una capa es un bloque contiguo de **17 703 936 B** (verificable: `dir[id(L+1,0)].off − dir[id(L,0)].off`). RMSNorm se mantiene F32 sin plegar (QK-Norm impide plegar q_norm en q_proj).

**Globales (528..)**

| id | tensor | dtype | shape | nota |
|---|---|---|---|---|
| 528 | `talker.model.norm.weight` | F32 | [1024] | |
| 529 | `talker.codec_head` | Q8_0S | [3072,1024] | logits codebook 0 |
| 530 | `talker.model.codec_embedding` | Q8_0S | [3072,1024] | gather fila → dequant |
| 531 | **`text_emb_proj`** | F16 (o Q8_0S si flag bit0) | [151936,1024] | §4.1 |
| 532 | `cp.model.norm.weight` | F32 | [1024] | |
| 533–547 | `cp.lm_head.{0..14}` | Q8_0S | [2048,1024] | grupos 1..15 |
| 548–562 | `cp.model.codec_embedding.{0..14}` | Q8_0S | [2048,1024] | |
| 563 | `rope_tk_cos` | F32 | [4096,64] | §4.3, compartida Talker+CP |
| 564 | `rope_tk_sin` | F32 | [4096,64] | |
| 565 | `rope_dec_cos` | F32 | [4096,32] | |
| 566 | `rope_dec_sin` | F32 | [4096,32] | |
| 567 | `rvq_table` | F32 | [16,2048,512] | §4.4 |
| 568–575 | reservado | | | |

`small_to_mtp_projection` es `Identity` (comprobado en WORK.md): no se almacena. `text_projection.*` no se almacena (consumido por §4.1).

**Decoder (base 576)** — dtype: matrices/convs `F16` (layout PyTorch `[out,in,k]` para Conv1d y `[in,out,k]` para ConvTranspose1d, tal cual), vectores `F32`.

| ids | contenido |
|---|---|
| 576–577 | `pre_conv.conv.{weight [1024,512,3], bias}` |
| 578–579 | `pre_transformer.input_proj.{weight [512,1024], bias}` |
| 580 + 8·l + s (l=0..7) | s0 `input_layernorm`, s1 q [1024,512], s2 k, s3 v, s4 o′ [512,1024], s5 `post_attention_layernorm`, s6 gate [1024,512], s7 up; **down′ [512,1024] en 644+l** |
| 652 | `pre_transformer.norm.weight` |
| 653–654 | `pre_transformer.output_proj.{weight [1024,512], bias}` |
| 656 + 16·u + s (u=0,1) | `upsample.u`: s0/s1 transconv w[1024,1024,2]/b; s2/s3 dwconv w[1024,1,7] **F32**/b; s4/s5 `norm.{weight,bias}`; s6/s7 pwconv1 w[4096,1024]/b; s8/s9 pwconv2 w[1024,4096]/b; s10 `gamma` |
| 688–689 | `decoder.0.conv.{weight [1536,1024,7], bias}` |
| 690 + 32·b + s (b=0..3 = `decoder.{1..4}`) | s0 `snake.exp_alpha`, s1 `snake.inv_beta`, s2/s3 transconv w/b; residual unit r=0..2 en s4+8r: `act1.exp_alpha`, `act1.inv_beta`, conv1 w[C,C,7]/b, `act2.exp_alpha`, `act2.inv_beta`, conv2 w[C,C,1]/b |
| 818–819 | `decoder.5` Snake `exp_alpha`, `inv_beta` [96] |
| 820–821 | `decoder.6.conv.{weight [1,96,7], bias [1]}` |

Dilataciones (1,3,9), kernels de transconv `2·rate` y stride `rate` son implícitos por `b`/`r` y viven en el header (0x0C0), no en el directorio.
Excluido por completo: `encoder.*` (solo hace falta para clonar voz; CustomVoice usa speaker id) y `decoder.quantizer.*.input_proj` (solo encode).

## 4. Precomputaciones offline (en FP64, redondeo final a dtype destino)

### 4.1 Text embedding pre-proyectado
`text_emb_proj[t] = fc2( SiLU( fc1(E_text[t]) + b1 ) ) + b2` para **todo** el vocabulario (151936 filas), replicando exactamente `text_projection` (`linear_fc1 2048→2048`, SiLU, `linear_fc2 2048→1024`, con bias). Es una función fila a fila sin interacción entre tokens ⇒ **exacta** (salvo redondeo). Se calcula en FP32 desde BF16 (igual que PyTorch CPU con `dtype=float32`) y se guarda:
- **por defecto F16** (311 164 928 B; error relativo ≈ 5·10⁻⁴, cubre el rango; si alguna fila supera ±65504 el empaquetador falla en vez de saturar),
- variante **Q8_0S** con flag bit0 (175 030 272 B) para `-m 2G` justo.
Opción descartada: tabla solo del prompt — rompe la generalidad y el ahorro (~136 MB) ya lo da Q8_0S.

### 4.2 Layer-scale del Decoder plegado
`o′ = diag(self_attn_layer_scale)·o_proj`, `down′ = diag(mlp_layer_scale)·down_proj` (escala por fila de salida, exacto en FP64 antes de F16). El ConvNeXt `gamma` se guarda aparte (va tras pwconv2 en el residual; se podría plegar igual — **pendiente de confirmar en código** antes de plegarlo, por eso v1 no lo pliega).

### 4.3 RoPE
`inv_freq[i] = θ^(−2i/d)`, `cos[p][i] = cos(p·inv_freq[i])`, `sin` análogo, FP64→F32.
- Talker/CP: θ=1e6, d=128 ⇒ i∈[0,64), p∈[0,4096). M-RoPE degenera en 1D porque los 3 índices de posición son idénticos en TTS (WORK.md) ⇒ tabla única compartida. CP usa posiciones 0..16 de la misma tabla.
- Decoder: θ=1e4, d=64 ⇒ i∈[0,32), p∈[0,4096).
Convención **rotate_half** (HF): `x′[i] = x[i]·c − x[i+half]·s`, `x′[i+half] = x[i+half]·c + x[i]·s`. Debe validarse contra `golden_ref_greedy.npz`.

### 4.4 RVQ pre-dividido y pre-proyectado
`E_k = embedding_sum_k / clamp(cluster_usage_k, 1e-5)[:,None]` ([2048,256]). Como `output_proj` es Conv1d k=1 **sin bias** y lineal, la suma se reordena exactamente:
`rvq_table[0] = W_out_first · E_0ᵀ` y `rvq_table[k] = W_out_rest · E_kᵀ` (k=1..15), ambos [2048,512].
Decode en la VM = `h[t] = Σ_k rvq_table[k][code_k[t]]` (16 gathers + sumas F32, sin matmul). 67 108 864 B. (Si `output_proj` resultara tener bias, se suma una vez al final; el empaquetador lo verifica.)

### 4.5 SnakeBeta precomputado
`y = x + inv_beta · sin²(exp_alpha · x)` con `exp_alpha = exp(alpha)`, `inv_beta = 1/(exp(beta) + 1e-9)` (constante `no_div_by_zero` del código oficial). Ambos F32 por canal. La VM sigue necesitando `sin` sobre activaciones (polinomio minimax en el slot; fuera del alcance de este archivo).

## 5. Presupuesto exacto de bytes

| sección | B |
|---|---:|
| Header + directorio | 69 632 |
| Talker 28 capas (17 703 936 × 28) | 495 710 208 |
| CP 5 capas | 88 519 680 |
| codec_head + codec_emb Talker + 15 lm_head + 15 codec_emb CP + 2 normas | 77 864 960 |
| text_emb_proj F16 | 311 164 928 |
| Decoder matrices/convs F16 | 210 576 704 |
| Decoder vectores F32 (bias, normas, Snake, dwconv, gamma) | 356 544 |
| RVQ table | 67 108 864 |
| RoPE Talker/CP + Decoder | 3 145 728 |
| **Total `weights.nqts` (F16 text emb)** | **1 254 517 248 B = 1.168 GiB** |
| Total variante `TEXT_EMB_Q8` | 1 118 382 592 B = 1.042 GiB |

(±padding de entradas F32 pequeñas: el empaquetador imprime el total real y debe coincidir con estas cifras a ±64 B por tensor vectorial; cualquier otra diferencia es bug.) Ambos < 1.2 GiB.

### 5.1 RAM del guest (runtime caps del header)

| bloque | B |
|---|---:|
| Imagen ELF NOPRAY + pila + tablas de página (identity 4 GiB con páginas 2 MiB) | ≈ 1 MiB |
| KV cache Talker F32: 28·2·8·128·4 B/token × 1024 | 234 881 024 |
| KV cache CP (17 pos × 5 capas) | 348 160 |
| Activaciones Talker/CP (h, q, k, v, mlp 3072, logits 3072/2048, x8 en Q8) | < 1 MiB |
| Decoder pipeline por **chunks de 25 frames (2 s) + contexto causal**, ping-pong: peor capa 96 ch × 48 000 muestras × 4 B × 2 buffers + transformer (KV ventana 72) | ≈ 40 MiB |
| WAV de salida (max_frames 512 · 1920 · 2 B, s16le) | 1 966 080 |
| **Total runtime** | ≈ 280 MiB |

| QEMU `-m` | pesos | runtime | margen (memoria baja reservada ≈ 1 MiB, BIOS/ACPI ≈ 1 MiB) |
|---|---:|---:|---:|
| **3G** (recomendado) | 1.168 GiB | 0.27 GiB | ≈ 1.55 GiB libre |
| **2G** | 1.168 GiB | 0.27 GiB | ≈ 0.55 GiB libre — válido; con `kv_max_tokens=2048` (+224 MiB) aún cabe |
| 2G + `TEXT_EMB_Q8` | 1.042 GiB | 0.27 GiB | ≈ 0.68 GiB |

Nota: en KVM, `-m` reserva RAM del host bajo demanda; RSS del proceso QEMU en el host ≈ pesos tocados + runtime + overhead QEMU (~100–200 MiB), medir con `/proc/<pid>/status:VmHWM`. El módulo Multiboot se carga por debajo de 4 GiB, así que `-m` > 3.5G no aporta.

### 5.2 Disco (staging KVM)
`weights.nqts` 1 254 517 248 B + `nopray-qwen-tts.elf` (32 KiB) + WAV (≤ 2 MiB) ⇒ **≈ 1.17 GiB** (variante Q8: 1.04 GiB).

## 6. Validación obligatoria (Track A)
1. `pack_weights.py --check`: magic, tamaños §5, alineación 64 de cada `off/off2`, CRCs, ids sin colisión.
2. `numpy_ref.py` lee **solo** `weights.nqts` (nunca safetensors) y compara contra `artifacts/golden_ref_greedy.npz`: coseno ≥ 0.999 en hidden de Talker (capa 27) y CP, igualdad exacta de códigos greedy los primeros N frames, SNR wav ≥ 30 dB frente a `official_es_greedy.wav`.
3. Error de cuantización Q8_0S por tensor (rel. Frobenius) registrado; alarma si > 1 %.

## 7. Abiertos
- Confirmar posición exacta de `gamma` en ConvNeXt antes de plegarlo (v2).
- Confirmar que `rvq_*.output_proj` no tiene bias (el listado de tensores no muestra bias ⇒ esperado).
- Elegir activación Q8 (`vpmaddubsw` con offset) vs F32 en el kernel: no cambia el formato.
