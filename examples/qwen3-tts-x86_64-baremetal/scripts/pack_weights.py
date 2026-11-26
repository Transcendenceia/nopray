#!/usr/bin/env python3
"""Pack Qwen3-TTS-12Hz-0.6B-CustomVoice into weights.nqts (see docs/WEIGHTS_SPEC.md, v1).

Usage: .venv/bin/python scripts/pack_weights.py [--text-emb q8|f16] [--out artifacts/weights.nqts]
                                                 [--prompt-ids 151644,77091,...]
Torch is used only here (safetensors BF16 loading + offline precomputation).
"""
import argparse
import hashlib
import json
import struct
import sys
import time
import zlib
from pathlib import Path

import numpy as np
import torch
from safetensors import safe_open

ROOT = Path(__file__).resolve().parent.parent
ALIGN = 64
HEADER_SIZE = 4096
DIR_CAP = 1024
DIR_OFF = HEADER_SIZE
DATA_OFF = HEADER_SIZE + DIR_CAP * 64  # 0x11000

DT_F32, DT_F16, DT_Q8, DT_I32 = 0, 1, 2, 3
FLAG_TEXT_EMB_Q8, FLAG_LS_FOLDED, FLAG_RVQ_PREPROJ, FLAG_GAMMA_FOLDED = 1, 2, 4, 8

# Default prompt: the exact ids of the official greedy baseline (artifacts/official_baseline_init.json)
DEFAULT_PROMPT = [151644, 77091, 198, 68012, 2876, 12212, 11, 37793, 1531, 44059, 135702, 281, 2798,
                  390, 65026, 11611, 11, 7437, 14340, 379, 7437, 96931, 5364, 13, 151645, 198,
                  151644, 77091, 198]

TK_LAYERS, CP_LAYERS, DEC_LAYERS = 28, 5, 8
ID_CP = 448
ID_G = 528
ID_DEC = 576
ROPE_MAX = 4096


def A(n):
    return (n + ALIGN - 1) // ALIGN * ALIGN


def fnv1a(s):
    h = 0x811C9DC5
    for b in s.encode():
        h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF
    return h


