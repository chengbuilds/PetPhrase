# 生成 GitHub Social Preview 卡(1280×640,Settings → Social preview 手动上传)
# 素材:screenshots/pet-desktop.png(白底精灵,四角泛洪抠背景);短语文案为虚构示例
from collections import deque
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
W, H = 1280, 640

INK = (30, 41, 59)        # #1e293b
INK_SOFT = (71, 85, 105)  # #475569
MUTED = (100, 116, 139)   # #64748b
ACCENT = (249, 115, 22)   # #f97316
BORDER = (226, 232, 240)  # #e2e8f0


def font(path, size):
    return ImageFont.truetype(f"C:/Windows/Fonts/{path}", size)


def keyed_sprite(path, scale):
    """白底精灵抠图:仅从四角泛洪连通的背景置透明,保住图案内部白色。"""
    im = Image.open(path).convert("RGBA")
    px = im.load()
    w, h = im.size
    bg = px[0, 0][:3]
    seen = set()
    q = deque([(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)])
    while q:
        x, y = q.popleft()
        if (x, y) in seen or not (0 <= x < w and 0 <= y < h):
            continue
        seen.add((x, y))
        r, g, b, _ = px[x, y]
        if abs(r - bg[0]) + abs(g - bg[1]) + abs(b - bg[2]) > 60:
            continue
        px[x, y] = (r, g, b, 0)
        q.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)])
    return im.resize((w * scale, h * scale), Image.NEAREST)


card = Image.new("RGBA", (W, H))
d = ImageDraw.Draw(card)
# 竖向暖白渐变底
for y in range(H):
    t = y / H
    d.line([(0, y), (W, y)], fill=(
        int(255 - 3 * t), int(251 - 12 * t), int(245 - 32 * t), 255))
# 右上柔和橘色光斑
glow = Image.new("RGBA", (W, H), (0, 0, 0, 0))
ImageDraw.Draw(glow).ellipse([W - 460, -260, W + 220, 320], fill=ACCENT + (26,))
card = Image.alpha_composite(card, glow.filter(ImageFilter.GaussianBlur(60)))
d = ImageDraw.Draw(card)

# ---- 左:文案区 ----
X = 84
d.rounded_rectangle([X, 96, X + 10, 196], 5, fill=ACCENT)
d.text((X + 34, 88), "PetPhrase", font=font("segoeuib.ttf", 84), fill=INK)
d.text((X, 236), "Windows 桌宠常用语 / 话术工具",
       font=font("msyhbd.ttc", 42), fill=INK_SOFT)
d.text((X, 318), "点击桌宠 · 弹出短语面板 · 一键复制",
       font=font("msyh.ttc", 30), fill=MUTED)
d.text((X, 372), "Desktop pet · canned responses · one-click copy",
       font=font("segoeui.ttf", 25), fill=(148, 163, 184))

chips = ["轻量原生", "Rust + Slint", "petdex 生态"]
cf = font("msyh.ttc", 22)
cx = X
for label in chips:
    tw = d.textlength(label, font=cf)
    d.rounded_rectangle([cx, 452, cx + tw + 36, 496], 22,
                        outline=ACCENT, width=2, fill=(255, 247, 237, 255))
    d.text((cx + 18, 461), label, font=cf, fill=ACCENT)
    cx += tw + 56

# ---- 右:气泡 + 宠物 ----
def bubble(box, text, fnt, fill=(255, 255, 255, 255), ink=INK_SOFT,
           outline=BORDER, pad=(20, 13)):
    sh = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    ImageDraw.Draw(sh).rounded_rectangle(
        [box[0] + 3, box[1] + 6, box[2] + 3, box[3] + 6], 18, fill=(15, 23, 42, 36))
    nonlocal_card = Image.alpha_composite(card, sh.filter(ImageFilter.GaussianBlur(5)))
    dd = ImageDraw.Draw(nonlocal_card)
    dd.rounded_rectangle(box, 18, fill=fill, outline=outline, width=1)
    dd.text((box[0] + pad[0], box[1] + pad[1]), text, font=fnt, fill=ink)
    return nonlocal_card

bf = font("msyh.ttc", 26)
for i, (text, dx) in enumerate([("您好,请问有什么可以帮您?", 0),
                                 ("好的,马上为您处理", 56),
                                 ("收到,感谢您的反馈", -104)]):
    x0 = 758 + dx
    y0 = 118 + i * 86
    card = bubble([x0, y0, x0 + d.textlength(text, font=bf) + 40, y0 + 62], text, bf)
d = ImageDraw.Draw(card)

pet = keyed_sprite(ROOT / "screenshots" / "pet-desktop.png", 3)
px_, py_ = 964, H - pet.height - 46
card.alpha_composite(pet, (px_, py_))
# 「已复制 ✓」小气泡贴宠(YaHei 无 ✓ 字形,对勾手绘)
toast = "已复制"
tf = font("msyhbd.ttc", 24)
tw = d.textlength(toast, font=tf)
tx, ty = px_ - tw - 92, py_ + 158
card = bubble([tx, ty, tx + tw + 64, ty + 52], toast, tf,
              fill=(255, 247, 237, 255), ink=ACCENT, outline=ACCENT, pad=(18, 10))
d = ImageDraw.Draw(card)
ck = (tx + tw + 28, ty + 26)
d.line([ck, (ck[0] + 6, ck[1] + 7), (ck[0] + 17, ck[1] - 7)],
       fill=ACCENT, width=4, joint="curve")

out = ROOT / "screenshots" / "social-preview.png"
Image.alpha_composite(
    Image.new("RGBA", (W, H), (255, 255, 255, 255)), card
).convert("RGB").save(out)
print("saved", out)
