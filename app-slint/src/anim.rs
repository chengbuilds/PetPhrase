//! petdex 雪碧图帧驱动。状态表与每帧时长照搬上游
//! crafter-station/petdex `src/lib/pet-states.ts` + `petdex-desktop-native/src/sprite.zig`,
//! 行序/帧数/时长以上游为唯一事实来源,改动前先对照上游。

/// 桌宠显示尺寸(逻辑像素);素材可等比缩放,格子尺寸见 [`Atlas`]
pub const FRAME_W: u32 = 192;
pub const FRAME_H: u32 = 208;
const COLUMNS: u32 = 8;
/// 9 个动画状态占前 9 行;v2 图集多出的 2 行是朝向图,不是动画
const ANIM_ROWS: u32 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PetState {
    Idle,
    RunRight,
    RunLeft,
    Wave,
    Jump,
    Failed,
    Waiting,
    Run,
    Review,
}

/// 均匀帧 + 末帧停留(上游 sprite.zig 的 uniform())
const fn uniform<const N: usize>(dur: u32, last: u32) -> [u32; N] {
    let mut f = [dur; N];
    f[N - 1] = last;
    f
}

const IDLE: [u32; 6] = [280, 110, 110, 140, 140, 320];
const RUN_SIDE: [u32; 8] = uniform(120, 220);
const WAVE: [u32; 4] = uniform(140, 280);
const JUMP: [u32; 5] = uniform(140, 280);
const FAILED: [u32; 8] = uniform(140, 240);
const WAITING: [u32; 6] = uniform(150, 260);
const RUN: [u32; 6] = uniform(120, 220);
const REVIEW: [u32; 6] = uniform(150, 280);

impl PetState {
    /// (雪碧图行, 每帧时长 ms)
    fn def(self) -> (u32, &'static [u32]) {
        match self {
            PetState::Idle => (0, &IDLE),
            PetState::RunRight => (1, &RUN_SIDE),
            PetState::RunLeft => (2, &RUN_SIDE),
            PetState::Wave => (3, &WAVE),
            PetState::Jump => (4, &JUMP),
            PetState::Failed => (5, &FAILED),
            PetState::Waiting => (6, &WAITING),
            PetState::Run => (7, &RUN),
            PetState::Review => (8, &REVIEW),
        }
    }
}

/// 闲时彩蛋候选(failed 留给复制失败,不当彩蛋);行数不够的素材自动落选
const SPECIALS: [PetState; 4] = [
    PetState::Jump,
    PetState::Waiting,
    PetState::Review,
    PetState::Run,
];

pub fn pick_special(rows: u32, rand: u32) -> Option<PetState> {
    let avail: Vec<PetState> = SPECIALS
        .iter()
        .copied()
        .filter(|s| s.def().0 < rows)
        .collect();
    if avail.is_empty() {
        None
    } else {
        Some(avail[rand as usize % avail.len()])
    }
}

/// 图集几何:格子像素尺寸 + 行数
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Atlas {
    pub cell_w: u32,
    pub cell_h: u32,
    pub rows: u32,
}

impl Atlas {
    /// 按上游 detectSpriteAtlas 识别 v1(8×9)/v2(8×11),允许整数格等比缩放;
    /// 非标尺寸宽松兜底:按 192×208 切,行数按图高推算,至少 1 行
    pub fn from_size(w: u32, h: u32) -> Atlas {
        for rows in [9u32, 11] {
            let canon_w = (COLUMNS * FRAME_W) as u64;
            let canon_h = (rows * FRAME_H) as u64;
            if w.is_multiple_of(COLUMNS)
                && h.is_multiple_of(rows)
                && w as u64 * canon_h == h as u64 * canon_w
            {
                return Atlas {
                    cell_w: w / COLUMNS,
                    cell_h: h / rows,
                    rows,
                };
            }
        }
        Atlas {
            cell_w: FRAME_W,
            cell_h: FRAME_H,
            rows: (h / FRAME_H).max(1),
        }
    }

    /// 可用于动画的行数(v2 的朝向行不算)
    pub fn anim_rows(&self) -> u32 {
        self.rows.min(ANIM_ROWS)
    }
}

/// 两层状态:base 是持续情境(闲置/等待/审阅/拖动跑步)的循环动画,
/// play_once 的一次性动作(挥手/失败/彩蛋)播完回到 base
pub struct Animator {
    rows: u32,
    base: PetState,
    state: PetState,
    once: bool,
    frame: usize,
}

impl Animator {
    pub fn new(anim_rows: u32) -> Self {
        Animator {
            rows: anim_rows,
            base: PetState::Idle,
            state: PetState::Idle,
            once: false,
            frame: 0,
        }
    }

    /// 一次性动作:播一轮回到 base
    pub fn play_once(&mut self, state: PetState) {
        self.state = state;
        self.once = true;
        self.frame = 0;
    }

