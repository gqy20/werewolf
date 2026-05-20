"""配置加载"""
import json
from pathlib import Path

_BASE_DIR = Path(__file__).resolve().parents[2]  # werewolf/
DEFAULT_CONFIG_PATH = _BASE_DIR / "config.json"
REGISTRY_PATH = _BASE_DIR / "data" / "registry.json"


def load_config(path: Path | None = None) -> dict:
    p = path or DEFAULT_CONFIG_PATH
    with open(p) as f:
        return json.load(f)


def load_registry() -> dict:
    if REGISTRY_PATH.exists():
        with open(REGISTRY_PATH) as f:
            return json.load(f)
    return {"players": {}}


def save_registry(data: dict):
    REGISTRY_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(REGISTRY_PATH, "w") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
