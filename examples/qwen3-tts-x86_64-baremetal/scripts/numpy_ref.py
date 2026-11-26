#!/usr/bin/env python3
"""Reference executor for weights.nqts — NumPy + struct + wave only (no torch, no safetensors).

Mirrors the algorithm the bare-metal x86-64 AVX2 VM implements (docs/WEIGHTS_SPEC.md §8):
  W8A32 Q8_0S GEMV, RMSNorm, SwiGLU, GQA + QK-Norm + rotate_half RoPE (table), Talker 28L,
  CodePredictor 5L (greedy), repetition_penalty on codebook 0, Speech-Tokenizer 12Hz V2 decoder.

Usage: .venv/bin/python -I scripts/numpy_ref.py [--weights artifacts/weights.nqts]
         [--golden artifacts/golden_ref_greedy.npz] [--prefill table|build] [--teacher-force]
         [--out artifacts/numpy_ref_es.wav]
"""
import argparse
import json
import math
import struct
import sys
import time
import wave
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent.parent
F32 = np.float32
DT_F32, DT_F16, DT_Q8, DT_I32 = 0, 1, 2, 3
ID_CP, ID_G, ID_DEC = 448, 528, 576


# ----------------------------------------------------------------------------- loader
class NQTS:
    def __init__(self, path):
        self.m = np.memmap(path, dtype=np.uint8, mode="r")
        h = self.m[:4096].tobytes()
        assert h[:8] == b"NQTS\x00\x01\r\n", "bad magic"
        (self.version, hs, self.dir_off, self.dir_count, align, self.data_off, self.file_size,
         self.flags, self.crc) = struct.unpack_from("<IIQIIQQII", h, 0x008)
        assert self.version == 1 and hs == 4096 and align == 64 and self.file_size == len(self.m)
        self.tk = struct.unpack_from("<8Iff", h, 0x040)
        self.cp = struct.unpack_from("<8Iff", h, 0x080)
        self.dec = struct.unpack_from("<13Ifff2I4Iff", h, 0x0C0)
        self.tok = struct.unpack_from("<16I", h, 0x140)
        self.rope = struct.unpack_from("<4I", h, 0x1C0)
        self.rt = struct.unpack_from("<6IfII", h, 0x200)
        self.ent = {}
        for i in range(1024):
            e = struct.unpack_from("<IHH4IQQQIIQ", self.m, self.dir_off + 64 * i)
            if e[1] == 0 and e[7] == 0:
                continue
            tid, dt, nd, s0, s1, s2, s3, off, off2, nb = e[:10]
            assert tid == i and off % 64 == 0 and off2 % 64 == 0
            self.ent[i] = (dt, (s0, s1, s2, s3)[:nd], off, off2, nb)
        self._deq = {}

    def raw(self, tid):
        dt, shp, off, off2, nb = self.ent[tid]
        n = int(np.prod(shp))
        if dt == DT_Q8:
            R, K = shp
            qs = self.m[off:off + R * K].view(np.int8).reshape(R, K)
            sc = self.m[off2:off2 + R * K // 8].view(F32).reshape(R, K // 32)
            return qs, sc
        npdt = {DT_F32: F32, DT_F16: np.float16, DT_I32: np.int32}[dt]
        return self.m[off:off + nb].view(npdt).reshape(shp)

    def f32(self, tid):
        """Any tensor as float32 (Q8 dequantized: w = d*q exactly, cached)."""
        if tid in self._deq:
            return self._deq[tid]
        dt = self.ent[tid][0]
        if dt == DT_Q8:
            qs, sc = self.raw(tid)
            R, K = qs.shape
            w = (qs.reshape(R, K // 32, 32).astype(F32) * sc[:, :, None]).reshape(R, K)
            self._deq[tid] = w
            return w
        a = np.asarray(self.raw(tid)).astype(F32)
        if dt != DT_F32:
            self._deq[tid] = a
        return a

    def row(self, tid, r):
        """Embedding gather: one dequantized row."""
        dt = self.ent[tid][0]
        if dt == DT_Q8:
            qs, sc = self.raw(tid)
            return (qs[r].reshape(-1, 32).astype(F32) * sc[r][:, None]).reshape(-1)
        return np.asarray(self.raw(tid)[r]).astype(F32)


# ----------------------------------------------------------------------------- primitives
def rmsnorm(x, w, eps):
    x = x.astype(F32)
    return (x / np.sqrt((x * x).mean(-1, keepdims=True) + F32(eps))) * w


def silu(x):
    return x / (F32(1) + np.exp(-x))


def rope(x, cos, sin):
    """rotate_half convention; x [T,H,D], cos/sin [T,D/2]."""
    h = x.shape[-1] // 2
    x1, x2 = x[..., :h], x[..., h:]
    c, s = cos[:, None, :], sin[:, None, :]
    return np.concatenate([x1 * c - x2 * s, x2 * c + x1 * s], axis=-1)


def softmax(s):
    s = s - s.max(-1, keepdims=True)
    e = np.exp(s)
    return e / e.sum(-1, keepdims=True)


def erf_as(x):
    """Abramowitz–Stegun 7.1.26 (|err| < 1.5e-7) — same polynomial the VM uses."""
    s = np.sign(x)
    a = np.abs(x)
    t = 1.0 / (1.0 + 0.3275911 * a)
    y = 1.0 - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * np.exp(-a * a)
    return (s * y).astype(F32)


def gelu(x):
    return (F32(0.5) * x * (F32(1) + erf_as(x * F32(1 / math.sqrt(2))))).astype(F32)


def snake(x, ea, ib):
    """x [C,T]; y = x + inv_beta * sin(exp_alpha*x)^2"""
    s = np.sin(x * ea[:, None])
    return x + ib[:, None] * s * s


def conv1d_causal(x, w, b, dil=1):
    """x [Cin,T], w [Cout,Cin,K] -> [Cout,T]; left pad (K-1)*dil (stride 1)."""
    co, ci, k = w.shape
    T = x.shape[1]
    pad = (k - 1) * dil
    xp = np.concatenate([np.zeros((ci, pad), F32), x], axis=1)
    y = np.zeros((co, T), F32)
    for kk in range(k):
        y += w[:, :, kk] @ xp[:, kk * dil: kk * dil + T]
    return y + b[:, None]


def dwconv_causal(x, w, b):
    C, _, k = w.shape
    T = x.shape[1]
    xp = np.concatenate([np.zeros((C, k - 1), F32), x], axis=1)
    y = np.zeros_like(x)
    for kk in range(k):
        y += w[:, 0, kk][:, None] * xp[:, kk:kk + T]
    return y + b[:, None]


def transconv_causal(x, w, b, s):
    """x [Cin,T], w [Cin,Cout,K] (PyTorch ConvTranspose1d), stride s, trim right K-s -> [Cout, T*s]."""
    ci, co, k = w.shape
    T = x.shape[1]
    L = (T - 1) * s + k
    y = np.zeros((co, L), F32)
    for kk in range(k):
        y[:, kk: kk + (T - 1) * s + 1: s] += w[:, :, kk].T @ x
    y = y[:, : L - (k - s)]
    return y + b[:, None]


# ----------------------------------------------------------------------------- transformer (Talker / CP)
class LM:
    def __init__(self, W, base, n_layers, max_pos, cos, sin):
        self.W, self.base, self.L = W, base, n_layers
        self.cos, self.sin = cos, sin
        self.K = np.zeros((n_layers, max_pos, 8, 128), F32)
        self.V = np.zeros((n_layers, max_pos, 8, 128), F32)
        self.ln = [[W.f32(base + 16 * l + s) for s in (0, 4, 5, 7)] for l in range(n_layers)]
        self.mat = [[W.f32(base + 16 * l + s) for s in (1, 2, 3, 6, 8, 9, 10)] for l in range(n_layers)]

    def forward(self, x, pos0):
        """x [T,1024] at positions pos0..pos0+T-1 -> pre-final-norm hidden [T,1024]."""
        T = x.shape[0]
        c, s = self.cos[pos0:pos0 + T], self.sin[pos0:pos0 + T]
        n = pos0 + T
        mask = np.where(np.arange(n)[None, :] <= (pos0 + np.arange(T))[:, None], 0, -np.inf).astype(F32)
        for l in range(self.L):
            w_in, w_qn, w_kn, w_post = self.ln[l]
            Wq, Wk, Wv, Wo, Wg, Wu, Wd = self.mat[l]
            h = rmsnorm(x, w_in, 1e-6)
            q = rmsnorm((h @ Wq.T).reshape(T, 16, 128), w_qn, 1e-6)
            k = rmsnorm((h @ Wk.T).reshape(T, 8, 128), w_kn, 1e-6)
            v = (h @ Wv.T).reshape(T, 8, 128)
            q, k = rope(q, c, s), rope(k, c, s)
            self.K[l, pos0:n], self.V[l, pos0:n] = k, v
            Kc, Vc = self.K[l, :n], self.V[l, :n]
            o = np.empty((T, 16, 128), F32)
            for hh in range(16):
                kv = hh // 2  # GQA: 16 Q heads / 8 KV heads
                sc = (q[:, hh] @ Kc[:, kv].T) * F32(128 ** -0.5) + mask
                o[:, hh] = softmax(sc) @ Vc[:, kv]
            x = x + o.reshape(T, 2048) @ Wo.T
            h = rmsnorm(x, w_post, 1e-6)
            x = x + (silu(h @ Wg.T) * (h @ Wu.T)) @ Wd.T
        return x


def build_prefill(W, tok):
    """General path from prompt_ids (Q8/F16 tables) — identical layout to the packer's table."""
    tts_pad, tts_bos, tts_eos, _, _, _, c_pad, c_bos, _, c_think, _, c_tbos, c_teos, lang, spk, _ = tok
    ids = W.raw(568)
    te = lambda t: W.row(ID_G + 3, int(t))
    ce = lambda t: W.row(ID_G + 2, int(t))
    pad_e, bos_e, eos_e = W.f32(570), W.f32(572), W.f32(571)
    rows = [te(t) for t in ids[:3]]
    codec = [ce(c_think), ce(c_tbos), ce(lang), ce(c_teos), ce(spk), ce(c_pad), ce(c_bos)]
    rows += [pad_e + codec[i] for i in range(5)] + [bos_e + codec[5]]
    rows += [te(t) + ce(c_pad) for t in ids[3:-5]] + [eos_e + ce(c_pad)]
    rows += [pad_e + ce(c_bos)]
    return np.stack(rows).astype(F32)


def run_talker(W, prefill, golden_codes=None, max_new=160):
    tok = W.tok
    c_eos = tok[8]
    rp = W.rt[6]
    sup_lo, sup_hi = W.rt[7], W.rt[8]
    cos, sin = W.f32(563), W.f32(564)
    tk = LM(W, 0, 28, W.rt[0], cos, sin)
    cp = LM(W, ID_CP, 5, 32, cos, sin)
    tk_norm, cp_norm = W.f32(ID_G + 0), W.f32(ID_G + 4)
    head = W.f32(ID_G + 1)
    lm_heads = [W.f32(ID_G + 5 + g) for g in range(15)]
    pad_e = W.f32(570)

    h = tk.forward(prefill, 0)[-1:]
    pos = prefill.shape[0]
    codes, hiddens, prev0 = [], [], set()
    tf_agree = []
    for step in range(max_new):
        hn = rmsnorm(h, tk_norm, 1e-6)[0]
        logits = head @ hn
        logits[sup_lo:sup_hi] = np.where(np.arange(sup_lo, sup_hi) == c_eos, logits[sup_lo:sup_hi], -np.inf)
        if step < W.rt[5]:
            logits[c_eos] = -np.inf
        for t in prev0:
            logits[t] = logits[t] / rp if logits[t] > 0 else logits[t] * rp
        c0 = int(np.argmax(logits))
        if golden_codes is not None:
            if step >= len(golden_codes):
                break
            tf = [c0 == golden_codes[step, 0]]
            c0 = int(golden_codes[step, 0])
        if c0 == c_eos:
            break
        prev0.add(c0)
        hiddens.append(hn)
        # Code Predictor: fresh cache, positions 0..15
        e0 = W.row(ID_G + 2, c0)
        x = cp.forward(np.stack([hn, e0]), 0)[-1:]
        fr = [c0]
        emb_sum = e0.copy()
        for g in range(1, 16):
            cg = int(np.argmax(lm_heads[g - 1] @ rmsnorm(x, cp_norm, 1e-6)[0]))
            if golden_codes is not None:
                tf.append(cg == golden_codes[step, g])
                cg = int(golden_codes[step, g])
            fr.append(cg)
            eg = W.row(ID_G + 20 + g - 1, cg)
            emb_sum += eg
            if g < 15:
                x = cp.forward(eg[None], g + 1)
        codes.append(fr)
        if golden_codes is not None:
            tf_agree.append(tf)
        h = tk.forward((emb_sum + pad_e)[None], pos)
        pos += 1
    return np.array(codes, np.int32), np.array(hiddens, F32), np.array(tf_agree, bool)


# ----------------------------------------------------------------------------- decoder
def run_decoder(W, codes, taps):
    D = ID_DEC
    T = codes.shape[0]
    tab = W.raw(567)
    x = np.zeros((T, 512), F32)
    for k in range(16):
        x += tab[k][codes[:, k]]
    taps["rvq_out"] = x.T.copy()
    x = conv1d_causal(x.T, W.f32(D + 0), W.f32(D + 1))  # [1024,T]
    taps["pre_conv_out"] = x.T.copy()
    # pre_transformer
    h = x.T @ W.f32(D + 2).T + W.f32(D + 3)  # [T,512]
    cos, sin = W.f32(565)[:T], W.f32(566)[:T]
    i = np.arange(T)
    win = W.dec[10]
    mask = np.where((i[None, :] <= i[:, None]) & (i[None, :] > i[:, None] - win), 0, -np.inf).astype(F32)
    for l in range(8):
        b = D + 4 + 8 * l
        a = rmsnorm(h, W.f32(b + 0), 1e-5)
        q = rope((a @ W.f32(b + 1).T).reshape(T, 16, 64), cos, sin)
        k = rope((a @ W.f32(b + 2).T).reshape(T, 16, 64), cos, sin)
        v = (a @ W.f32(b + 3).T).reshape(T, 16, 64)
        o = np.empty((T, 16, 64), F32)
        for hh in range(16):
            o[:, hh] = softmax(q[:, hh] @ k[:, hh].T * F32(64 ** -0.5) + mask) @ v[:, hh]
        h = h + o.reshape(T, 1024) @ W.f32(b + 4).T
        a = rmsnorm(h, W.f32(b + 5), 1e-5)
        h = h + (silu(a @ W.f32(b + 6).T) * (a @ W.f32(b + 7).T)) @ W.f32(D + 68 + l).T
    h = rmsnorm(h, W.f32(D + 76), 1e-5) @ W.f32(D + 77).T + W.f32(D + 78)
    taps["pre_xfmr_out"] = h.copy()
    x = h.T  # [1024,T]
    for u in range(2):
        b = D + 80 + 16 * u
        x = transconv_causal(x, W.f32(b + 0), W.f32(b + 1), 2)
        r = x
        y = dwconv_causal(x, W.f32(b + 2), W.f32(b + 3)).T  # [T,C]
        mu = y.mean(-1, keepdims=True)
        var = ((y - mu) ** 2).mean(-1, keepdims=True)
        y = (y - mu) / np.sqrt(var + F32(1e-6)) * W.f32(b + 4) + W.f32(b + 5)
        y = gelu(y @ W.f32(b + 6).T + W.f32(b + 7))
        y = y @ W.f32(b + 8).T + W.f32(b + 9)
        x = r + y.T
    taps["up_out"] = x.copy()
    x = conv1d_causal(x, W.f32(D + 112), W.f32(D + 113))
    for bi, rate in enumerate(W.dec[18:22]):
        b = D + 114 + 32 * bi
        x = snake(x, W.f32(b + 0), W.f32(b + 1))
        x = transconv_causal(x, W.f32(b + 2), W.f32(b + 3), rate)
        for r, dil in enumerate((1, 3, 9)):
            s0 = b + 4 + 8 * r
            y = snake(x, W.f32(s0 + 0), W.f32(s0 + 1))
            y = conv1d_causal(y, W.f32(s0 + 2), W.f32(s0 + 3), dil)
            y = snake(y, W.f32(s0 + 4), W.f32(s0 + 5))
            y = conv1d_causal(y, W.f32(s0 + 6), W.f32(s0 + 7))
            x = x + y
    x = snake(x, W.f32(D + 242), W.f32(D + 243))
    x = conv1d_causal(x, W.f32(D + 244), W.f32(D + 245))
    wav = np.clip(x[0], -1, 1)
    taps["wav"] = wav
    return wav


def cos_sim(a, b):
    a, b = np.asarray(a, np.float64).ravel(), np.asarray(b, np.float64).ravel()
    return float(a @ b / (np.linalg.norm(a) * np.linalg.norm(b) + 1e-30))


def snr_db(ref, x):
    ref, x = np.asarray(ref, np.float64), np.asarray(x, np.float64)
    n = min(len(ref), len(x))
    return float(10 * np.log10((ref[:n] ** 2).sum() / max(((ref[:n] - x[:n]) ** 2).sum(), 1e-30)))


def write_wav(path, wav, sr=24000):
    pcm = np.clip(np.rint(wav * 32767.0), -32768, 32767).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(sr)
        w.writeframes(pcm.tobytes())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--weights", default=str(ROOT / "artifacts/weights.nqts"))
    ap.add_argument("--golden", default=str(ROOT / "artifacts/golden_ref_greedy.npz"))
    ap.add_argument("--prefill", choices=["table", "build"], default="table")
    ap.add_argument("--teacher-force", action="store_true")
    ap.add_argument("--out", default=str(ROOT / "artifacts/numpy_ref_es.wav"))
    ap.add_argument("--report", default=str(ROOT / "artifacts/numpy_ref_report.json"))
    args = ap.parse_args()
    assert "torch" not in sys.modules
    t0 = time.time()
    W = NQTS(args.weights)
    G = np.load(args.golden)
    gcodes = G["codes"]
    rep = {"weights": args.weights, "file_size": W.file_size, "flags": W.flags, "prefill_mode": args.prefill}

    pf = W.f32(569) if args.prefill == "table" else build_prefill(W, W.tok)
    if args.prefill == "build":
        rep["prefill_build_vs_table_cos"] = cos_sim(pf, W.f32(569))
    t1 = time.time()
    codes, hid, tf = run_talker(W, pf, gcodes if args.teacher_force else None, max_new=W.rt[4])
    t2 = time.time()
    n = min(len(codes), len(gcodes))
    frame_eq = (codes[:n] == gcodes[:n]).all(1)
    first_bad = int(np.argmin(frame_eq)) if not frame_eq.all() else None
    rep.update({
        "talker_cp_s": round(t2 - t1, 2), "frames": int(len(codes)), "golden_frames": int(len(gcodes)),
        "frames_exact": int(frame_eq.sum()), "first_mismatch_frame": first_bad,
        "codes_token_agreement": float((codes[:n] == gcodes[:n]).mean()),
        "code0_agreement": float((codes[:n, 0] == gcodes[:n, 0]).mean()),
        "talker_hidden_cos_min": float(min(cos_sim(hid[i], G["talker_last_hidden"][i]) for i in range(n))),
        "talker_hidden_cos_mean": float(np.mean([cos_sim(hid[i], G["talker_last_hidden"][i]) for i in range(n)])),
    })
    if args.teacher_force:
        rep["tf_agreement_per_group"] = [round(float(x), 4) for x in tf.mean(0)]
        rep["tf_agreement_all"] = float(tf.mean())
    print(json.dumps({k: v for k, v in rep.items()}, indent=1))

    # decoder isolated on golden codes (layerwise), then on own codes (end-to-end)
    taps = {}
    t3 = time.time()
    wav_g = run_decoder(W, gcodes, taps)
    t4 = time.time()
    rep["decoder_s"] = round(t4 - t3, 2)
    rep["decoder_on_golden_codes"] = {
        "rvq_cos": cos_sim(taps["rvq_out"], G["rvq_out"]),
        "pre_conv_cos": cos_sim(taps["pre_conv_out"], G["pre_conv_out"]),
        "pre_xfmr_cos": cos_sim(taps["pre_xfmr_out"], G["pre_xfmr_out"]),
        "upsample_cos": cos_sim(taps["up_out"], G["up_out"]),
        "wav_cos": cos_sim(wav_g, G["wav"]),
        "wav_snr_db": snr_db(G["wav"], wav_g),
    }
    if np.array_equal(codes, gcodes):
        wav = wav_g
    else:
        wav = run_decoder(W, codes, {})
    rep["e2e"] = {"samples": int(len(wav)), "duration_s": len(wav) / 24000,
                  "wav_cos": cos_sim(G["wav"][:len(wav)], wav[:len(G["wav"])]),
                  "wav_snr_db": snr_db(G["wav"], wav),
                  "rms": float(np.sqrt((wav ** 2).mean())), "peak": float(np.abs(wav).max())}
    if not args.teacher_force:
        write_wav(args.out, wav)
        rep["out"] = args.out
        np.save(str(Path(args.out).with_suffix(".codes.npy")), codes)
    rep["total_s"] = round(time.time() - t0, 2)
    print(json.dumps({k: rep[k] for k in ("decoder_on_golden_codes", "e2e", "decoder_s", "total_s")}, indent=1))
    Path(args.report).write_text(json.dumps(rep, indent=1) + "\n")


if __name__ == "__main__":
    main()
