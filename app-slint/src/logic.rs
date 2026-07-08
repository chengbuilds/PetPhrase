//! 面板纯逻辑:短句判定、搜索、气泡流布局、贴宠定位。
//! Slint 侧只做绝对定位渲染,布局全部在这里算,可单测。

use crate::storage::PhraseData;

pub const SHORT_MAX_CHARS: usize = 10;
pub const FONT_PX: f32 = 13.0;
pub const PANEL_W: f32 = 300.0;
pub const PANEL_H: f32 = 400.0;
pub const LIST_PAD: f32 = 10.0;
pub const GAP: f32 = 6.0;
const CHIP_PAD_X: f32 = 12.0;
const CHIP_H: f32 = 28.0;
const CARD_PAD: f32 = 10.0;
const CARD_LINE_H: f32 = 20.0;
const BADGE_H: f32 = 16.0;

pub fn is_short(text: &str) -> bool {
    !text.contains('\n') && text.chars().count() <= SHORT_MAX_CHARS
}

/// CJK 全宽、ASCII 半宽的近似测宽。
/// ponytail: 估算而非真实测量,±10% 误差由 chip 弹性 padding 吸收;偏差明显再接真实测量。
pub fn estimate_text_width(text: &str, font_px: f32) -> f32 {
    text.chars()
        .map(|c| {
            if (c as u32) < 0x2E80 {
                font_px * 0.55
            } else {
                font_px
            }
        })
        .sum()
}

#[derive(Debug, Clone, PartialEq)]
pub struct LaidItem {
    /// (group_idx, phrase_idx) 定位回源数据
    pub group_idx: usize,
    pub phrase_idx: usize,
    pub text: String,
    pub badge: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub is_chip: bool,
}

struct Ctx {
    items: Vec<LaidItem>,
    cursor_x: f32,
    cursor_y: f32,
    avail: f32,
}

impl Ctx {
    fn new(avail: f32) -> Self {
        Ctx {
            items: Vec::new(),
            cursor_x: 0.0,
            cursor_y: 0.0,
            avail,
        }
    }

    fn newline_if_needed(&mut self, w: f32) {
        if self.cursor_x > 0.0 && self.cursor_x + w > self.avail {
            self.cursor_x = 0.0;
            self.cursor_y = self.items.iter().map(|i| i.y + i.h).fold(0.0, f32::max) + GAP;
        }
    }

    fn close_row(&mut self) {
        if self.cursor_x > 0.0 {
            self.cursor_x = 0.0;
            self.cursor_y = self.items.iter().map(|i| i.y + i.h).fold(0.0, f32::max) + GAP;
        }
    }

    fn push_chip(&mut self, gi: usize, pi: usize, text: &str) {
        let w = (estimate_text_width(text, FONT_PX) + CHIP_PAD_X * 2.0).min(self.avail);
        self.newline_if_needed(w);
        self.items.push(LaidItem {
            group_idx: gi,
            phrase_idx: pi,
            text: text.into(),
            badge: String::new(),
            x: self.cursor_x,
            y: self.cursor_y,
            w,
            h: CHIP_H,
            is_chip: true,
        });
        self.cursor_x += w + GAP;
    }

    fn push_card(&mut self, gi: usize, pi: usize, text: &str, badge: &str) {
        self.close_row();
        let inner_w = (self.avail - CARD_PAD * 2.0).max(1.0); // avail 现为常量宽,兜底防改小后出负宽
        let text_w = estimate_text_width(text, FONT_PX);
        let lines_est = (text_w / inner_w).ceil().max(1.0) + text.matches('\n').count() as f32;
        let lines = lines_est.min(2.0);
        let badge_h = if badge.is_empty() { 0.0 } else { BADGE_H };
        let h = CARD_PAD * 2.0 + lines * CARD_LINE_H + badge_h;
        self.items.push(LaidItem {
            group_idx: gi,
            phrase_idx: pi,
            text: text.into(),
            badge: badge.into(),
            x: 0.0,
            y: self.cursor_y,
            w: self.avail,
            h,
            is_chip: false,
        });
        self.cursor_y += h + GAP;
    }
}

/// 当前分组的混排布局(连续短句成气泡流,长句独占卡片)
pub fn layout_group(data: &PhraseData, group_idx: usize, avail_w: f32) -> Vec<LaidItem> {
    let mut ctx = Ctx::new(avail_w);
    let Some(group) = data.groups.get(group_idx) else {
        return ctx.items;
    };
    for (pi, p) in group.phrases.iter().enumerate() {
        if is_short(&p.text) {
            ctx.push_chip(group_idx, pi, &p.text);
        } else {
            ctx.push_card(group_idx, pi, &p.text, "");
        }
    }
    ctx.items
}

