"""生成内置默认宠 Mochi 的 petdex 格式雪碧图。

1536x1872 = 8列 x 9行,每帧 192x208(与 petdex 实测素材同规格)。
行序: idle, wave, run, failed, review, jump, extra1, extra2, extra3(后7行复用 idle)。
每状态 6 帧(第 7-8 列留空)。运行一次,产物提交进仓库。
"""

import math
from pathlib import Path

from PIL import Image, ImageDraw

FRAME_W, FRAME_H = 192, 208
COLS, ROWS = 8, 9
FRAMES = 6

BODY = (249, 115, 22, 255)        # 橘
BODY_DARK = (234, 88, 12, 255)
HIGHLIGHT = (255, 200, 150, 255)
EYE = (30, 41, 59, 255)
BLUSH = (251, 146, 60, 160)
CHEEK = (255, 255, 255, 230)


def draw_blob(draw: ImageDraw.ImageDraw, cx: float, base_y: float, squash: float, lean: float) -> None:
    """圆润史莱姆:squash 呼吸压扁系数,lean 左右倾斜像素。"""
    w = 130 * (1 + (1 - squash) * 0.5)
    h = 110 * squash
    left = cx - w / 2 + lean
    top = base_y - h
    # 身体 + 底部阴影
    draw.ellipse([cx - 62, base_y - 14, cx + 62, base_y + 8], fill=(30, 41, 59, 36))
    draw.ellipse([left, top, left + w, base_y + 4], fill=BODY, outline=BODY_DARK, width=4)
    # 高光
    draw.ellipse([left + w * 0.16, top + h * 0.12, left + w * 0.42, top + h * 0.38], fill=HIGHLIGHT)
    # 眼睛
    ey = top + h * 0.45
    for dx in (-24, 24):
        draw.ellipse([cx + dx - 8 + lean, ey - 10, cx + dx + 8 + lean, ey + 10], fill=EYE)
        draw.ellipse([cx + dx - 3 + lean, ey - 7, cx + dx + 3 + lean, ey - 1], fill=CHEEK)
    # 腮红 + 嘴
    for dx in (-44, 44):
        draw.ellipse([cx + dx - 9 + lean, ey + 8, cx + dx + 9 + lean, ey + 20], fill=BLUSH)
    draw.arc([cx - 12 + lean, ey + 4, cx + 12 + lean, ey + 24], 20, 160, fill=EYE, width=4)


def draw_arm(draw: ImageDraw.ImageDraw, cx: float, base_y: float, angle_deg: float) -> None:
    """wave 招手小圆手。"""
    ax = cx + 66 + 10 * math.cos(math.radians(angle_deg))
    ay = base_y - 70 - 26 * math.sin(math.radians(angle_deg))
    draw.ellipse([ax - 14, ay - 14, ax + 14, ay + 14], fill=BODY, outline=BODY_DARK, width=3)


def main() -> None:
    sheet = Image.new("RGBA", (FRAME_W * COLS, FRAME_H * ROWS), (0, 0, 0, 0))
    cx, base_y = FRAME_W / 2, FRAME_H - 30.0

    for row in range(ROWS):
        for col in range(FRAMES):
            frame = Image.new("RGBA", (FRAME_W, FRAME_H), (0, 0, 0, 0))
            d = ImageDraw.Draw(frame)
            t = col / FRAMES * 2 * math.pi
            if row == 1:  # wave:身体微倾 + 招手
                draw_blob(d, cx, base_y, 1.0 + 0.03 * math.sin(t), lean=6 * math.sin(t))
                draw_arm(d, cx, base_y, 30 + 50 * (0.5 + 0.5 * math.sin(t)))
            else:  # idle 及其余行:呼吸起伏
                draw_blob(d, cx, base_y, 1.0 + 0.06 * math.sin(t), lean=0)
            sheet.paste(frame, (col * FRAME_W, row * FRAME_H))

    out = Path(__file__).resolve().parent.parent / "app-slint" / "pets" / "default"
    out.mkdir(parents=True, exist_ok=True)
    sheet.save(out / "spritesheet.png")
    (out / "pet.json").write_text(
        '{\n  "name": "Mochi",\n  "slug": "default",\n  "kind": "slime",\n'
        '  "frame": { "width": 192, "height": 208 }\n}\n',
        encoding="utf-8",
    )
    print(f"OK -> {out}")


if __name__ == "__main__":
    main()
