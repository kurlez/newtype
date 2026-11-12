#!/usr/bin/env python3
"""Benchmark Qwen3-ASR on macOS arm64. Not part of the app build.

1.7B uses the pinned llama.cpp Metal binary and the official ggml-org Q8_0
GGUF pair. 0.6B int8 uses the official sherpa-onnx package. Weights are
downloaded into spikes/qwen3-asr-mac/.cache and are not committed.

Run on Apple Silicon:

    python3 spikes/qwen3-asr-mac/bench.py

Check the committed clips on any machine:

    python3 spikes/qwen3-asr-mac/bench.py --self-test
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import signal
import subprocess
import sys
import tarfile
import time
import urllib.request
import wave
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SAMPLES = ROOT / "samples"
CACHE = ROOT / ".cache"
OUT = ROOT / "out"

LLAMA_TAG = "b12292"
LLAMA_URL = (
    "https://github.com/ggml-org/llama.cpp/releases/download/"
    f"{LLAMA_TAG}/llama-{LLAMA_TAG}-bin-macos-arm64.tar.gz"
)
LLAMA_ARCHIVE_BYTES = 16_870_580
DECODER_URL = (
    "https://huggingface.co/ggml-org/Qwen3-ASR-1.7B-GGUF/resolve/main/"
    "Qwen3-ASR-1.7B-Q8_0.gguf"
)
DECODER_BYTES = 2_165_034_944
MMPROJ_URL = (
    "https://huggingface.co/ggml-org/Qwen3-ASR-1.7B-GGUF/resolve/main/"
    "mmproj-Qwen3-ASR-1.7B-Q8_0.gguf"
)
MMPROJ_BYTES = 355_709_344

SHERPA_TAG = "v2.0.1"
SHERPA_URL = (
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/"
    f"{SHERPA_TAG}/sherpa-onnx-{SHERPA_TAG}-osx-arm64-shared.tar.bz2"
)
SHERPA_ARCHIVE_BYTES = 20_348_241
SHERPA_MODEL_URL = (
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/"
    "sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2"
)
SHERPA_MODEL_BYTES = 878_702_423

USER_AGENT = "newtype-qwen3-asr-spike"


def log(message: str) -> None:
    print(message, flush=True)


def remove_path(path: Path) -> None:
    if path.is_dir():
        shutil.rmtree(path, ignore_errors=True)
    elif path.exists():
        path.unlink()


def require_arm64_macos() -> None:
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise SystemExit(
            "This benchmark runs on macOS arm64. "
            "Use --self-test anywhere to check the sample clips."
        )


def download(url: str, dest: Path, expected_size: int) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists() and dest.stat().st_size == expected_size:
        log(f"cached {dest.name} ({expected_size} bytes)")
        return
    partial = dest.with_suffix(dest.suffix + ".part")
    if partial.exists():
        partial.unlink()
    log(f"downloading {dest.name}")
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=120) as response, partial.open("wb") as handle:
        copied = 0
        next_report = 256 * 1024 * 1024
        while True:
            block = response.read(1024 * 1024)
            if not block:
                break
            handle.write(block)
            copied += len(block)
            if copied >= next_report:
                log(f"  {dest.name}: {copied / 1e9:.2f} GB")
                next_report += 256 * 1024 * 1024
    size = partial.stat().st_size
    if size != expected_size:
        partial.unlink()
        raise SystemExit(f"{url} downloaded {size} bytes, expected {expected_size}")
    partial.rename(dest)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def extract(archive: Path, dest: Path) -> Path:
    marker = dest / ".extracted"
    if marker.exists():
        return dest
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    log(f"extracting {archive.name}")
    with tarfile.open(archive) as tar:
        try:
            tar.extractall(dest, filter="data")
        except TypeError:
            tar.extractall(dest)
    marker.write_text(archive.name + "\n")
    return dest


def find_named(root: Path, name: str) -> Path:
    matches = [path for path in root.rglob(name) if path.is_file()]
    if not matches:
        raise SystemExit(f"could not find {name} under {root}")
    return matches[0]


def hardware() -> dict:
    def sysctl(key: str) -> str:
        try:
            return subprocess.check_output(["sysctl", "-n", key], text=True).strip()
        except (subprocess.CalledProcessError, FileNotFoundError):
            return ""

    mem = sysctl("hw.memsize")
    return {
        "system": platform.platform(),
        "machine": platform.machine(),
        "cpu": sysctl("machdep.cpu.brand_string"),
        "logical_cpus": os.cpu_count(),
        "physical_cpus": sysctl("hw.physicalcpu"),
        "memsize_bytes": int(mem) if mem.isdigit() else None,
        "swap": swap_usage(),
    }


def swap_usage() -> str:
    try:
        return subprocess.check_output(["sysctl", "-n", "vm.swapusage"], text=True).strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return ""


def rss_kb(pid: int) -> int | None:
    try:
        text = subprocess.check_output(["ps", "-o", "rss=", "-p", str(pid)], text=True).strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None
    if not text:
        return None
    try:
        return int(text.split()[0])
    except ValueError:
        return None


def split_asr_text(raw: str) -> dict:
    text = raw.strip()
    language = None
    marker = "<asr_text>"
    if marker in text:
        head, text = text.split(marker, 1)
        if head.lower().startswith("language "):
            language = head[len("language ") :].strip()
    return {"language": language, "text": text.strip(), "raw": raw}


def wav_duration(path: Path) -> float:
    with wave.open(str(path)) as handle:
        if handle.getnchannels() != 1 or handle.getsampwidth() != 2:
            raise SystemExit(f"{path} is not mono PCM16")
        return handle.getnframes() / handle.getframerate()


def load_manifest() -> list[dict]:
    manifest = json.loads((SAMPLES / "manifest.json").read_text())
    for item in manifest:
        item["path"] = str(SAMPLES / item["file"])
        item["duration_s"] = wav_duration(Path(item["path"]))
    return manifest


def transcribe_http(port: int, wav: Path) -> tuple[float, str]:
    started = time.perf_counter()
    result = subprocess.run(
        [
            "curl",
            "-sS",
            "-m",
            "180",
            "-X",
            "POST",
            f"http://127.0.0.1:{port}/v1/audio/transcriptions",
            "-F",
            f"file=@{wav}",
            "-F",
            "response_format=json",
            "-F",
            "temperature=0",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    elapsed = time.perf_counter() - started
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or f"curl exited {result.returncode}")
    body = result.stdout.strip()
    try:
        payload = json.loads(body)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"non-JSON transcription response: {body[:500]}") from exc
    if "error" in payload:
        raise RuntimeError(json.dumps(payload["error"])[:500])
    text = payload.get("text")
    if not isinstance(text, str):
        raise RuntimeError(f"transcription response has no text: {body[:500]}")
    return elapsed, text


def interesting_log_lines(log_text: str) -> list[str]:
    keys = (
        "metal",
        "mtl",
        "gpu",
        "offload",
        "flash",
        "mmproj",
        "unsupported",
        "n_slots",
        "error",
        "oom",
        "failed",
        "audio",
        "experimental",
    )
    lines = []
    for line in log_text.splitlines():
        lowered = line.lower()
        if "type = " in lowered and "ne =" in lowered:
            continue
        if any(key in lowered for key in keys):
            lines.append(line[:400])
    return lines[:40]


def benchmark_llama(clips: list[dict]) -> dict:
    report: dict = {
        "runtime": "llama.cpp",
        "tag": LLAMA_TAG,
        "model": "ggml-org/Qwen3-ASR-1.7B-GGUF",
        "files": {
            "decoder": "Qwen3-ASR-1.7B-Q8_0.gguf",
            "mmproj": "mmproj-Qwen3-ASR-1.7B-Q8_0.gguf",
        },
        "ctx_size": 2048,
        "cache_ram_mib": 0,
        "gpu_layers": 99,
        "parallel_slots": 1,
    }
    try:
        archive = CACHE / f"llama-{LLAMA_TAG}-macos-arm64.tar.gz"
        decoder = CACHE / "Qwen3-ASR-1.7B-Q8_0.gguf"
        mmproj = CACHE / "mmproj-Qwen3-ASR-1.7B-Q8_0.gguf"
        download(LLAMA_URL, archive, LLAMA_ARCHIVE_BYTES)
        download(DECODER_URL, decoder, DECODER_BYTES)
        download(MMPROJ_URL, mmproj, MMPROJ_BYTES)
        report["sha256"] = {
            "decoder": sha256(decoder),
            "mmproj": sha256(mmproj),
        }
        tree = extract(archive, CACHE / f"llama-{LLAMA_TAG}")
        server = find_named(tree, "llama-server")
        server.chmod(server.stat().st_mode | 0o755)
        port = 8099
        log_path = OUT / "llama-server.log"
        command = [
            str(server),
            "-m",
            str(decoder),
            "--mmproj",
            str(mmproj),
            "-c",
            "2048",
            "-ngl",
            "99",
            "--parallel",
            "1",
            "--cache-ram",
            "0",
            "--no-warmup",
            "--reasoning",
            "off",
            "--alias",
            "qwen3-asr",
            "--host",
            "127.0.0.1",
            "--port",
            str(port),
        ]
        log("starting llama-server")
        log_handle = log_path.open("w")
        started = time.perf_counter()
        env = os.environ.copy()
        env["DYLD_LIBRARY_PATH"] = str(server.parent) + os.pathsep + env.get("DYLD_LIBRARY_PATH", "")
        process = subprocess.Popen(
            command,
            stdout=log_handle,
            stderr=subprocess.STDOUT,
            start_new_session=True,
            cwd=server.parent,
            env=env,
        )
        peaks: list[int] = []
        ready = None
        try:
            deadline = time.monotonic() + 240
            while time.monotonic() < deadline:
                rss = rss_kb(process.pid)
                if rss is not None:
                    peaks.append(rss)
                if process.poll() is not None:
                    raise RuntimeError(f"llama-server exited {process.returncode} during load")
                probe = subprocess.run(
                    ["curl", "-sf", "-m", "2", f"http://127.0.0.1:{port}/health"],
                    capture_output=True,
                    text=True,
                )
                if probe.returncode == 0:
                    ready = time.perf_counter() - started
                    break
                time.sleep(0.5)
            if ready is None:
                raise RuntimeError("llama-server did not become healthy within 240s")
            report["load_ready_s"] = round(ready, 3)
            report["swap_after_load"] = swap_usage()
            runs = []
            # First call pays for Metal shader compile. Keep it out of the warm set.
            warmup_s, warmup_text = transcribe_http(port, Path(clips[0]["path"]))
            rss = rss_kb(process.pid)
            if rss is not None:
                peaks.append(rss)
            runs.append(
                {
                    "file": clips[0]["file"],
                    "phase": "warmup",
                    "seconds": round(warmup_s, 3),
                    **split_asr_text(warmup_text),
                }
            )
            for clip in clips:
                for index in (1, 2):
                    seconds, text = transcribe_http(port, Path(clip["path"]))
                    rss = rss_kb(process.pid)
                    if rss is not None:
                        peaks.append(rss)
                    parsed = split_asr_text(text)
                    runs.append(
                        {
                            "file": clip["file"],
                            "phase": f"warm-{index}",
                            "audio_s": round(clip["duration_s"], 3),
                            "seconds": round(seconds, 3),
                            "rtf": round(seconds / clip["duration_s"], 4),
                            **parsed,
                        }
                    )
                    log(f"llama {clip['file']} warm-{index}: {seconds:.3f}s {parsed['text'][:80]}")
            report["runs"] = runs
            report["peak_rss_mb"] = round(max(peaks) / 1024, 1) if peaks else None
            report["swap_after"] = swap_usage()
        finally:
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    process.wait(timeout=10)
            log_handle.close()
        log_text = log_path.read_text(errors="replace") if log_path.exists() else ""
        report["log_lines"] = interesting_log_lines(log_text)
    except Exception as exc:  # noqa: BLE001 - the report is the product of this spike
        report["error"] = str(exc)
        report["swap_after_error"] = swap_usage()
        log(f"llama.cpp benchmark failed: {exc}")
    finally:
        # The Actions runner has a 14 GB disk. Drop the 2.5 GB weights before sherpa.
        remove_path(CACHE / "Qwen3-ASR-1.7B-Q8_0.gguf")
        remove_path(CACHE / "mmproj-Qwen3-ASR-1.7B-Q8_0.gguf")
        remove_path(CACHE / f"llama-{LLAMA_TAG}")
    return report


def sherpa_python_script(model_dir: Path, clips: list[dict], destination: Path) -> None:
    payload = {
        "model_dir": str(model_dir),
        "clips": [
            {"file": clip["file"], "path": clip["path"], "duration_s": clip["duration_s"]}
            for clip in clips
        ],
    }
    destination.write_text(
        "import json, resource, sys, time, wave\n"
        "import sherpa_onnx\n"
        f"spec = {json.dumps(payload)}\n"
        "model = spec['model_dir']\n"
        "started = time.perf_counter()\n"
        "recognizer = sherpa_onnx.OfflineRecognizer.from_qwen3_asr(\n"
        "    conv_frontend=model + '/conv_frontend.onnx',\n"
        "    encoder=model + '/encoder.int8.onnx',\n"
        "    decoder=model + '/decoder.int8.onnx',\n"
        "    tokenizer=model + '/tokenizer',\n"
        "    num_threads=3,\n"
        ")\n"
        "load_s = time.perf_counter() - started\n"
        "def samples(path):\n"
        "    with wave.open(path) as handle:\n"
        "        raw = handle.readframes(handle.getnframes())\n"
        "    return [int.from_bytes(raw[i:i+2], 'little', signed=True) / 32768.0 for i in range(0, len(raw), 2)]\n"
        "def once(path):\n"
        "    audio = samples(path)\n"
        "    started = time.perf_counter()\n"
        "    stream = recognizer.create_stream()\n"
        "    stream.accept_waveform(16000, audio)\n"
        "    recognizer.decode_stream(stream)\n"
        "    return time.perf_counter() - started, stream.result.text\n"
        "runs = []\n"
        "warm_s, warm_text = once(spec['clips'][0]['path'])\n"
        "runs.append({'file': spec['clips'][0]['file'], 'phase': 'warmup', 'seconds': round(warm_s, 3), 'text': warm_text})\n"
        "for clip in spec['clips']:\n"
        "    for index in (1, 2):\n"
        "        seconds, text = once(clip['path'])\n"
        "        runs.append({\n"
        "            'file': clip['file'],\n"
        "            'phase': f'warm-{index}',\n"
        "            'audio_s': round(clip['duration_s'], 3),\n"
        "            'seconds': round(seconds, 3),\n"
        "            'rtf': round(seconds / clip['duration_s'], 4),\n"
        "            'text': text,\n"
        "        })\n"
        "        print(f\"sherpa {clip['file']} warm-{index}: {seconds:.3f}s {text[:80]}\", flush=True)\n"
        "maxrss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss\n"
        "json.dump({'load_s': round(load_s, 3), 'ru_maxrss': maxrss, 'runs': runs}, sys.stdout)\n"
    )


def benchmark_sherpa(clips: list[dict]) -> dict:
    report: dict = {
        "runtime": "sherpa-onnx",
        "tag": SHERPA_TAG,
        "model": "sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25",
        "model_url": SHERPA_MODEL_URL,
    }
    try:
        archive = CACHE / "sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2"
        download(SHERPA_MODEL_URL, archive, SHERPA_MODEL_BYTES)
        report["model_sha256"] = sha256(archive)
        tree = extract(archive, CACHE / "sherpa-model")
        remove_path(archive)
        model_dir = find_named(tree, "conv_frontend.onnx").parent
        report["model_dir_name"] = model_dir.name
        script = OUT / "sherpa_once.py"
        sherpa_python_script(model_dir, clips, script)
        env = os.environ.copy()
        log("installing sherpa-onnx python wheel for a resident-model timing")
        pip = subprocess.run(
            [
                sys.executable,
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                "--user",
                "--break-system-packages",
                f"sherpa-onnx=={SHERPA_TAG.lstrip('v')}",
            ],
            capture_output=True,
            text=True,
        )
        report["pip_returncode"] = pip.returncode
        if pip.returncode != 0:
            report["pip_stderr_tail"] = pip.stderr[-1500:]
            log("sherpa wheel unavailable; timing the CLI, which includes model load")
            report.update(benchmark_sherpa_cli(clips, model_dir))
            return report
        log("running resident sherpa-onnx decode")
        result = subprocess.run(
            [sys.executable, str(script)],
            check=False,
            capture_output=True,
            text=True,
        )
        report["stderr_tail"] = result.stderr[-2000:]
        if result.returncode != 0:
            report["error"] = f"sherpa python exited {result.returncode}"
            report.update(benchmark_sherpa_cli(clips, model_dir))
            return report
        # The script prints progress lines and then one JSON object.
        json_line = result.stdout.strip().splitlines()[-1]
        parsed = json.loads(json_line)
        # macOS reports ru_maxrss in bytes. Linux reports kilobytes. This script
        # is only invoked on Darwin for the real run.
        report["load_s"] = parsed["load_s"]
        report["peak_rss_mb"] = round(parsed["ru_maxrss"] / (1024 * 1024), 1)
        report["ru_maxrss_raw"] = parsed["ru_maxrss"]
        report["runs"] = parsed["runs"]
        report["swap_after"] = swap_usage()
        report["mode"] = "python-wheel-resident"
    except Exception as exc:  # noqa: BLE001
        report["error"] = str(exc)
        log(f"sherpa benchmark failed: {exc}")
    return report


def benchmark_sherpa_cli(clips: list[dict], model_dir: Path) -> dict:
    """Each CLI invocation reloads the model, so these times include load."""
    archive = CACHE / f"sherpa-onnx-{SHERPA_TAG}-osx-arm64-shared.tar.bz2"
    download(SHERPA_URL, archive, SHERPA_ARCHIVE_BYTES)
    tree = extract(archive, CACHE / f"sherpa-{SHERPA_TAG}")
    binary = find_named(tree, "sherpa-onnx-offline")
    binary.chmod(binary.stat().st_mode | 0o755)
    lib = binary.parent.parent / "lib"
    env = os.environ.copy()
    env["DYLD_LIBRARY_PATH"] = str(lib) + os.pathsep + env.get("DYLD_LIBRARY_PATH", "")
    runs = []
    peak = 0
    for clip in clips:
        started = time.perf_counter()
        timed = subprocess.run(
            ["/usr/bin/time", "-l", str(binary),
             f"--qwen3-asr-conv-frontend={model_dir / 'conv_frontend.onnx'}",
             f"--qwen3-asr-encoder={model_dir / 'encoder.int8.onnx'}",
             f"--qwen3-asr-decoder={model_dir / 'decoder.int8.onnx'}",
             f"--qwen3-asr-tokenizer={model_dir / 'tokenizer'}",
             "--qwen3-asr-max-new-tokens=256",
             "--num-threads=3",
             clip["path"]],
            check=False,
            capture_output=True,
            text=True,
            env=env,
        )
        elapsed = time.perf_counter() - started
        for line in timed.stderr.splitlines():
            if "maximum resident set size" in line:
                try:
                    peak = max(peak, int(line.split()[0]))
                except ValueError:
                    pass
        runs.append(
            {
                "file": clip["file"],
                "phase": "cold-cli",
                "audio_s": round(clip["duration_s"], 3),
                "seconds": round(elapsed, 3),
                "rtf": round(elapsed / clip["duration_s"], 4),
                "returncode": timed.returncode,
                "stdout_tail": timed.stdout[-500:],
                "stderr_tail": timed.stderr[-500:],
            }
        )
    return {
        "mode": "cli-includes-load",
        "runs": runs,
        "peak_rss_mb": round(peak / (1024 * 1024), 1) if peak else None,
        "swap_after": swap_usage(),
    }


def self_test() -> int:
    clips = load_manifest()
    if len(clips) != 4:
        raise SystemExit(f"expected 4 clips, found {len(clips)}")
    parsed = split_asr_text("language Chinese<asr_text>他称他制作了一个 WiFi 门铃。")
    if parsed["language"] != "Chinese" or "WiFi" not in parsed["text"]:
        raise SystemExit(f"prefix parser failed: {parsed}")
    if split_asr_text("plain")["text"] != "plain":
        raise SystemExit("plain text parser failed")
    for clip in clips:
        if clip["duration_s"] <= 0:
            raise SystemExit(f"bad duration for {clip['file']}")
        log(f"{clip['file']}: {clip['duration_s']:.2f}s")
    log("self-test ok")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--skip-llama", action="store_true")
    parser.add_argument("--skip-sherpa", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    require_arm64_macos()
    CACHE.mkdir(parents=True, exist_ok=True)
    OUT.mkdir(parents=True, exist_ok=True)
    clips = load_manifest()
    disk = shutil.disk_usage(ROOT)
    log(f"disk free {disk.free / 1e9:.1f} GB")
    report = {
        "created_unix": time.time(),
        "disk_free_bytes_at_start": disk.free,
        "hardware": hardware(),
        "note": (
            "GitHub-hosted macOS arm64 runners have 7 GB RAM and 3 vCPUs. "
            "These numbers are not a 16 GB Mac measurement."
        ),
        "clips": [
            {
                "file": clip["file"],
                "role": clip["role"],
                "duration_s": round(clip["duration_s"], 3),
                "reference": clip["raw_transcription"],
                "license": clip["license"],
            }
            for clip in clips
        ],
    }
    report["swap_before"] = swap_usage()
    if not args.skip_llama:
        report["llama_1_7b_q8"] = benchmark_llama(clips)
    if not args.skip_sherpa:
        report["sherpa_0_6b_int8"] = benchmark_sherpa(clips)
    report["swap_after_all"] = swap_usage()
    destination = OUT / "results.json"
    destination.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    log(f"wrote {destination}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