def q8_0s(w):
    """w: float32 [R, K] -> (qs int8 [R,K], sc float32 [R,K/32]). Q8_0 semantics, float32 math."""
    w = np.ascontiguousarray(w, dtype=np.float32)
    R, K = w.shape
    assert K % 32 == 0, (R, K)
    b = w.reshape(R, K // 32, 32)
    amax = np.abs(b).max(axis=2)
    d = (amax / np.float32(127.0)).astype(np.float32)
    inv = np.where(d > 0, np.float32(1.0) / np.where(d > 0, d, 1), 0).astype(np.float32)
    q = np.rint(b * inv[:, :, None]).clip(-127, 127).astype(np.int8)
    return q.reshape(R, K), d


class Packer:
    def __init__(self, path):
        self.f = open(path, "w+b")
        self.f.write(b"\0" * DATA_OFF)
        self.pos = DATA_OFF
        self.dir = {}
        self.stats = {}

    def _plane(self, arr):
        off = self.pos
        raw = np.ascontiguousarray(arr).tobytes()
        self.f.write(raw)
        pad = A(len(raw)) - len(raw)
        self.f.write(b"\0" * pad)
        self.pos += len(raw) + pad
        return off, raw

    def add(self, tid, name, arr, dtype):
        assert tid not in self.dir and 0 <= tid < DIR_CAP, tid
        if dtype == DT_Q8:
            w = np.asarray(arr, dtype=np.float32)
            qs, sc = q8_0s(w)
            deq = qs.reshape(w.shape[0], -1, 32).astype(np.float32) * sc[:, :, None]
            err = float(np.linalg.norm(deq.reshape(w.shape) - w) / max(np.linalg.norm(w), 1e-30))
            off, r1 = self._plane(qs)
            off2, r2 = self._plane(sc)
            shape, nbytes, crc = w.shape, len(r1), zlib.crc32(r2, zlib.crc32(r1))
        else:
            npdt = {DT_F32: np.float32, DT_F16: np.float16, DT_I32: np.int32}[dtype]
            src = np.asarray(arr)
            a = src.astype(npdt)
            if dtype == DT_F16:
                assert np.isfinite(a).all(), f"F16 overflow in {name}"
                err = float(np.linalg.norm(a.astype(np.float32) - src) / max(np.linalg.norm(src), 1e-30))
            else:
                err = 0.0
            off, r1 = self._plane(a)
            off2, shape, nbytes, crc = 0, a.shape, len(r1), zlib.crc32(r1)
        shp = list(shape) + [1] * (4 - len(shape))
        assert len(shape) <= 4
        self.dir[tid] = struct.pack("<IHH4IQQQIIQ", tid, dtype, len(shape), *shp, off, off2, nbytes,
                                    fnv1a(name), crc, 0)
        self.stats[tid] = (name, dtype, tuple(shape), err)

    def finish(self, header):
        size = self.pos
        self.f.seek(DIR_OFF)
        for tid in range(DIR_CAP):
            self.f.write(self.dir.get(tid, b"\0" * 64))
        # global CRC of data
        self.f.seek(DATA_OFF)
        crc = 0
        while True:
            blk = self.f.read(1 << 24)
            if not blk:
                break
            crc = zlib.crc32(blk, crc)
        header[0x028:0x030] = struct.pack("<Q", size)
        header[0x018:0x01C] = struct.pack("<I", len(self.dir))
        header[0x034:0x038] = struct.pack("<I", crc)
        self.f.seek(0)
        self.f.write(bytes(header))
        self.f.close()
        return size, crc


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for blk in iter(lambda: f.read(1 << 24), b""):
            h.update(blk)
    return h.digest()


def rope_tables(theta, d, n):
    inv = 1.0 / (theta ** (np.arange(0, d, 2, dtype=np.float64) / d))
    fr = np.arange(n, dtype=np.float64)[:, None] * inv[None, :]
    return np.cos(fr).astype(np.float32), np.sin(fr).astype(np.float32)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=str(ROOT / "model/model.safetensors"))
    ap.add_argument("--tok", default=str(ROOT / "model/speech_tokenizer/model.safetensors"))
    ap.add_argument("--out", default=str(ROOT / "artifacts/weights.nqts"))
    ap.add_argument("--text-emb", choices=["q8", "f16"], default="q8")
    ap.add_argument("--prompt-ids", default=None, help="comma list of input_ids (default: baseline prompt)")
    ap.add_argument("--speaker", default="serena")
    ap.add_argument("--language", default="spanish")
    ap.add_argument("--max-new-tokens", type=int, default=160)
    ap.add_argument("--threads", type=int, default=8)
    ap.add_argument("--lm-dtype", choices=["q8", "f16"], default="q8",
                    help="Talker/CP matrices dtype (f16 = verification variant, ~2 GiB)")
    args = ap.parse_args()
    torch.set_num_threads(args.threads)
    t0 = time.time()

    cfg = json.loads((ROOT / "model/config.json").read_text())
    tcfg = cfg["talker_config"]
    prompt = [int(x) for x in args.prompt_ids.split(",")] if args.prompt_ids else DEFAULT_PROMPT
    assert prompt[:3] == [151644, 77091, 198] and prompt[-5:] == [151645, 198, 151644, 77091, 198], \
        "prompt must be <|im_start|>assistant\\n ... <|im_end|>\\n<|im_start|>assistant\\n"
    spk_id = tcfg["spk_id"][args.speaker]
    lang_id = tcfg["codec_language_id"][args.language]

    fm = safe_open(args.model, framework="pt", device="cpu")
    ft = safe_open(args.tok, framework="pt", device="cpu")
    gm = lambda k: fm.get_tensor(k).float()
    gt = lambda k: ft.get_tensor(k).float()
    npf = lambda t: t.numpy().astype(np.float32)

    # --- sanity checks requested by coordinator
    tkeys = set(ft.keys())
    for k in ["decoder.quantizer.rvq_first.output_proj.bias", "decoder.quantizer.rvq_rest.output_proj.bias"]:
        assert k not in tkeys, f"unexpected bias {k}"
    print("[check] rvq_first/rvq_rest.output_proj: bias=False (no tensor)")

    P = Packer(args.out)

    LMQ = DT_Q8 if args.lm_dtype == "q8" else DT_F16

    def tf_layer(base, prefix):
        m = lambda n: gm(prefix + n)
        P.add(base + 0, prefix + "input_layernorm.weight", npf(m("input_layernorm.weight")), DT_F32)
        P.add(base + 1, prefix + "self_attn.q_proj.weight", npf(m("self_attn.q_proj.weight")), LMQ)
        P.add(base + 2, prefix + "self_attn.k_proj.weight", npf(m("self_attn.k_proj.weight")), LMQ)
        P.add(base + 3, prefix + "self_attn.v_proj.weight", npf(m("self_attn.v_proj.weight")), LMQ)
        P.add(base + 4, prefix + "self_attn.q_norm.weight", npf(m("self_attn.q_norm.weight")), DT_F32)
        P.add(base + 5, prefix + "self_attn.k_norm.weight", npf(m("self_attn.k_norm.weight")), DT_F32)
        P.add(base + 6, prefix + "self_attn.o_proj.weight", npf(m("self_attn.o_proj.weight")), LMQ)
        P.add(base + 7, prefix + "post_attention_layernorm.weight", npf(m("post_attention_layernorm.weight")), DT_F32)
        P.add(base + 8, prefix + "mlp.gate_proj.weight", npf(m("mlp.gate_proj.weight")), LMQ)
        P.add(base + 9, prefix + "mlp.up_proj.weight", npf(m("mlp.up_proj.weight")), LMQ)
        P.add(base + 10, prefix + "mlp.down_proj.weight", npf(m("mlp.down_proj.weight")), LMQ)

    for l in range(TK_LAYERS):
        tf_layer(16 * l, f"talker.model.layers.{l}.")
    for l in range(CP_LAYERS):
        tf_layer(ID_CP + 16 * l, f"talker.code_predictor.model.layers.{l}.")
    print(f"[pack] talker+cp layers done {time.time()-t0:.1f}s")

    codec_emb = gm("talker.model.codec_embedding.weight")
    P.add(ID_G + 0, "talker.model.norm.weight", npf(gm("talker.model.norm.weight")), DT_F32)
    P.add(ID_G + 1, "talker.codec_head.weight", npf(gm("talker.codec_head.weight")), LMQ)
    P.add(ID_G + 2, "talker.model.codec_embedding.weight", npf(codec_emb), LMQ)

    # --- text embedding pre-projection (exact text_projection: fc1 -> SiLU -> fc2, with biases)
    W1, b1 = gm("talker.text_projection.linear_fc1.weight"), gm("talker.text_projection.linear_fc1.bias")
    W2, b2 = gm("talker.text_projection.linear_fc2.weight"), gm("talker.text_projection.linear_fc2.bias")
    E = fm.get_tensor("talker.model.text_embedding.weight")  # bf16 [151936, 2048]

    def proj(rows_bf16):
        x = rows_bf16.float()
        return torch.nn.functional.linear(torch.nn.functional.silu(torch.nn.functional.linear(x, W1, b1)), W2, b2)

    with torch.no_grad():
        TE = torch.empty(E.shape[0], 1024, dtype=torch.float32)
        for i in range(0, E.shape[0], 8192):
            TE[i:i + 8192] = proj(E[i:i + 8192])
    TE = TE.numpy()
    print(f"[pack] text projection {TE.shape} done {time.time()-t0:.1f}s  max|x|={np.abs(TE).max():.3f}")
    P.add(ID_G + 3, "text_emb_proj", TE, DT_Q8 if args.text_emb == "q8" else DT_F16)

    P.add(ID_G + 4, "talker.code_predictor.model.norm.weight", npf(gm("talker.code_predictor.model.norm.weight")), DT_F32)
    for g in range(15):
        P.add(ID_G + 5 + g, f"talker.code_predictor.lm_head.{g}.weight",
              npf(gm(f"talker.code_predictor.lm_head.{g}.weight")), LMQ)
    for g in range(15):
        P.add(ID_G + 20 + g, f"talker.code_predictor.model.codec_embedding.{g}.weight",
              npf(gm(f"talker.code_predictor.model.codec_embedding.{g}.weight")), LMQ)

    c, s = rope_tables(float(tcfg["rope_theta"]), 128, ROPE_MAX)
    P.add(563, "rope_tk_cos", c, DT_F32)
    P.add(564, "rope_tk_sin", s, DT_F32)
    c, s = rope_tables(10000.0, 64, ROPE_MAX)
    P.add(565, "rope_dec_cos", c, DT_F32)
    P.add(566, "rope_dec_sin", s, DT_F32)

    # --- RVQ: divided + pre-projected through output_proj (Conv1d k=1, no bias)
    tabs = []
    for k in range(16):
        grp, j = ("rvq_first", 0) if k == 0 else ("rvq_rest", k - 1)
        es = gt(f"decoder.quantizer.{grp}.vq.layers.{j}._codebook.embedding_sum").double()
        cu = gt(f"decoder.quantizer.{grp}.vq.layers.{j}._codebook.cluster_usage").double()
        Wo = gt(f"decoder.quantizer.{grp}.output_proj.weight").double()[:, :, 0]  # [512,256]
        tabs.append(((es / cu.clamp(min=1e-5)[:, None]) @ Wo.T).float().numpy())
    P.add(567, "rvq_table", np.stack(tabs), DT_F32)

    # --- prompt helpers (exact F32 from unquantized weights)
    with torch.no_grad():
        ids = torch.tensor(prompt)
        tp = lambda t: proj(E[t])
        bos_e, eos_e, pad_e = tp(torch.tensor([cfg["tts_bos_token_id"], cfg["tts_eos_token_id"], cfg["tts_pad_token_id"]]))
        ce = lambda lst: codec_emb[torch.tensor(lst)]
        codec0 = ce([tcfg["codec_think_id"], tcfg["codec_think_bos_id"], lang_id, tcfg["codec_think_eos_id"]])
        codec = torch.cat([codec0, codec_emb[spk_id][None], ce([tcfg["codec_pad_id"], tcfg["codec_bos_id"]])])  # 7
        role = tp(ids[:3])
        part = torch.cat([pad_e[None].expand(codec.shape[0] - 2, -1), bos_e[None]]) + codec[:-1]  # 6
        body = torch.cat([tp(ids[3:-5]), eos_e[None]]) + codec_emb[tcfg["codec_pad_id"]][None]
        tail = pad_e[None] + codec_emb[tcfg["codec_bos_id"]][None]
        prefill = torch.cat([role, part, body, tail]).numpy()
    print(f"[pack] prefill {prefill.shape}")
    P.add(568, "prompt_ids", np.array(prompt, np.int32), DT_I32)
    P.add(569, "prefill_embeds", prefill, DT_F32)
    P.add(570, "tts_pad_embed", pad_e.numpy(), DT_F32)
    P.add(571, "tts_eos_embed", eos_e.numpy(), DT_F32)
    P.add(572, "tts_bos_embed", bos_e.numpy(), DT_F32)

    # --- Decoder
    D = ID_DEC
    P.add(D + 0, "decoder.pre_conv.conv.weight", npf(gt("decoder.pre_conv.conv.weight")), DT_F16)
    P.add(D + 1, "decoder.pre_conv.conv.bias", npf(gt("decoder.pre_conv.conv.bias")), DT_F32)
    P.add(D + 2, "decoder.pre_transformer.input_proj.weight", npf(gt("decoder.pre_transformer.input_proj.weight")), DT_F16)
    P.add(D + 3, "decoder.pre_transformer.input_proj.bias", npf(gt("decoder.pre_transformer.input_proj.bias")), DT_F32)
    for l in range(DEC_LAYERS):
        p = f"decoder.pre_transformer.layers.{l}."
        b = D + 4 + 8 * l
        ls_a = gt(p + "self_attn_layer_scale.scale")
        ls_m = gt(p + "mlp_layer_scale.scale")
        P.add(b + 0, p + "input_layernorm.weight", npf(gt(p + "input_layernorm.weight")), DT_F32)
        P.add(b + 1, p + "self_attn.q_proj.weight", npf(gt(p + "self_attn.q_proj.weight")), DT_F16)
        P.add(b + 2, p + "self_attn.k_proj.weight", npf(gt(p + "self_attn.k_proj.weight")), DT_F16)
        P.add(b + 3, p + "self_attn.v_proj.weight", npf(gt(p + "self_attn.v_proj.weight")), DT_F16)
        P.add(b + 4, p + "self_attn.o_proj.weight*ls", npf(gt(p + "self_attn.o_proj.weight") * ls_a[:, None]), DT_F16)
        P.add(b + 5, p + "post_attention_layernorm.weight", npf(gt(p + "post_attention_layernorm.weight")), DT_F32)
        P.add(b + 6, p + "mlp.gate_proj.weight", npf(gt(p + "mlp.gate_proj.weight")), DT_F16)
        P.add(b + 7, p + "mlp.up_proj.weight", npf(gt(p + "mlp.up_proj.weight")), DT_F16)
        P.add(D + 68 + l, p + "mlp.down_proj.weight*ls", npf(gt(p + "mlp.down_proj.weight") * ls_m[:, None]), DT_F16)
    P.add(D + 76, "decoder.pre_transformer.norm.weight", npf(gt("decoder.pre_transformer.norm.weight")), DT_F32)
    P.add(D + 77, "decoder.pre_transformer.output_proj.weight", npf(gt("decoder.pre_transformer.output_proj.weight")), DT_F16)
    P.add(D + 78, "decoder.pre_transformer.output_proj.bias", npf(gt("decoder.pre_transformer.output_proj.bias")), DT_F32)
    for u in range(2):
        p = f"decoder.upsample.{u}."
        b = D + 80 + 16 * u
        g = gt(p + "1.gamma")
        P.add(b + 0, p + "0.conv.weight", npf(gt(p + "0.conv.weight")), DT_F16)
        P.add(b + 1, p + "0.conv.bias", npf(gt(p + "0.conv.bias")), DT_F32)
        P.add(b + 2, p + "1.dwconv.conv.weight", npf(gt(p + "1.dwconv.conv.weight")), DT_F32)
        P.add(b + 3, p + "1.dwconv.conv.bias", npf(gt(p + "1.dwconv.conv.bias")), DT_F32)
        P.add(b + 4, p + "1.norm.weight", npf(gt(p + "1.norm.weight")), DT_F32)
        P.add(b + 5, p + "1.norm.bias", npf(gt(p + "1.norm.bias")), DT_F32)
        P.add(b + 6, p + "1.pwconv1.weight", npf(gt(p + "1.pwconv1.weight")), DT_F16)
        P.add(b + 7, p + "1.pwconv1.bias", npf(gt(p + "1.pwconv1.bias")), DT_F32)
        P.add(b + 8, p + "1.pwconv2.weight*gamma", npf(gt(p + "1.pwconv2.weight") * g[:, None]), DT_F16)
        P.add(b + 9, p + "1.pwconv2.bias*gamma", npf(gt(p + "1.pwconv2.bias") * g), DT_F32)

    def snake(tid, p):
        a, be = gt(p + "alpha").double(), gt(p + "beta").double()
        P.add(tid, p + "exp_alpha", torch.exp(a).float().numpy(), DT_F32)
        P.add(tid + 1, p + "inv_beta", (1.0 / (torch.exp(be) + 1e-9)).float().numpy(), DT_F32)

    P.add(D + 112, "decoder.decoder.0.conv.weight", npf(gt("decoder.decoder.0.conv.weight")), DT_F16)
    P.add(D + 113, "decoder.decoder.0.conv.bias", npf(gt("decoder.decoder.0.conv.bias")), DT_F32)
    for bi in range(4):
        p = f"decoder.decoder.{bi+1}.block."
        b = D + 114 + 32 * bi
        snake(b + 0, p + "0.")
        P.add(b + 2, p + "1.conv.weight", npf(gt(p + "1.conv.weight")), DT_F16)
        P.add(b + 3, p + "1.conv.bias", npf(gt(p + "1.conv.bias")), DT_F32)
        for r in range(3):
            q = p + f"{r+2}."
            s0 = b + 4 + 8 * r
            snake(s0 + 0, q + "act1.")
            P.add(s0 + 2, q + "conv1.conv.weight", npf(gt(q + "conv1.conv.weight")), DT_F16)
            P.add(s0 + 3, q + "conv1.conv.bias", npf(gt(q + "conv1.conv.bias")), DT_F32)
            snake(s0 + 4, q + "act2.")
            P.add(s0 + 6, q + "conv2.conv.weight", npf(gt(q + "conv2.conv.weight")), DT_F16)
            P.add(s0 + 7, q + "conv2.conv.bias", npf(gt(q + "conv2.conv.bias")), DT_F32)
    snake(D + 242, "decoder.decoder.5.")
    P.add(D + 244, "decoder.decoder.6.conv.weight", npf(gt("decoder.decoder.6.conv.weight")), DT_F16)
    P.add(D + 245, "decoder.decoder.6.conv.bias", npf(gt("decoder.decoder.6.conv.bias")), DT_F32)

    # --- header
    H = bytearray(HEADER_SIZE)
    H[0:8] = b"NQTS\x00\x01\r\n"
    flags = FLAG_LS_FOLDED | FLAG_RVQ_PREPROJ | FLAG_GAMMA_FOLDED | (FLAG_TEXT_EMB_Q8 if args.text_emb == "q8" else 0)
    struct.pack_into("<IIQIIQQII", H, 0x008, 1, HEADER_SIZE, DIR_OFF, 0, ALIGN, DATA_OFF, 0, flags, 0)
    struct.pack_into("<8Iff", H, 0x040, 1024, 28, 16, 8, 128, 3072, 3072, 151936, 1e-6, float(tcfg["rope_theta"]))
    struct.pack_into("<8Iff", H, 0x080, 1024, 5, 16, 8, 128, 3072, 2048, 16, 1e-6, 1e6)
    struct.pack_into("<13Ifff2I4Iff", H, 0x0C0, 512, 2048, 16, 1024, 1536, 512, 8, 16, 64, 1024, 72, 1920, 24000,
                     1e-5, 1e4, 1e-6, 2, 2, 8, 5, 4, 3, 1e-9, 1e-5)
    struct.pack_into("<16I", H, 0x140, cfg["tts_pad_token_id"], cfg["tts_bos_token_id"], cfg["tts_eos_token_id"],
                     cfg["im_start_token_id"], cfg["im_end_token_id"], cfg["assistant_token_id"],
                     tcfg["codec_pad_id"], tcfg["codec_bos_id"], tcfg["codec_eos_token_id"], tcfg["codec_think_id"],
                     tcfg["codec_nothink_id"], tcfg["codec_think_bos_id"], tcfg["codec_think_eos_id"], lang_id,
                     spk_id, tcfg["spk_id"]["ryan"])
    struct.pack_into("<4I", H, 0x1C0, ROPE_MAX, 64, ROPE_MAX, 32)
    struct.pack_into("<6IfII", H, 0x200, 1024, 512, len(prompt), prefill.shape[0], args.max_new_tokens, 2,
                     1.05, 2048, 3072)
    H[0x800:0x820] = sha256(args.model)
    H[0x820:0x840] = sha256(args.tok)
    size, crc = P.finish(H)

    # --- report
    worst = sorted(((v[3], v[0]) for v in P.stats.values() if v[1] in (DT_Q8, DT_F16)), reverse=True)[:5]
    print(f"[pack] {len(P.dir)} tensors -> {args.out}")
    print(f"[pack] file_size={size} B ({size/2**30:.4f} GiB) crc32_data=0x{crc:08x} flags=0x{flags:x}")
    print("[pack] worst rel. quant error:", ", ".join(f"{n}={e:.4%}" for e, n in worst))
    print(f"[pack] layer stride talker = {struct.unpack_from('<Q', P.dir[16], 24)[0] - struct.unpack_from('<Q', P.dir[0], 24)[0]} B")
    print(f"[pack] total {time.time()-t0:.1f}s")


if __name__ == "__main__":
    main()