    /// 切换持续情境;正在播一次性动作时不打断,播完自然落到新 base
    pub fn set_base(&mut self, base: PetState) {
        if self.base == base {
            return;
        }
        self.base = base;
        if !self.once {
            self.state = base;
            self.frame = 0;
        }
    }

    /// 纯闲置(无情境、无一次性动作)时才允许插彩蛋
    pub fn is_resting(&self) -> bool {
        !self.once && self.state == PetState::Idle
    }

    pub fn rows(&self) -> u32 {
        self.rows
    }

    /// 取当前帧并前进,返回 (row, col, 本帧时长 ms);素材缺该行时落回 idle
    pub fn step(&mut self) -> (u32, u32, u32) {
        if self.state.def().0 >= self.rows {
            self.state = PetState::Idle;
            self.once = false;
            self.frame = 0;
        }
        let (row, durs) = self.state.def();
        let frame = self.frame.min(durs.len() - 1);
        let result = (row, frame as u32, durs[frame]);
        self.frame = frame + 1;
        if self.frame >= durs.len() {
            self.frame = 0;
            if self.once {
                self.state = self.base;
                self.once = false;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_table_matches_upstream() {
        // 上游 pet-states.ts:行号 / 帧数 / 一轮总时长
        let expect = [
            (PetState::Idle, 0, 6, 1100),
            (PetState::RunRight, 1, 8, 1060),
            (PetState::RunLeft, 2, 8, 1060),
            (PetState::Wave, 3, 4, 700),
            (PetState::Jump, 4, 5, 840),
            (PetState::Failed, 5, 8, 1220),
            (PetState::Waiting, 6, 6, 1010),
            (PetState::Run, 7, 6, 820),
            (PetState::Review, 8, 6, 1030),
        ];
        for (s, row, frames, total) in expect {
            let (r, d) = s.def();
            assert_eq!(
                (r, d.len(), d.iter().sum::<u32>()),
                (row, frames, total),
                "{s:?}"
            );
        }
    }

    #[test]
    fn atlas_detects_v1_v2_and_scaled() {
        assert_eq!(
            Atlas::from_size(1536, 1872),
            Atlas {
                cell_w: 192,
                cell_h: 208,
                rows: 9
            }
        );
        assert_eq!(
            Atlas::from_size(1536, 2288),
            Atlas {
                cell_w: 192,
                cell_h: 208,
                rows: 11
            }
        );
        assert_eq!(
            Atlas::from_size(768, 936),
            Atlas {
                cell_w: 96,
                cell_h: 104,
                rows: 9
            }
        );
        assert_eq!(
            Atlas::from_size(1536, 2288).anim_rows(),
            9,
            "v2 朝向行不参与动画"
        );
        // 非标尺寸宽松兜底
        assert_eq!(
            Atlas::from_size(1536, 416),
            Atlas {
                cell_w: 192,
                cell_h: 208,
                rows: 2
            }
        );
        assert_eq!(Atlas::from_size(10, 10).rows, 1);
    }

    #[test]
    fn wave_plays_row3_four_frames_then_idle() {
        let mut a = Animator::new(9);
        a.play_once(PetState::Wave);
        let durs: Vec<u32> = (0..4)
            .map(|i| {
                let (row, col, d) = a.step();
                assert_eq!((row, col), (3, i));
                d
            })
            .collect();
        assert_eq!(durs, [140, 140, 140, 280]);
        assert_eq!(a.step(), (0, 0, 280), "播完一轮回 idle");
    }

    #[test]
    fn base_loops_and_once_returns_to_base() {
        let mut a = Animator::new(9);
        a.set_base(PetState::Waiting);
        for _ in 0..6 {
            a.step();
        }
        assert_eq!(a.step().0, 6, "base 循环播放");
        a.play_once(PetState::Wave);
        a.set_base(PetState::Review); // 一次性动作中途换情境不打断
        assert_eq!(a.step().0, 3);
        for _ in 0..3 {
            a.step();
        }
        assert_eq!(a.step().0, 8, "挥手播完落到新 base");
        assert!(!a.is_resting());
        a.set_base(PetState::Idle);
        assert!(a.is_resting());
    }

    #[test]
    fn missing_rows_fall_back_to_idle_row() {
        let mut a = Animator::new(1);
        a.play_once(PetState::Wave);
        assert_eq!(a.step().0, 0);
    }

    #[test]
    fn specials_respect_rows() {
        assert_eq!(pick_special(2, 0), None, "只有 idle/右跑两行时无彩蛋");
        assert_eq!(pick_special(5, 7), Some(PetState::Jump), "5 行只够 jump");
        for r in 0..8 {
            let s = pick_special(9, r).unwrap();
            assert!(SPECIALS.contains(&s));
        }
    }
}