/// 搜索结果布局:跨组、全部为带来源徽标的卡片
pub fn layout_search(data: &PhraseData, query: &str, avail_w: f32) -> Vec<LaidItem> {
    let mut ctx = Ctx::new(avail_w);
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return ctx.items;
    }
    for (gi, g) in data.groups.iter().enumerate() {
        for (pi, p) in g.phrases.iter().enumerate() {
            if p.text.to_lowercase().contains(&q) {
                let badge = format!("来自「{}」", g.name);
                ctx.push_card(gi, pi, &p.text, &badge);
            }
        }
    }
    ctx.items
}

pub fn content_height(items: &[LaidItem]) -> f32 {
    items.iter().map(|i| i.y + i.h).fold(0.0, f32::max)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub right_side: bool,
}

const SCREEN_GAP: f32 = 12.0;

/// 面板贴宠定位:优先上方,越界翻转/钳制;预览侧按右侧余量定
pub fn panel_position(pet: Rect, panel_w: f32, panel_h: f32, work: Rect) -> Placement {
    let mut x = pet.x;
    if x + panel_w > work.x + work.w {
        x = pet.x + pet.w - panel_w;
    }
    x = x.clamp(work.x, (work.x + work.w - panel_w).max(work.x));

    let mut y = pet.y - panel_h - SCREEN_GAP;
    if y < work.y {
        y = pet.y + pet.h + SCREEN_GAP;
    }
    y = y.clamp(work.y, (work.y + work.h - panel_h).max(work.y));

    let right_room = work.x + work.w - (x + panel_w);
    Placement {
        x,
        y,
        right_side: right_room >= 260.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Group, Phrase, PhraseData};

    fn data() -> PhraseData {
        PhraseData {
            groups: vec![Group {
                id: "g1".into(),
                name: "工作".into(),
                icon: None,
                phrases: vec![
                    Phrase { id: "a".into(), text: "收到".into() },
                    Phrase { id: "b".into(), text: "好的没问题".into() },
                    Phrase { id: "c".into(), text: "这是一条相当长的常用语,会被排成卡片并且可能被截断显示,超过两行的部分省略,再补充一些字数确保估宽后稳定超过两行的高度。".into() },
                    Phrase { id: "d".into(), text: "辛苦了".into() },
                ],
            }],
        }
    }

    #[test]
    fn short_boundary() {
        assert!(is_short("一二三四五六七八九十"));
        assert!(!is_short("一二三四五六七八九十一"));
        assert!(!is_short("短\n句"));
    }

    #[test]
    fn chips_flow_then_card_breaks_row() {
        let items = layout_group(&data(), 0, 280.0);
        assert_eq!(items.len(), 4);
        // 前两条 chip 同行
        assert!(items[0].is_chip && items[1].is_chip);
        assert_eq!(items[0].y, items[1].y);
        assert!(items[1].x > items[0].x);
        // 卡片独占整行且换行
        assert!(!items[2].is_chip);
        assert_eq!(items[2].x, 0.0);
        assert!(items[2].y > items[0].y);
        assert_eq!(items[2].w, 280.0);
        // 卡片后的 chip 另起一行
        assert!(items[3].is_chip);
        assert!(items[3].y > items[2].y);
    }

    #[test]
    fn chip_wraps_when_row_full() {
        let mut d = data();
        d.groups[0].phrases = (0..8)
            .map(|i| Phrase {
                id: i.to_string(),
                text: "八字长度短句啊".into(),
            })
            .collect();
        let items = layout_group(&d, 0, 280.0);
        let rows: std::collections::BTreeSet<i32> = items.iter().map(|i| i.y as i32).collect();
        assert!(rows.len() >= 2, "应换行,实际 rows={rows:?}");
        for it in &items {
            assert!(it.x + it.w <= 280.0 + 0.01);
        }
    }

    #[test]
    fn search_crosses_groups_with_badge() {
        let items = layout_search(&data(), "好的", 280.0);
        assert_eq!(items.len(), 1);
        assert!(!items[0].is_chip);
        assert_eq!(items[0].badge, "来自「工作」");
        let empty = layout_search(&data(), "  ", 280.0);
        assert!(empty.is_empty());
    }

    #[test]
    fn panel_position_flip_and_clamp() {
        let work = Rect {
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1040.0,
        };
        let p = panel_position(
            Rect {
                x: 800.0,
                y: 600.0,
                w: 192.0,
                h: 208.0,
            },
            300.0,
            400.0,
            work,
        );
        assert_eq!((p.x, p.y), (800.0, 600.0 - 400.0 - 12.0));
        assert!(p.right_side);
        // 贴顶翻下方
        let p2 = panel_position(
            Rect {
                x: 800.0,
                y: 10.0,
                w: 192.0,
                h: 208.0,
            },
            300.0,
            400.0,
            work,
        );
        assert_eq!(p2.y, 10.0 + 208.0 + 12.0);
        // 贴右缘:钳制且预览贴左
        let p3 = panel_position(
            Rect {
                x: 1700.0,
                y: 600.0,
                w: 192.0,
                h: 208.0,
            },
            300.0,
            400.0,
            work,
        );
        assert!(p3.x + 300.0 <= 1920.0);
        assert!(!p3.right_side);
    }
}
