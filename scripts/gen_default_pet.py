"""导出内置默认宠「晨光」到 app-slint/pets/default/(spritesheet.webp + pet.json)。

绘制源是同目录的 gen_default_pet.html(Canvas 逐帧绘制,双击可预览);
这里用无头 Edge 打开它,读出页面写进 #out 的 WebP data URL 落盘。只依赖 Python 标准库 + 系统自带 Edge。
运行一次,产物提交进仓库。
"""

import base64
import json
import re
import subprocess
import tempfile
from pathlib import Path

EDGE = Path(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe")
SHEET_W, SHEET_H = 192 * 8, 208 * 9

PET_JSON = {
    "id": "default",
    "displayName": "晨光",
    "description": "白色玻璃泡 + 七彩流光边(PetPhrase 内置)",
    "spritesheetPath": "spritesheet.webp",
}


def main() -> None:
    here = Path(__file__).resolve().parent
    out = here.parent / "app-slint" / "pets" / "default"
    with tempfile.TemporaryDirectory() as profile:  # 独立配置目录:不挂到用户正开着的 Edge 上
        dom = subprocess.run(
            [str(EDGE), "--headless=new", "--disable-gpu", f"--user-data-dir={profile}",
             "--virtual-time-budget=5000", "--dump-dom", (here / "gen_default_pet.html").as_uri()],
            capture_output=True, text=True, encoding="utf-8", check=True,
        ).stdout
    m = re.search(r'<pre id="out">data:image/webp;base64,([A-Za-z0-9+/=]+)</pre>', dom)
    if not m:
        raise SystemExit("页面没产出 WebP(#out 为空),检查 gen_default_pet.html 是否报错")
    webp = base64.b64decode(m.group(1))
    if webp[12:16] != b"VP8X":  # 带透明通道的有损 WebP 必是扩展格式
        raise SystemExit(f"WebP 格式意外: {webp[12:16]!r}")
    w = int.from_bytes(webp[24:27], "little") + 1
    h = int.from_bytes(webp[27:30], "little") + 1
    if (w, h) != (SHEET_W, SHEET_H):
        raise SystemExit(f"雪碧图尺寸 {w}x{h},应为 {SHEET_W}x{SHEET_H}")
    out.mkdir(parents=True, exist_ok=True)
    (out / "spritesheet.webp").write_bytes(webp)
    (out / "pet.json").write_text(json.dumps(PET_JSON, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"OK -> {out} ({len(webp) // 1024} KB)")


if __name__ == "__main__":
    main()
