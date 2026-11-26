#!/usr/bin/env python3
"""Single-configuration benchmark worker for official Qwen3-TTS-12Hz-0.6B-CustomVoice on CPU.

Runs in a fresh Python process so cold import, model load, Talker+CP, Decoder,
and Peak RSS (VmHWM) are measured cleanly without cross-run state pollution.
"""
import json
import os
import resource
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MODEL_DIR = ROOT / "model"
TEXT_ES = "Hola Marlon, esto es código máquina puro con nopray, sin Linux y sin compilador."
SPEAKER = "serena"
LANGUAGE = "Spanish"


def rss_mib() -> float:
    with open("/proc/self/status") as f:
        for line in f:
            if line.startswith("VmHWM:"):
                return int(line.split()[1]) / 1024.0
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024.0


def main():
    threads = int(sys.argv[1])
    dtype_name = sys.argv[2]
    out_json = Path(sys.argv[3])

    t0 = time.perf_counter()
    import numpy as np
    import soundfile as sf
    import torch

    torch.set_num_threads(threads)
    torch.set_num_interop_threads(1)
    from qwen_tts import Qwen3TTSModel

    t_imp = time.perf_counter()
    dtype = torch.bfloat16 if dtype_name == "bfloat16" else torch.float32
    tts = Qwen3TTSModel.from_pretrained(str(MODEL_DIR), device_map="cpu", dtype=dtype)
    t_load = time.perf_counter()

    assistant_text = tts._build_assistant_text(TEXT_ES)
    input_ids = tts._tokenize_texts([assistant_text])

    torch.manual_seed(42)
    t_gen0 = time.perf_counter()
    with torch.no_grad():
        talker_codes_list, _ = tts.model.generate(
            input_ids=input_ids,
            instruct_ids=[None],
            languages=[LANGUAGE],
            speakers=[SPEAKER],
            non_streaming_mode=True,
            do_sample=False,
            subtalker_dosample=False,
            repetition_penalty=1.05,
            max_new_tokens=160,
        )
    t_gen1 = time.perf_counter()

    codes = talker_codes_list[0].cpu()
    t_dec0 = time.perf_counter()
    with torch.no_grad():
        dec = tts.model.speech_tokenizer.model.decoder
        wav_t = dec(codes.unsqueeze(0).transpose(1, 2))
        wav = wav_t.clamp(-1.0, 1.0).squeeze().float().cpu().numpy()
    t_dec1 = time.perf_counter()

    wav_out = ROOT / "benchmarks" / f"official_{threads}t_{dtype_name}.wav"
    wav_out.parent.mkdir(parents=True, exist_ok=True)
    sf.write(str(wav_out), wav, 24000, subtype="PCM_16")
    t_end = time.perf_counter()

    dur_s = len(wav) / 24000.0
    gen_s = t_gen1 - t_gen0
    dec_s = t_dec1 - t_dec0
    synth_s = gen_s + dec_s
    cold_wall_s = t_end - t0

    res = {
        "engine": "official_qwen_tts_pytorch",
        "threads": threads,
        "dtype": dtype_name,
        "frames": int(codes.shape[0]),
        "samples": int(len(wav)),
        "duration_s": round(dur_s, 3),
        "import_s": round(t_imp - t0, 3),
        "load_s": round(t_load - t_imp, 3),
        "cold_start_s": round(t_load - t0, 3),
        "talker_cp_s": round(gen_s, 3),
        "decoder_s": round(dec_s, 3),
        "synth_total_s": round(synth_s, 3),
        "cold_to_wav_s": round(cold_wall_s, 3),
        "rtf_synth": round(synth_s / dur_s, 3),
        "rtf_cold": round(cold_wall_s / dur_s, 3),
        "ms_per_frame_talker": round(1000.0 * gen_s / codes.shape[0], 2),
        "peak_rss_mib": round(rss_mib(), 1),
        "rms": round(float(np.sqrt(np.mean(wav ** 2))), 4),
    }
    out_json.write_text(json.dumps(res, indent=2) + "\n")
    print(json.dumps(res))


if __name__ == "__main__":
    main()
