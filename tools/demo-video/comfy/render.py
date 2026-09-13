#!/usr/bin/env python3
"""Headless MiniMax H3 b-roll driver for the Shipmates demo video.

Queues a shot from shots.json against a running ComfyUI server over its HTTP
API (POST /prompt, poll /history/<id>, fetch /view) and writes the clip to
out/<shot-id>/<seed>-<prompt-hash>.mp4. No GUI, no ComfyUI import, stdlib only.

    python3 render.py --list
    python3 render.py --shot hook-atmosphere-explore --dry-run
    python3 render.py --shot hook-atmosphere

Background and the no-text rule: docs/DEMO_VIDEO.md §6b.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import mimetypes
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))
SHOTS_PATH = os.path.join(HERE, "shots.json")
WORKFLOW_DIR = os.path.join(HERE, "workflows")
PLATE_DIR = os.path.join(HERE, "plates")
OUT_DIR = os.path.join(HERE, "out")

DEFAULT_SERVER = "http://127.0.0.1:8188"
FPS = 24

# Node ids in the committed workflow JSON. Keep in sync with workflows/*.api.json.
NODE_CONDITIONING = "104"
NODE_NOISE = "15"
NODE_SCHEDULER = "9"
NODE_PLATE = "30"
NODE_SAVE = "92"
NODE_UNET = "6"
NODE_GUIDER = "16"
NODE_LORA = "121"

TURBO_LORA = "minimax_h3_fl2v_turbo_8step_v1.0_comfyui_bf16.safetensors"
TURBO_STEPS = 6


def die(msg: str) -> "NoReturn":  # type: ignore[valid-type]
    print(f"render: {msg}", file=sys.stderr)
    raise SystemExit(1)


def align_frame_count(n: int) -> int:
    """Snap up to H3's 17k+5 frame grid.

    Mirrors align_frame_count in ComfyUI's comfy_extras/nodes_minimax_h3.py.
    Computed here rather than in a ComfyMathExpression node so --dry-run shows
    the resolved frame count and the sidecar can record it.
    """
    n = max(5, n)
    while n % 17 != 5:
        n += 1
    return n


def load_shots() -> dict:
    with open(SHOTS_PATH, encoding="utf-8") as fh:
        return json.load(fh)


def resolve_shot(cfg: dict, shot_id: str) -> dict:
    for shot in cfg["shots"]:
        if shot["id"] == shot_id:
            return shot
    known = ", ".join(s["id"] for s in cfg["shots"])
    die(f"unknown shot {shot_id!r}. Known shots: {known}")


class Comfy:
    def __init__(self, server: str, timeout: int = 30):
        self.server = server.rstrip("/")
        self.timeout = timeout
        self.client_id = str(uuid.uuid4())

    def _request(self, path: str, data=None, headers=None, raw=False):
        url = f"{self.server}{path}"
        req = urllib.request.Request(url, data=data, headers=headers or {})
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                payload = resp.read()
        except urllib.error.HTTPError as err:
            body = err.read().decode("utf-8", "replace")
            die(f"{path} failed: HTTP {err.code}\n{body}")
        except urllib.error.URLError as err:
            die(f"cannot reach ComfyUI at {self.server} ({err.reason}). "
                "Start it with: python main.py --listen 127.0.0.1 --port 8188")
        return payload if raw else json.loads(payload)

    def object_info(self, node: str) -> dict:
        return self._request(f"/object_info/{node}")

    def upload_image(self, path: str) -> str:
        """POST /upload/image so LoadImage can find the plate by name."""
        name = os.path.basename(path)
        boundary = f"----shipmates{uuid.uuid4().hex}"
        ctype = mimetypes.guess_type(name)[0] or "application/octet-stream"
        with open(path, "rb") as fh:
            blob = fh.read()
        parts = [
            f"--{boundary}\r\n".encode(),
            f'Content-Disposition: form-data; name="image"; filename="{name}"\r\n'.encode(),
            f"Content-Type: {ctype}\r\n\r\n".encode(),
            blob,
            f"\r\n--{boundary}\r\n".encode(),
            b'Content-Disposition: form-data; name="overwrite"\r\n\r\ntrue\r\n',
            f"--{boundary}--\r\n".encode(),
        ]
        body = b"".join(parts)
        result = self._request(
            "/upload/image",
            data=body,
            headers={"Content-Type": f"multipart/form-data; boundary={boundary}",
                     "Content-Length": str(len(body))},
        )
        uploaded = result.get("name", name)
        subfolder = result.get("subfolder") or ""
        return f"{subfolder}/{uploaded}" if subfolder else uploaded

    def queue(self, graph: dict) -> str:
        body = json.dumps({"prompt": graph, "client_id": self.client_id}).encode()
        result = self._request(
            "/prompt", data=body,
            headers={"Content-Type": "application/json", "Content-Length": str(len(body))},
        )
        return result["prompt_id"]

    def wait(self, prompt_id: str, poll_s: float = 2.0) -> dict:
        """Poll /history until the prompt leaves the queue. H3 takes minutes."""
        started = time.time()
        while True:
            history = self._request(f"/history/{prompt_id}")
            entry = history.get(prompt_id)
            if entry:
                status = entry.get("status", {})
                if status.get("status_str") == "error":
                    for msg in status.get("messages", []):
                        print(f"  {msg}", file=sys.stderr)
                    die(f"ComfyUI reported an execution error for {prompt_id}")
                if status.get("completed", True):
                    return entry
            elapsed = int(time.time() - started)
            print(f"\r  rendering… {elapsed // 60:d}m{elapsed % 60:02d}s", end="", flush=True)
            time.sleep(poll_s)

    def fetch(self, filename: str, subfolder: str, folder_type: str) -> bytes:
        query = urllib.parse.urlencode(
            {"filename": filename, "subfolder": subfolder, "type": folder_type})
        return self._request(f"/view?{query}", raw=True)


def build_graph(cfg: dict, shot: dict, args, plate_ref: str | None) -> tuple[dict, dict]:
    """Patch the committed workflow with this shot's parameters.

    Returns (graph, resolved) where resolved is the sidecar record.
    """
    pipeline = shot["pipeline"]
    workflow_path = os.path.join(WORKFLOW_DIR, f"h3-{pipeline}.api.json")
    if not os.path.exists(workflow_path):
        die(f"no workflow for pipeline {pipeline!r} at {workflow_path}")
    with open(workflow_path, encoding="utf-8") as fh:
        graph = json.load(fh)

    defaults = cfg.get("defaults", {})
    preset_name = args.preset or shot.get("preset") or defaults["preset"]
    preset = cfg["presets"].get(preset_name)
    if preset is None:
        die(f"unknown preset {preset_name!r}. Known: {', '.join(cfg['presets'])}")

    duration = args.duration or shot.get("duration_s") or defaults["duration_s"]
    length = align_frame_count(round(duration * FPS))
    seed = args.seed if args.seed is not None else shot["seed"]
    turbo = args.turbo if args.turbo is not None else shot.get("turbo", defaults["turbo"])
    steps = args.steps or shot.get("steps") or (TURBO_STEPS if turbo else defaults["steps"])

    # The guardrail is appended, never inlined per shot: H3 exposes no negative
    # conditioning (see README), so the no-text rule has to live in the prompt
    # and must be identical for every shot.
    prompt = f"{shot['prompt'].strip()}\n\n{cfg['guardrail'].strip()}"

    cond = graph[NODE_CONDITIONING]["inputs"]
    cond["prompt"] = prompt
    cond["width"] = preset["width"]
    cond["height"] = preset["height"]
    cond["length"] = length

    graph[NODE_NOISE]["inputs"]["noise_seed"] = seed
    graph[NODE_SCHEDULER]["inputs"]["steps"] = steps
    graph[NODE_SAVE]["inputs"]["filename_prefix"] = f"shipmates/{shot['id']}"

    if pipeline == "i2v":
        if plate_ref is None:
            die(f"shot {shot['id']!r} is i2v and needs a plate")
        graph[NODE_PLATE]["inputs"]["image"] = plate_ref

    if turbo:
        graph[NODE_LORA] = {
            "class_type": "LoraLoaderModelOnly",
            "_meta": {"title": "Turbo LoRA (fast preview)"},
            "inputs": {"model": [NODE_UNET, 0], "lora_name": TURBO_LORA, "strength_model": 1.0},
        }
        graph[NODE_GUIDER]["inputs"]["model"] = [NODE_LORA, 0]
        graph[NODE_SCHEDULER]["inputs"]["model"] = [NODE_LORA, 0]

    prompt_hash = hashlib.sha256(prompt.encode()).hexdigest()[:8]
    resolved = {
        "shot": shot["id"],
        "pipeline": pipeline,
        "preset": preset_name,
        "width": preset["width"],
        "height": preset["height"],
        "duration_s_requested": duration,
        "length_frames": length,
        "duration_s_actual": round(length / FPS, 3),
        "fps": FPS,
        "seed": seed,
        "steps": steps,
        "turbo": turbo,
        "plate": plate_ref,
        "prompt_sha256_8": prompt_hash,
        "prompt": prompt,
    }
    return graph, resolved


def collect_outputs(entry: dict) -> list[dict]:
    """SaveVideo reports through ui.PreviewVideo, which keys results as 'images'."""
    results = []
    for node_output in entry.get("outputs", {}).values():
        results.extend(node_output.get("images", []))
    return results


def cmd_list(cfg: dict) -> int:
    print(f"{'shot':<28} {'pipe':<5} {'seed':<6} used by")
    for shot in cfg["shots"]:
        print(f"{shot['id']:<28} {shot['pipeline']:<5} {shot['seed']:<6} {shot.get('used_by', '')}")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--shot", help="shot id from shots.json")
    parser.add_argument("--list", action="store_true", help="list shots and exit")
    parser.add_argument("--dry-run", action="store_true",
                        help="resolve and print the graph without queueing")
    parser.add_argument("--server", default=os.environ.get("COMFY_SERVER", DEFAULT_SERVER))
    parser.add_argument("--seed", type=int, help="override the shot's fixed seed")
    parser.add_argument("--duration", type=float, help="override duration in seconds")
    parser.add_argument("--preset", help="override the resolution preset")
    parser.add_argument("--steps", type=int, help="override sampler steps")
    parser.add_argument("--turbo", action="store_true", default=None,
                        help="use the 8-step turbo LoRA for fast previews")
    parser.add_argument("--timeout", type=int, default=30, help="per-request timeout (s)")
    args = parser.parse_args(argv)

    cfg = load_shots()
    if args.list:
        return cmd_list(cfg)
    if not args.shot:
        parser.error("--shot is required (or use --list)")

    shot = resolve_shot(cfg, args.shot)

    plate_ref = None
    plate_path = None
    if shot["pipeline"] == "i2v":
        plate_path = os.path.join(PLATE_DIR, shot["plate"])
        if not os.path.exists(plate_path):
            die(f"plate not found: {plate_path}\n"
                f"       Author a text-free plate there, or explore with "
                f"--shot {shot['id']}-explore (t2v).")
        plate_ref = shot["plate"]

    if args.dry_run:
        graph, resolved = build_graph(cfg, shot, args, plate_ref)
        print(json.dumps({"resolved": resolved, "graph": graph}, indent=2))
        return 0

    comfy = Comfy(args.server, timeout=args.timeout)
    if plate_path:
        plate_ref = comfy.upload_image(plate_path)
        print(f"  uploaded plate: {plate_ref}")

    graph, resolved = build_graph(cfg, shot, args, plate_ref)

    dest_dir = os.path.join(OUT_DIR, shot["id"])
    os.makedirs(dest_dir, exist_ok=True)
    stem = f"{resolved['seed']}-{resolved['prompt_sha256_8']}"

    print(f"  {shot['id']}: {resolved['width']}x{resolved['height']}, "
          f"{resolved['length_frames']}f ({resolved['duration_s_actual']}s), "
          f"seed {resolved['seed']}, {resolved['steps']} steps"
          f"{' [turbo]' if resolved['turbo'] else ''}")

    prompt_id = comfy.queue(graph)
    entry = comfy.wait(prompt_id)
    print()

    outputs = collect_outputs(entry)
    if not outputs:
        die(f"prompt {prompt_id} finished with no video output")

    written = []
    for index, item in enumerate(outputs):
        blob = comfy.fetch(item["filename"], item.get("subfolder", ""),
                           item.get("type", "output"))
        ext = os.path.splitext(item["filename"])[1] or ".mp4"
        suffix = "" if len(outputs) == 1 else f"-{index}"
        dest = os.path.join(dest_dir, f"{stem}{suffix}{ext}")
        with open(dest, "wb") as fh:
            fh.write(blob)
        written.append(dest)

    sidecar = os.path.join(dest_dir, f"{stem}.json")
    with open(sidecar, "w", encoding="utf-8") as fh:
        json.dump(resolved, fh, indent=2)
        fh.write("\n")

    for path in written:
        print(f"  wrote {os.path.relpath(path, HERE)}")
    print(f"  wrote {os.path.relpath(sidecar, HERE)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
