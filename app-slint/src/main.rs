//! PetPhrase Slint 原生版 —— 单进程装配:
//! 宠物/面板/预览/设置四窗口、托盘、剪贴板、自启、单实例。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod anim;
mod logic;
mod pet_loader;
mod storage;
mod updater;

use anim::{Animator, PetState};
use logic::LaidItem;
use pet_loader::PetInfo;
use slint::winit_030::{winit, WinitWindowAccessor};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

slint::include_modules!();

/// 分组图标固定集,顺序与 common.slint GroupIcon 分支一致
const ICON_KEYS: [&str; 12] = [
    "star",
    "briefcase",
    "headphones",
    "code",
    "mail",
    "message-circle",
    "smile",
    "heart",
    "fish",
    "map-pin",
    "credit-card",
    "folder",
];

fn icon_idx(icon: &Option<String>) -> i32 {
    icon.as_deref()
        .and_then(|k| ICON_KEYS.iter().position(|x| *x == k))
        .unwrap_or(11) as i32
}

fn data_dir() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| {
            rfd::MessageDialog::new()
                .set_title("PetPhrase 无法启动")
                .set_description("找不到数据目录:APPDATA 环境变量缺失。")
                .set_level(rfd::MessageLevel::Error)
                .show();
            std::process::exit(1);
        });
        PathBuf::from(base).join("PetPhrase")
    })
    .clone()
}

/// 设置窗「数据」区消息统一入口:错误红色高亮,普通消息灰色
fn set_data_msg(app: &Rc<App>, msg: &str, is_err: bool) {
    app.settings_win.set_data_msg(msg.into());
    app.settings_win.set_data_msg_error(is_err);
}

/// 保存失败双通道提示:设置窗「数据」区 + 面板红色横幅(面板内编辑时设置窗常不可见)。
/// 磁盘故障是全局条件,任一保存成功即视为恢复,顺手清面板横幅。
fn report_persist(app: &Rc<App>, result: std::io::Result<()>, what: &str) {
    match result {
        Ok(()) => {
            app.panel.set_save_error("".into());
            // 恢复后清设置窗残留的保存失败提示;按内容判断,别误伤「已导出 ✓」等其他消息
            if app.settings_win.get_data_msg().contains("保存失败") {
                set_data_msg(app, "", false);
            }
        }
        Err(e) => {
            let msg = format!("⚠ {what}保存失败:{e}");
            set_data_msg(app, &msg, true);
            app.panel.set_save_error(msg.into());
        }
    }
}

fn persist_data(app: &Rc<App>) {
    let st = app.state.borrow();
    let result = storage::save_phrases(&data_dir(), &st.data);
    drop(st);
    report_persist(app, result, "常用语");
}

fn persist_settings(app: &Rc<App>) {
    let st = app.state.borrow();
    let result = storage::save_settings(&data_dir(), &st.settings);
    drop(st);
    report_persist(app, result, "设置");
}

/// 三档桌宠缩放,索引对应设置里的 小/中/大
const PET_SCALES: [f32; 3] = [0.5, 0.75, 1.0];

fn pet_scale_idx(scale: f32) -> i32 {
    if scale < 0.625 {
        0
    } else if scale < 0.875 {
        1
    } else {
        2
    }
}

fn pet_roots(custom: &Option<String>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.join("pets"));
        }
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        roots.push(PathBuf::from(home).join(".codex").join("pets"));
    }
    if let Some(c) = custom {
        roots.push(PathBuf::from(c));
    }
    roots
}

/// 设置窗确认框待执行动作(泛化:同一个框服务多种危险操作)
enum ConfirmAction {
    DeleteGroup,
    /// 已读入并校验通过的导入数据,确认后快照+覆盖
    ImportReplace(storage::PhraseData),
}

struct State {
    data: storage::PhraseData,
    settings: storage::Settings,
    pets: Vec<PetInfo>,
    active_group: usize,
    items: Vec<LaidItem>,
    animator: Animator,
    clipboard: Option<arboard::Clipboard>,
    thumb_cache: HashMap<String, slint::Image>,
    panel_native_ready: bool,
    /// show 后是否真正拿到过焦点 —— 防初始 Focused(false) 误隐藏
    panel_got_focus: bool,
    /// 就地编辑器目标:(分组下标, Some(短语下标)=编辑 / None=新增)
    pending_edit: Option<(usize, Option<usize>)>,
    /// 常驻面板因打开设置而暂隐,设置关闭后恢复
    panel_resume_after_settings: bool,
    /// 已发现的新版本(检查成功后填入,「立即更新」据此下载)
    update: Option<updater::Update>,
    /// 检查/下载进行中,防重入
    update_busy: bool,
    /// 托盘「检查更新」项句柄,发现新版后改文案
    update_menu: Option<tray_icon::menu::MenuItem>,
    /// 确认框按下「确认」后要执行的动作
    confirm_action: Option<ConfirmAction>,
    /// 单槽删除撤销:(分组下标, 原短语下标, 短语)
    last_deleted: Option<(usize, usize, storage::Phrase)>,
    /// 闲时动画:idle 帧计数与下次彩蛋触发阈值
    idle_ticks: i32,
    next_special: i32,
}

struct App {
    pet: PetWindow,
    panel: PanelWindow,
    settings_win: SettingsWindow,
    state: RefCell<State>,
    hide_timer: slint::Timer,
    move_timer: slint::Timer,
    update_timer: slint::Timer,
    undo_timer: slint::Timer,
}

// 后台线程结果经 invoke_from_event_loop 回主线程时取 App:
// Rc 不能跨线程捕获,闭包只带 Send 数据,落地后从主线程 thread_local 取
thread_local! {
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

fn with_app(f: impl FnOnce(&Rc<App>)) {
    APP.with(|a| {
        if let Some(app) = a.borrow().as_ref() {
            f(app);
        }
    });
}

/// 二实例唤醒信号文件:第二实例写入后退出,主实例托盘轮询发现即找回桌宠
fn wake_signal_path() -> PathBuf {
    data_dir().join("wake.signal")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let instance = single_instance::SingleInstance::new("petphrase-slint")?;
    if !instance.is_single() {
        // 不再静默退出:通知主实例把桌宠找回来,用户双击图标必须有反应
        let _ = std::fs::write(wake_signal_path(), b"wake");
        return Ok(());
    }
    let _ = std::fs::remove_file(wake_signal_path()); // 清残留信号,防启动即误触发

    // 崩溃留痕:release 无控制台,没有日志就只能靠用户口述「用着用着没了」
    std::panic::set_hook(Box::new(|info| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let log = data_dir().join("crash.log");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
        {
            use std::io::Write;
            let _ = writeln!(f, "[unix {ts}] v{} {info}", updater::CURRENT_VERSION);
        }
        rfd::MessageDialog::new()
            .set_title("PetPhrase 遇到错误")
            .set_description(format!(
                "程序发生内部错误,已记录日志:\n{}\n重新启动即可继续使用。",
                log.display()
            ))
            .set_level(rfd::MessageLevel::Error)
            .show();
    }));

    // 软件渲染:实测常驻 ~15MB(GPU 渲染 ~78MB),雪碧图 6fps 动画绰绰有余
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .with_winit_window_attributes_hook(|attrs| attrs.with_transparent(true))
        .select()?;

    let dir = data_dir();
    storage::backup_phrases(&dir);
    storage::backup_settings(&dir);
    let data = storage::load_phrases(&dir);
    let settings = storage::load_settings(&dir);
    let roots = pet_roots(&settings.custom_pet_dir);
    let refs: Vec<&std::path::Path> = roots.iter().map(|p| p.as_path()).collect();
    let pets = pet_loader::scan_pets(&refs);

    let active_group = settings
        .last_group
        .as_ref()
        .and_then(|id| data.groups.iter().position(|g| &g.id == id))
        .unwrap_or(0);

    let app = Rc::new(App {
        pet: PetWindow::new()?,
        panel: PanelWindow::new()?,
        settings_win: SettingsWindow::new()?,
        state: RefCell::new(State {
            data,
            settings,
            pets,
            active_group,
            items: Vec::new(),
            animator: Animator::new(1, 1),
            clipboard: arboard::Clipboard::new().ok(),
            thumb_cache: HashMap::new(),
            panel_native_ready: false,
            panel_got_focus: false,
            pending_edit: None,
            panel_resume_after_settings: false,
            update: None,
            update_busy: false,
            update_menu: None,
            confirm_action: None,
            last_deleted: None,
            idle_ticks: 0,
            next_special: rand_ticks(),
        }),
        hide_timer: slint::Timer::default(),
        move_timer: slint::Timer::default(),
        update_timer: slint::Timer::default(),
        undo_timer: slint::Timer::default(),
    });
    APP.with(|a| *a.borrow_mut() = Some(app.clone()));

    let solid = app.state.borrow().settings.theme == "solid";
    set_theme(&app, solid);
    app.pet.set_pet_scale(app.state.borrow().settings.pet_scale);
    // 面板高度从设置恢复(载入时钳制,防手改配置文件出离谱值)
    {
        let mut st = app.state.borrow_mut();
        st.settings.panel_h = st
            .settings
            .panel_h
            .clamp(storage::PANEL_H_MIN, storage::PANEL_H_MAX);
        app.panel.set_panel_h(st.settings.panel_h);
    }

    wire_pet(&app);
    wire_panel(&app);
    wire_settings(&app);
    setup_frame_timer(&app);
    let _tray = setup_tray(&app)?;

    // 启动 10s 后静默检查一次更新;此后每 24h 重查一次
    // (开机自启用户一挂数周,只查启动那一次等于永远不查)
    {
        let a = app.clone();
        app.update_timer.start(
            slint::TimerMode::SingleShot,
            Duration::from_secs(10),
            move || {
                if a.state.borrow().settings.auto_check_update {
                    start_update_check(&a, false);
                }
                let a2 = a.clone();
                a.update_timer.start(
                    slint::TimerMode::Repeated,
                    Duration::from_secs(24 * 3600),
                    move || {
                        if a2.state.borrow().settings.auto_check_update {
                            start_update_check(&a2, false);
                        }
                    },
                );
            },
        );
    }

    refresh_pet_sprite(&app);
    refresh_panel(&app);
    // refresh_settings 延迟到设置窗打开时:缩略图解码占 10+MB/宠,不该常驻

    // 设置窗关闭 → 释放缩略图缓存与模型;常驻面板若因设置暂隐则恢复
    {
        let a = app.clone();
        app.settings_win.window().on_close_requested(move || {
            a.state.borrow_mut().thumb_cache.clear();
            a.settings_win
                .set_pets(ModelRc::new(VecModel::from(Vec::<PetCardUi>::new())));
            let resume = std::mem::take(&mut a.state.borrow_mut().panel_resume_after_settings);
            if resume && a.panel.get_pinned() {
                show_panel(&a);
            }
            slint::CloseRequestResponse::HideWindow
        });
    }

    app.pet.show()?;
    // 落位 + 原生属性(跳任务栏);恢复坐标后钳制到可见显示器,
    // 换显示器/改缩放留下的屏外坐标会让宠永远找不到
    {
        let st = app.state.borrow();
        if let Some((x, y)) = st.settings.pet_pos {
            app.pet
                .window()
                .set_position(slint::PhysicalPosition::new(x, y));
        }
    }
    clamp_pet_to_screen(&app);
    app.pet
        .window()
        .with_winit_window(|w: &winit::window::Window| {
            use winit::platform::windows::WindowExtWindows;
            w.set_skip_taskbar(true);
        });

    slint::run_event_loop_until_quit()?;
    Ok(())
}

fn set_theme(app: &Rc<App>, solid: bool) {
    app.panel.global::<Theme>().set_solid(solid);
    app.settings_win.global::<Theme>().set_solid(solid);
    app.settings_win.set_solid_theme(solid);
}

/* ================= 宠物窗 ================= */

/// 闲时彩蛋间隔:90~330 帧(FRAME_MS≈183ms → 约 16~60 秒),纳秒时钟当随机源免拉依赖
fn rand_ticks() -> i32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    90 + (nanos % 241) as i32
}

/// 宠窗中心不在任何显示器内 → 挪回主显示器右下角(留出任务栏与边距)
fn clamp_pet_to_screen(app: &Rc<App>) {
    app.pet
        .window()
        .with_winit_window(|w: &winit::window::Window| {
            let pos = w.outer_position().unwrap_or_default();
            let size = w.outer_size();
            let cx = pos.x + size.width as i32 / 2;
            let cy = pos.y + size.height as i32 / 2;
            let visible = w.available_monitors().any(|m| {
                let mp = m.position();
                let ms = m.size();
                cx >= mp.x
                    && cx < mp.x + ms.width as i32
                    && cy >= mp.y
                    && cy < mp.y + ms.height as i32
            });
            if visible {
                return;
            }
            if let Some(m) = w.primary_monitor().or_else(|| w.available_monitors().next()) {
                let mp = m.position();
                let ms = m.size();
                let nx = (mp.x + ms.width as i32 - size.width as i32 - 80).max(mp.x);
                let ny = (mp.y + ms.height as i32 - size.height as i32 - 120).max(mp.y);
                w.set_outer_position(winit::dpi::PhysicalPosition::new(nx, ny));
            }
        });
}

/// 重新压回置顶带顶端:后创建的 topmost 窗口会盖住宠,重显示时重申一次。
/// (独占全屏与 UAC 安全桌面无解,属系统语义;不做定时抢前台)
fn assert_topmost(win: &slint::Window) {
    win.with_winit_window(|w: &winit::window::Window| {
        w.set_window_level(winit::window::WindowLevel::Normal);
        w.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
    });
}

/// 托盘找回/二实例唤醒共用:显示、钳回屏内、重申置顶、挥手打招呼
fn recover_pet(app: &Rc<App>) {
    let _ = app.pet.show();
    clamp_pet_to_screen(app);
    assert_topmost(app.pet.window());
    app.state.borrow_mut().animator.play(PetState::Wave, true);
}

fn wire_pet(app: &Rc<App>) {
    let a = app.clone();
    app.pet.on_pet_clicked(move || {
        dbg_log("pet clicked");
        if a.pet.get_missing() {
            open_settings(&a); // 素材缺失:点占位提示直达设置选宠
            return;
        }
        a.state.borrow_mut().animator.play(PetState::Wave, true);
        toggle_panel(&a);
    });

    let a = app.clone();
    app.pet.on_drag_start(move || {
        a.pet
            .window()
            .with_winit_window(|w: &winit::window::Window| {
                let _ = w.drag_window();
            });
    });

    // 拖完保存位置(去抖 500ms);常驻开启时面板实时跟随
    let a = app.clone();
    app.pet.window().on_winit_window_event(move |_, event| {
        if let winit::event::WindowEvent::Moved(pos) = event {
            if a.panel.get_pinned() && a.panel.window().is_visible() {
                if let Some(p) = compute_panel_placement(&a) {
                    a.panel
                        .window()
                        .set_position(slint::PhysicalPosition::new(p.x as i32, p.y as i32));
                }
            }
            let (x, y) = (pos.x, pos.y);
            let a2 = a.clone();
            a.move_timer.start(
                slint::TimerMode::SingleShot,
                Duration::from_millis(500),
                move || {
                    a2.state.borrow_mut().settings.pet_pos = Some((x, y));
                    persist_settings(&a2);
                },
            );
        }
        slint::winit_030::EventResult::Propagate
    });
}

/// 解码雪碧图并推算网格;失败(文件损坏/非图片)返回 None
fn try_load_sheet(path: &str) -> Option<(slint::Image, i32, i32)> {
    let img = slint::Image::load_from_path(std::path::Path::new(path)).ok()?;
    let size = img.size();
    let (rows, cols) = anim::grid_from_image(size.width, size.height);
    Some((img, rows, cols))
}

/// 选中宠优先,解码失败依次回退其它可用宠;全部失败显示缺失占位
/// (不显示占位的话窗口全透明还挡点击,用户以为程序没启动)
fn refresh_pet_sprite(app: &Rc<App>) {
    let candidates: Vec<String> = {
        let st = app.state.borrow();
        let selected = st
            .pets
            .iter()
            .find(|p| p.id == st.settings.pet_id && p.error.is_none());
        selected
            .into_iter()
            .chain(st.pets.iter().filter(|p| p.error.is_none()))
            .map(|p| p.spritesheet.clone())
            .collect() // 选中宠在首位,可能重复一次,解码成功即返回无所谓
    };
    for path in &candidates {
        if let Some((img, rows, cols)) = try_load_sheet(path) {
            app.state.borrow_mut().animator = Animator::new(rows, cols);
            app.pet.set_sheet(img);
            app.pet.set_missing(false);
            return;
        }
    }
    app.pet.set_missing(true);
    app.pet.set_sheet(slint::Image::default());
}

/// 应用缩放并移窗,锚定底边中点——宠的「脚」原地不动。
/// 移窗触发 Moved 事件,pet_pos 由既有去抖逻辑保存。
fn apply_pet_scale(app: &Rc<App>, old_scale: f32, new_scale: f32) {
    app.pet.set_pet_scale(new_scale);
    let win = app.pet.window();
    let sf = win.scale_factor();
    let pos = win.position();
    let dw = anim::FRAME_W as f32 * (old_scale - new_scale) * sf;
    let dh = anim::FRAME_H as f32 * (old_scale - new_scale) * sf;
    let nx = pos.x + (dw / 2.0).round() as i32;
    let ny = pos.y + dh.round() as i32;
    win.set_position(slint::PhysicalPosition::new(nx, ny));
}

fn setup_frame_timer(app: &Rc<App>) {
    let a = app.clone();
    let timer = Box::leak(Box::new(slint::Timer::default()));
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(anim::FRAME_MS),
        move || {
            // 宠隐藏时不空转(托盘隐藏后帧步进/属性刷新纯浪费)
            if !a.pet.window().is_visible() {
                return;
            }
            let (row, col) = {
                let mut st = a.state.borrow_mut();
                // 闲够一段随机时长播一个彩蛋动画(jump/run/review/failed),
                // 雪碧图 6 行素材以前只用了 idle/wave 两行
                if st.animator.is_idle() {
                    st.idle_ticks += 1;
                    if st.idle_ticks >= st.next_special {
                        st.idle_ticks = 0;
                        st.next_special = rand_ticks();
                        let rows = st.animator.rows();
                        let rand = st.next_special as u32; // 已是随机值,直接复用作挑选源
                        if let Some(s) = anim::pick_special(rows, rand) {
                            st.animator.play(s, true);
                        }
                    }
                } else {
                    st.idle_ticks = 0;
                }
                st.animator.step()
            };
            a.pet.set_frame_row(row);
            a.pet.set_frame_col(col);
        },
    );
}

/* ================= 面板窗 ================= */

const LIST_AVAIL_W: f32 = logic::PANEL_W - logic::LIST_PAD * 2.0;

fn refresh_panel(app: &Rc<App>) {
    let (tabs, items, content_h, show_grid) = {
        let st = app.state.borrow();
        let query = app.panel.get_search_text().to_string();
        let items = if query.trim().is_empty() {
            logic::layout_group(
                &st.data,
                st.active_group,
                LIST_AVAIL_W,
                st.settings.sort_by_use,
            )
        } else {
            logic::layout_search(&st.data, &query, LIST_AVAIL_W)
        };
        let tabs: Vec<TabUi> = st
            .data
            .groups
            .iter()
            .enumerate()
            .map(|(i, g)| TabUi {
                id: g.id.clone().into(),
                name: g.name.clone().into(),
                icon_idx: icon_idx(&g.icon),
                active: i == st.active_group,
            })
            .collect();
        let h = logic::content_height(&items);
        let show_grid = st.data.groups.len() > 4;
        (tabs, items, h, show_grid)
    };

    let ui_items: Vec<PanelItemUi> = items
        .iter()
        .map(|it| PanelItemUi {
            text: it.text.clone().into(),
            badge: it.badge.clone().into(),
            x: it.x,
            y: it.y,
            w: it.w,
            h: it.h,
            is_chip: it.is_chip,
        })
        .collect();

    app.state.borrow_mut().items = items;
    app.panel.set_tabs(ModelRc::new(VecModel::from(tabs)));
    app.panel.set_items(ModelRc::new(VecModel::from(ui_items)));
    app.panel.set_content_h(content_h);
    app.panel.set_show_grid_btn(show_grid);
    app.panel.set_copied_idx(-1);
    app.panel.set_failed_idx(-1);
}

fn wire_panel(app: &Rc<App>) {
    let a = app.clone();
    app.panel.on_tab_clicked(move |i| {
        select_group(&a, i as usize);
    });

    let a = app.clone();
    app.panel.on_search_changed(move |_| {
        a.panel.invoke_reset_scroll(); // 结果集变了,滚动位置沿用会露空白
        refresh_panel(&a);
    });

    let a = app.clone();
    app.panel.on_item_clicked(move |i| copy_item(&a, i));

    let a = app.clone();
    app.panel.on_gear_clicked(move || {
        open_settings(&a);
        // 常驻时 open_settings 已暂隐面板并保留图钉;非常驻正常收起
        if !a.panel.get_pinned() {
            hide_panel(&a);
        }
    });

    let a = app.clone();
    app.panel.on_escape_pressed(move || hide_panel(&a));

    // ---- 右键菜单动作(LaidItem 带 group_idx/phrase_idx 定位回源数据) ----
    let a = app.clone();
    app.panel.on_item_delete_requested(move |i| {
        {
            let mut st = a.state.borrow_mut();
            let Some((gi, pi)) = st
                .items
                .get(i as usize)
                .map(|it| (it.group_idx, it.phrase_idx))
            else {
                return;
            };
            if let Some(g) = st.data.groups.get_mut(gi) {
                if pi < g.phrases.len() {
                    let p = g.phrases.remove(pi);
                    st.last_deleted = Some((gi, pi, p));
                }
            }
        }
        offer_undo(&a);
        persist_data(&a);
        refresh_panel(&a);
        if a.settings_win.window().is_visible() {
            refresh_settings(&a);
        }
    });

    let a = app.clone();
    app.panel.on_undo_clicked(move || undo_delete(&a));

    // 拖高手柄松手:钳制、持久化、按新高重摆位置(面板在宠上方时高度变了要往上挪)
    let a = app.clone();
    app.panel.on_height_resized(move |h| {
        let h = h.clamp(storage::PANEL_H_MIN, storage::PANEL_H_MAX);
        a.state.borrow_mut().settings.panel_h = h;
        a.panel.set_panel_h(h);
        persist_settings(&a);
        if let Some(p) = compute_panel_placement(&a) {
            a.panel
                .window()
                .set_position(slint::PhysicalPosition::new(p.x as i32, p.y as i32));
        }
    });

    // 编辑/添加:面板内就地编辑器,不跳设置窗
    let a = app.clone();
    app.panel.on_item_edit_requested(move |i| {
        let Some((gi, pi, text)) = a
            .state
            .borrow()
            .items
            .get(i as usize)
            .map(|it| (it.group_idx, it.phrase_idx, it.text.clone()))
        else {
            return;
        };
        a.state.borrow_mut().pending_edit = Some((gi, Some(pi)));
        a.panel.set_editor_is_add(false);
        a.panel.set_editor_text(text.into());
        a.panel.set_editor_open(true);
    });

    // i < 0 = 空白区右键,落到当前分组
    let a = app.clone();
    app.panel.on_item_add_requested(move |i| {
        let gi = {
            let st = a.state.borrow();
            if st.data.groups.is_empty() {
                return;
            }
            st.items
                .get(i.max(0) as usize)
                .filter(|_| i >= 0)
                .map(|it| it.group_idx)
                .unwrap_or(st.active_group.min(st.data.groups.len() - 1))
        };
        a.state.borrow_mut().pending_edit = Some((gi, None));
        a.panel.set_editor_is_add(true);
        a.panel.set_editor_text("".into());
        a.panel.set_editor_open(true);
    });

    let a = app.clone();
    app.panel.on_editor_saved(move |text| {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        // 与导入同一套上限:导入拒 1 万字,应用内添加没理由放行
        if text.chars().count() > storage::MAX_TEXT_CHARS {
            a.panel.set_save_error(
                format!("⚠ 内容过长(上限 {} 字),未保存", storage::MAX_TEXT_CHARS).into(),
            );
            return;
        }
        let Some((gi, target)) = a.state.borrow_mut().pending_edit.take() else {
            return;
        };
        {
            let mut st = a.state.borrow_mut();
            let Some(g) = st.data.groups.get_mut(gi) else {
                return;
            };
            match target {
                Some(pi) => {
                    if let Some(p) = g.phrases.get_mut(pi) {
                        p.text = text;
                    }
                }
                None => {
                    if g.phrases.len() >= storage::MAX_PHRASES_PER_GROUP {
                        a.panel.set_save_error("⚠ 该分组短语已达上限".into());
                        return;
                    }
                    g.phrases.push(storage::Phrase::new(uid(), text));
                }
            }
        }
        persist_data(&a);
        refresh_panel(&a);
        if a.settings_win.window().is_visible() {
            refresh_settings(&a);
        }
    });

    // 失焦即隐:仅在拿到过焦点后才生效,防 show 初期的 Focused(false)。
    // 底层 filter 是替换语义,注册一次即可,放这儿免得每次 show_panel 重复注册
    let a = app.clone();
    app.panel.window().on_winit_window_event(move |_, event| {
        match event {
            winit::event::WindowEvent::Focused(true) => {
                a.state.borrow_mut().panel_got_focus = true;
            }
            // 常驻开启时失焦不隐,收起只走图钉关/点宠/Esc
            winit::event::WindowEvent::Focused(false)
                if a.state.borrow().panel_got_focus && !a.panel.get_pinned() =>
            {
                hide_panel(&a);
            }
            _ => {}
        }
        slint::winit_030::EventResult::Propagate
    });
}

/// active_group 与 settings.last_group 是同一事实的两份存储,
/// 必须只经此处同步更新(新建/删除/拖拽分组以前各改各的,重启后回不到预期分组)
fn set_active_group(app: &Rc<App>, idx: usize) {
    {
        let mut st = app.state.borrow_mut();
        let idx = idx.min(st.data.groups.len().saturating_sub(1));
        st.active_group = idx;
        st.settings.last_group = st.data.groups.get(idx).map(|g| g.id.clone());
    }
    persist_settings(app);
}

fn select_group(app: &Rc<App>, idx: usize) {
    if idx >= app.state.borrow().data.groups.len() {
        return;
    }
    set_active_group(app, idx);
    app.panel.set_search_text("".into());
    app.panel.invoke_reset_scroll();
    refresh_panel(app);
    // 设置窗可见才刷:refresh_settings 会为缺缓存的宠解码整张雪碧图(~11.5MB/宠)做缩略图,
    // 面板点 Tab 不该触发,缩略图只在设置窗打开期间常驻
    if app.settings_win.window().is_visible() {
        refresh_settings(app);
    }
}

fn copy_item(app: &Rc<App>, i: i32) {
    let (text, gi, pi) = match app.state.borrow().items.get(i as usize) {
        Some(it) => (it.text.clone(), it.group_idx, it.phrase_idx),
        None => return,
    };
    // 剪贴板句柄可能已失效(或启动时就没拿到):失败先重建再试一次,
    // 否则启动那一下失败就整个会话永远复制不了
    let ok = {
        let mut st = app.state.borrow_mut();
        let mut done = false;
        for retry in 0..2 {
            done = match st.clipboard.as_mut() {
                Some(cb) => cb.set_text(text.clone()).is_ok(),
                None => false,
            };
            if done {
                break;
            }
            if retry == 0 {
                st.clipboard = arboard::Clipboard::new().ok();
            }
        }
        done
    };
    if ok {
        // 计数走 (group_idx, phrase_idx) 回源,搜索/频率排序下标不影响归属
        {
            let mut st = app.state.borrow_mut();
            if let Some(p) = st
                .data
                .groups
                .get_mut(gi)
                .and_then(|g| g.phrases.get_mut(pi))
            {
                p.use_count = p.use_count.saturating_add(1);
            }
        }
        persist_data(app); // 只落盘不刷面板:排序开着也不能在点击瞬间重排列表
        app.panel.set_copied_idx(i);
        app.state.borrow_mut().animator.play(PetState::Wave, true);
        let a = app.clone();
        if app.panel.get_pinned() {
            // 常驻:不收面板,✓ 反馈稍后自清
            app.hide_timer.start(
                slint::TimerMode::SingleShot,
                Duration::from_millis(800),
                move || a.panel.set_copied_idx(-1),
            );
        } else {
            app.hide_timer.start(
                slint::TimerMode::SingleShot,
                Duration::from_millis(200),
                move || hide_panel(&a),
            );
        }
    } else {
        app.panel.set_failed_idx(i);
        app.panel
            .set_save_error("⚠ 复制失败,请重试(剪贴板被其它程序占用)".into());
    }
}

/// 删除后开 8s 撤销窗口:面板横幅 + 设置页按钮双入口,超时自动收(单槽,新删除顶旧)
fn offer_undo(app: &Rc<App>) {
    app.panel.set_undo_open(true);
    app.settings_win.set_can_undo(true);
    let a = app.clone();
    app.undo_timer.start(
        slint::TimerMode::SingleShot,
        Duration::from_secs(8),
        move || clear_undo(&a),
    );
}

fn clear_undo(app: &Rc<App>) {
    app.state.borrow_mut().last_deleted = None;
    app.panel.set_undo_open(false);
    app.settings_win.set_can_undo(false);
}

/// 撤销期间数据可能又变过:分组/下标钳制后插回,宁可位置偏一点也不丢内容
fn undo_delete(app: &Rc<App>) {
    let restored = {
        let mut st = app.state.borrow_mut();
        match st.last_deleted.take() {
            Some((gi, pi, p)) if !st.data.groups.is_empty() => {
                let gi = gi.min(st.data.groups.len() - 1);
                let g = &mut st.data.groups[gi];
                let pi = pi.min(g.phrases.len());
                g.phrases.insert(pi, p);
                true
            }
            _ => false,
        }
    };
    clear_undo(app);
    if restored {
        persist_data(app);
        refresh_panel(app);
        if app.settings_win.window().is_visible() {
            refresh_settings(app);
        }
    }
}

/// 确认后的导入落地:先快照当前数据(失败即中止,不能拿用户数据赌),再整体替换
fn apply_import(app: &Rc<App>, data: storage::PhraseData) {
    let snap = {
        let st = app.state.borrow();
        storage::snapshot_before_import(&data_dir(), &st.data)
    };
    if let Err(e) = snap {
        set_data_msg(app, &format!("⚠ 导入中止:备份当前数据失败({e})"), true);
        return;
    }
    {
        let mut st = app.state.borrow_mut();
        st.data = data;
        st.last_deleted = None; // 旧数据的撤销槽随之作废
    }
    clear_undo(app);
    set_active_group(app, 0);
    persist_data(app);
    set_data_msg(app, "已导入 ✓(原数据备份在 phrases.pre-import.json)", false);
    refresh_settings(app);
    refresh_panel(app);
}

fn ensure_panel_native(app: &Rc<App>) {
    if app.state.borrow().panel_native_ready {
        return;
    }
    let solid_pref = app.state.borrow().settings.theme == "solid";
    let mut acrylic_ok = false;
    app.panel
        .window()
        .with_winit_window(|w: &winit::window::Window| {
            use winit::platform::windows::WindowExtWindows;
            w.set_skip_taskbar(true);
            acrylic_ok = window_vibrancy::apply_acrylic(w, Some((255, 255, 255, 170))).is_ok();
        });
    // acrylic 失败或用户选实底 → solid
    set_theme(app, solid_pref || !acrylic_ok);
    app.state.borrow_mut().panel_native_ready = true;
}

/// 贴宠定位(物理像素);面板高度随设置可调
fn compute_panel_placement(app: &Rc<App>) -> Option<logic::Placement> {
    let scale = app.pet.window().scale_factor();
    let panel_h = app.state.borrow().settings.panel_h;
    app.pet
        .window()
        .with_winit_window(|w: &winit::window::Window| {
            // 拿不到窗口坐标就别摆了,(0,0) 兜底会把面板甩到屏幕角落
            let pos = w.outer_position().ok()?;
            let size = w.outer_size();
            let (mx, my, mw, mh) = match w.current_monitor() {
                Some(m) => {
                    let p = m.position();
                    let s = m.size();
                    (p.x as f32, p.y as f32, s.width as f32, s.height as f32)
                }
                None => (0.0, 0.0, 1920.0, 1080.0),
            };
            Some(logic::panel_position(
                logic::Rect {
                    x: pos.x as f32,
                    y: pos.y as f32,
                    w: size.width as f32,
                    h: size.height as f32,
                },
                logic::PANEL_W * scale,
                panel_h * scale,
                logic::Rect {
                    x: mx,
                    y: my,
                    w: mw,
                    h: mh,
                },
            ))
        })
        .flatten()
}

fn toggle_panel(app: &Rc<App>) {
    if app.panel.window().is_visible() {
        hide_panel(app);
        return;
    }
    show_panel(app);
}

fn show_panel(app: &Rc<App>) {
    let Some(placement) = compute_panel_placement(app) else {
        dbg_log("show_panel: no placement (pet native window missing?)");
        return;
    };
    app.state.borrow_mut().panel_got_focus = false;

    app.panel.set_search_text("".into());
    app.panel
        .set_panel_h(app.state.borrow().settings.panel_h);
    app.panel.invoke_reset_scroll();
    refresh_panel(app);
    dbg_log(&format!(
        "show_panel: show at {},{}",
        placement.x, placement.y
    ));
    if let Err(e) = app.panel.show() {
        dbg_log(&format!("panel.show err: {e}"));
        return;
    }
    app.panel
        .window()
        .set_position(slint::PhysicalPosition::new(
            placement.x as i32,
            placement.y as i32,
        ));
    ensure_panel_native(app);

    app.panel
        .window()
        .with_winit_window(|w: &winit::window::Window| {
            w.focus_window();
        });
}

fn dbg_log(msg: &str) {
    #[cfg(debug_assertions)]
    eprintln!("[petphrase] {msg}");
    #[cfg(not(debug_assertions))]
    let _ = msg;
}

fn hide_panel(app: &Rc<App>) {
    app.panel.set_grid_open(false);
    app.panel.set_ctx_open(false);
    // 编辑器一并关+清目标:否则重开面板露出陈旧编辑器,期间数据变动过的话 pending_edit 下标错位会改错短语
    app.panel.set_editor_open(false);
    app.state.borrow_mut().pending_edit = None;
    app.panel.set_pinned(false); // 收起即解除常驻(点宠/Esc/图钉关,语义一致)
    let _ = app.panel.window().hide();
}

/* ================= 设置窗 ================= */

fn open_settings(app: &Rc<App>) {
    // 常驻面板遇设置窗:暂隐(保留图钉),设置关闭后恢复
    if app.panel.get_pinned() && app.panel.window().is_visible() {
        app.state.borrow_mut().panel_resume_after_settings = true;
        app.panel.set_ctx_open(false);
        app.panel.set_editor_open(false);
        let _ = app.panel.window().hide();
    }
    refresh_pets(app);
    refresh_settings(app);
    let _ = app.settings_win.show();
    app.settings_win
        .window()
        .with_winit_window(|w: &winit::window::Window| {
            w.focus_window();
        });
}

fn refresh_pets(app: &Rc<App>) {
    let mut st = app.state.borrow_mut();
    let roots = pet_roots(&st.settings.custom_pet_dir);
    let refs: Vec<&std::path::Path> = roots.iter().map(|p| p.as_path()).collect();
    st.pets = pet_loader::scan_pets(&refs);
    // 裁掉已不在列表里的缩略图,防止反复切换目录时缓存只增不减
    let keep: HashSet<String> = st.pets.iter().map(|p| p.spritesheet.clone()).collect();
    st.thumb_cache.retain(|k, _| keep.contains(k));
}

fn refresh_settings(app: &Rc<App>) {
    let mut st = app.state.borrow_mut();

    let groups: Vec<GroupRowUi> = st
        .data
        .groups
        .iter()
        .enumerate()
        .map(|(i, g)| GroupRowUi {
            name: g.name.clone().into(),
            icon_idx: icon_idx(&g.icon),
            count: g.phrases.len() as i32,
            selected: i == st.active_group,
        })
        .collect();

    let active = st.active_group.min(st.data.groups.len().saturating_sub(1));
    st.active_group = active;
    let (name, gicon, phrases): (SharedString, i32, Vec<PhraseRowUi>) =
        match st.data.groups.get(active) {
            Some(g) => (
                g.name.clone().into(),
                icon_idx(&g.icon),
                g.phrases
                    .iter()
                    .map(|p| PhraseRowUi {
                        text: p.text.clone().into(),
                        display: p.text.replace('\n', " ").into(),
                    })
                    .collect(),
            ),
            None => ("".into(), 11, Vec::new()),
        };

    // 宠物卡(缩略图缓存)
    let pets = st.pets.clone();
    let selected_pet = st.settings.pet_id.clone();
    let mut cards = Vec::new();
    for p in &pets {
        let thumb = if p.error.is_none() {
            let cache = &mut st.thumb_cache;
            cache
                .entry(p.spritesheet.clone())
                .or_insert_with(|| pet_loader::load_thumb(&p.spritesheet))
                .clone()
        } else {
            slint::Image::default()
        };
        cards.push(PetCardUi {
            name: p.name.clone().into(),
            err: p.error.clone().unwrap_or_default().into(),
            selected: p.id == selected_pet,
            thumb,
        });
    }

    let custom_dir: SharedString = st
        .settings
        .custom_pet_dir
        .clone()
        .unwrap_or_default()
        .into();
    let has_group = !st.data.groups.is_empty();
    let size_idx = pet_scale_idx(st.settings.pet_scale);
    drop(st);

    app.settings_win.set_pet_size_idx(size_idx);
    app.settings_win.set_renaming(false); // 任何数据刷新都退出分组名编辑态

    app.settings_win
        .set_groups(ModelRc::new(VecModel::from(groups)));
    app.settings_win
        .set_phrases(ModelRc::new(VecModel::from(phrases)));
    app.settings_win
        .set_pets(ModelRc::new(VecModel::from(cards)));
    app.settings_win.set_group_name(name);
    app.settings_win.set_group_icon_idx(gicon);
    app.settings_win.set_has_group(has_group);
    app.settings_win.set_custom_dir(custom_dir);
}

fn uid() -> String {
    // 时间戳+计数足够本地唯一,免拉 uuid 依赖
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("p-{t:x}-{}", N.fetch_add(1, Ordering::Relaxed))
}

fn wire_settings(app: &Rc<App>) {
    let a = app.clone();
    app.settings_win
        .on_group_selected(move |i| select_group(&a, i as usize));

    let a = app.clone();
    app.settings_win.on_group_add(move || {
        let new_idx = {
            let mut st = a.state.borrow_mut();
            if st.data.groups.len() >= storage::MAX_GROUPS {
                return;
            }
            st.data.groups.push(storage::Group {
                id: uid(),
                name: "新分组".into(),
                icon: Some("folder".into()),
                phrases: Vec::new(),
            });
            st.data.groups.len() - 1
        };
        set_active_group(&a, new_idx); // 同步 last_group,重启才能回到新建的分组
        persist_data(&a);
        refresh_settings(&a);
        refresh_panel(&a);
    });

    let a = app.clone();
    app.settings_win.on_group_renamed(move |name| {
        let name = name.trim().to_string();
        if name.is_empty() {
            refresh_settings(&a);
            return;
        }
        {
            let mut st = a.state.borrow_mut();
            let idx = st.active_group;
            if let Some(g) = st.data.groups.get_mut(idx) {
                g.name = name;
            }
        }
        persist_data(&a);
        refresh_settings(&a);
        refresh_panel(&a);
    });

    let a = app.clone();
    app.settings_win.on_group_icon_set(move |i| {
        {
            let mut st = a.state.borrow_mut();
            let idx = st.active_group;
            if let Some(g) = st.data.groups.get_mut(idx) {
                g.icon = Some(ICON_KEYS[i.clamp(0, 11) as usize].to_string());
            }
        }
        persist_data(&a);
        refresh_settings(&a);
        refresh_panel(&a);
    });

    // 删除分组走应用内确认框
    let a = app.clone();
    app.settings_win.on_group_delete(move || {
        let (title, msg) = {
            let st = a.state.borrow();
            match st.data.groups.get(st.active_group) {
                Some(g) => (
                    "删除分组".to_string(),
                    format!(
                        "将删除「{}」及其中 {} 条常用语,此操作不可撤销。",
                        g.name,
                        g.phrases.len()
                    ),
                ),
                None => return,
            }
        };
        a.state.borrow_mut().confirm_action = Some(ConfirmAction::DeleteGroup);
        a.settings_win.set_confirm_kind(0);
        a.settings_win.set_confirm_action_label("删除".into());
        a.settings_win.set_confirm_title(title.into());
        a.settings_win.set_confirm_msg(msg.into());
        a.settings_win.set_confirm_visible(true);
    });

    let a = app.clone();
    app.settings_win.on_confirm_ok(move || {
        let action = a.state.borrow_mut().confirm_action.take();
        match action {
            Some(ConfirmAction::DeleteGroup) => {
                {
                    let mut st = a.state.borrow_mut();
                    let idx = st.active_group;
                    if idx < st.data.groups.len() {
                        st.data.groups.remove(idx);
                    }
                }
                set_active_group(&a, 0);
                persist_data(&a);
                refresh_settings(&a);
                refresh_panel(&a);
            }
            Some(ConfirmAction::ImportReplace(data)) => apply_import(&a, data),
            None => {}
        }
    });

    let a = app.clone();
    app.settings_win.on_group_dropped(move |from, to| {
        let to = {
            let mut st = a.state.borrow_mut();
            let len = st.data.groups.len() as i32;
            if len == 0 {
                return; // clamp(0, -1) 会 panic;与短语拖拽的空判断保持一致
            }
            let from = from.clamp(0, len - 1) as usize;
            let to = to.clamp(0, len - 1) as usize;
            if from == to {
                return;
            }
            let g = st.data.groups.remove(from);
            st.data.groups.insert(to, g);
            to
        };
        set_active_group(&a, to);
        persist_data(&a);
        refresh_settings(&a);
        refresh_panel(&a);
    });

    let a = app.clone();
    app.settings_win.on_phrase_add(move |text| {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        if text.chars().count() > storage::MAX_TEXT_CHARS {
            set_data_msg(
                &a,
                &format!("⚠ 内容过长(上限 {} 字),未添加", storage::MAX_TEXT_CHARS),
                true,
            );
            return;
        }
        {
            let mut st = a.state.borrow_mut();
            let idx = st.active_group;
            if let Some(g) = st.data.groups.get_mut(idx) {
                if g.phrases.len() >= storage::MAX_PHRASES_PER_GROUP {
                    drop(st);
                    set_data_msg(&a, "⚠ 该分组短语已达上限", true);
                    return;
                }
                g.phrases.push(storage::Phrase::new(uid(), text));
            }
        }
        persist_data(&a);
        refresh_settings(&a);
        refresh_panel(&a);
    });

    let a = app.clone();
    app.settings_win.on_phrase_edited(move |i, text| {
        let text = text.trim().to_string();
        // 清空/超长以前被静默吞掉,用户以为改成功了
        if text.is_empty() {
            set_data_msg(&a, "⚠ 内容不能为空,未修改", true);
            refresh_settings(&a);
            return;
        }
        if text.chars().count() > storage::MAX_TEXT_CHARS {
            set_data_msg(
                &a,
                &format!("⚠ 内容过长(上限 {} 字),未修改", storage::MAX_TEXT_CHARS),
                true,
            );
            refresh_settings(&a);
            return;
        }
        {
            let mut st = a.state.borrow_mut();
            let idx = st.active_group;
            if let Some(p) = st
                .data
                .groups
                .get_mut(idx)
                .and_then(|g| g.phrases.get_mut(i as usize))
            {
                p.text = text;
            }
        }
        persist_data(&a);
        refresh_settings(&a);
        refresh_panel(&a);
    });

    let a = app.clone();
    app.settings_win.on_phrase_delete(move |i| {
        {
            let mut st = a.state.borrow_mut();
            let idx = st.active_group;
            if let Some(g) = st.data.groups.get_mut(idx) {
                if (i as usize) < g.phrases.len() {
                    let p = g.phrases.remove(i as usize);
                    st.last_deleted = Some((idx, i as usize, p));
                }
            }
        }
        offer_undo(&a);
        persist_data(&a);
        refresh_settings(&a);
        refresh_panel(&a);
    });

    let a = app.clone();
    app.settings_win.on_undo_delete(move || undo_delete(&a));

    let a = app.clone();
    app.settings_win.on_phrase_dropped(move |from, to| {
        {
            let mut st = a.state.borrow_mut();
            let idx = st.active_group;
            let Some(g) = st.data.groups.get_mut(idx) else {
                return;
            };
            let len = g.phrases.len() as i32;
            if len == 0 {
                return;
            }
            let from = from.clamp(0, len - 1) as usize;
            let to = to.clamp(0, len - 1) as usize;
            if from == to {
                return;
            }
            let p = g.phrases.remove(from);
            g.phrases.insert(to, p);
        }
        persist_data(&a);
        refresh_settings(&a);
        refresh_panel(&a);
    });

    let a = app.clone();
    app.settings_win.on_pet_selected(move |i| {
        // 扫描期只验文件存在;真解码成功才落选择,否则设置显示已换、桌面还是旧宠
        let (id, sheet) = {
            let st = a.state.borrow();
            let Some(p) = st.pets.get(i as usize) else {
                return;
            };
            if p.error.is_some() {
                return;
            }
            (p.id.clone(), p.spritesheet.clone())
        };
        if try_load_sheet(&sheet).is_none() {
            set_data_msg(&a, "⚠ 该宠物雪碧图无法解码(文件损坏?),已保留当前选择", true);
            return;
        }
        a.state.borrow_mut().settings.pet_id = id;
        persist_settings(&a);
        refresh_pet_sprite(&a);
        refresh_settings(&a);
    });

    let a = app.clone();
    app.settings_win.on_pet_size_changed(move |i| {
        let new_scale = PET_SCALES[i.clamp(0, 2) as usize];
        let old_scale = {
            let mut st = a.state.borrow_mut();
            let old = st.settings.pet_scale;
            st.settings.pet_scale = new_scale;
            old
        };
        persist_settings(&a);
        if (new_scale - old_scale).abs() > 0.001 {
            apply_pet_scale(&a, old_scale, new_scale);
        }
        a.settings_win.set_pet_size_idx(pet_scale_idx(new_scale));
    });

    let a = app.clone();
    app.settings_win.on_theme_toggled(move |solid| {
        {
            let mut st = a.state.borrow_mut();
            st.settings.theme = if solid {
                "solid".into()
            } else {
                "acrylic".into()
            };
        }
        persist_settings(&a);
        set_theme(&a, solid);
    });

    let a = app.clone();
    app.settings_win.on_sort_toggled(move |on| {
        a.state.borrow_mut().settings.sort_by_use = on;
        persist_settings(&a);
        refresh_panel(&a);
    });

    let a = app.clone();
    app.settings_win.on_autostart_toggled(move |on| {
        let result = autostart_handle().and_then(|al| {
            if on {
                al.enable().map_err(|e| e.to_string())
            } else {
                al.disable().map_err(|e| e.to_string())
            }
        });
        if let Err(e) = result {
            a.settings_win.set_autostart_on(!on);
            set_data_msg(&a, &format!("⚠ 开机自启设置失败:{e}"), true);
        }
    });

    let a = app.clone();
    app.settings_win.on_pick_dir(move || {
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("选择宠物目录")
            .pick_folder()
        {
            {
                let mut st = a.state.borrow_mut();
                st.settings.custom_pet_dir = Some(dir.to_string_lossy().to_string());
            }
            persist_settings(&a);
            refresh_pets(&a);
            refresh_settings(&a);
        }
    });

    let a = app.clone();
    app.settings_win.on_do_export(move || {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("导出常用语")
            .set_file_name("petphrase-phrases.json")
            .add_filter("JSON", &["json"])
            .save_file()
        {
            // 导内存态而非磁盘态:保存持续失败时,导出是用户抢救新数据的手段
            let result = {
                let st = a.state.borrow();
                storage::export_phrases(&st.data, &path)
            };
            match result {
                Ok(()) => set_data_msg(&a, "已导出 ✓", false),
                Err(e) => set_data_msg(&a, &format!("⚠ 导出失败:{e}"), true),
            }
        }
    });

    // 导入 = 整体替换,走确认框:读入校验 → 显示替换规模 → 确认后快照+覆盖
    let a = app.clone();
    app.settings_win.on_do_import(move || {
        let Some(path) = rfd::FileDialog::new()
            .set_title("导入常用语")
            .add_filter("JSON", &["json"])
            .pick_file()
        else {
            return;
        };
        let data = match storage::read_import(&path) {
            Ok(d) => d,
            Err(e) => {
                set_data_msg(&a, &format!("⚠ 导入失败:{e}"), true);
                return;
            }
        };
        let (cur_g, cur_p) = {
            let st = a.state.borrow();
            (
                st.data.groups.len(),
                st.data.groups.iter().map(|g| g.phrases.len()).sum::<usize>(),
            )
        };
        let new_p: usize = data.groups.iter().map(|g| g.phrases.len()).sum();
        let msg = format!(
            "将用文件内容(共 {} 组 {} 条)替换当前全部常用语({} 组 {} 条)。\n当前数据会先备份为 phrases.pre-import.json。",
            data.groups.len(),
            new_p,
            cur_g,
            cur_p
        );
        a.state.borrow_mut().confirm_action = Some(ConfirmAction::ImportReplace(data));
        a.settings_win.set_confirm_kind(1);
        a.settings_win.set_confirm_action_label("替换导入".into());
        a.settings_win.set_confirm_title("导入并替换".into());
        a.settings_win.set_confirm_msg(msg.into());
        a.settings_win.set_confirm_visible(true);
    });

    // ---- 关于与更新 ----
    app.settings_win
        .set_app_version(updater::CURRENT_VERSION.into());
    app.settings_win
        .set_auto_check_on(app.state.borrow().settings.auto_check_update);
    app.settings_win
        .set_sort_by_use(app.state.borrow().settings.sort_by_use);

    let a = app.clone();
    app.settings_win
        .on_check_update(move || start_update_check(&a, true));

    let a = app.clone();
    app.settings_win
        .on_do_update(move || start_update_install(&a));

    let a = app.clone();
    app.settings_win.on_auto_check_toggled(move |on| {
        a.state.borrow_mut().settings.auto_check_update = on;
        persist_settings(&a);
    });

    // 初始自启状态
    if let Ok(al) = autostart_handle() {
        app.settings_win
            .set_autostart_on(al.is_enabled().unwrap_or(false));
    }
}

/* ================= 一键更新 ================= */

/// 后台查最新 release;manual = 手动触发(结果无论好坏都提示,自动检查只在发现新版时出声)
fn start_update_check(app: &Rc<App>, manual: bool) {
    {
        let st = app.state.borrow();
        // 已发现新版则 UI 已是「立即更新」态,重查无意义
        if st.update_busy || st.update.is_some() {
            return;
        }
    }
    app.state.borrow_mut().update_busy = true;
    app.settings_win.set_update_busy(true);
    if manual {
        app.settings_win.set_update_msg("正在检查更新…".into());
        app.settings_win.set_update_msg_error(false);
    }
    std::thread::spawn(move || {
        let result = updater::fetch_latest(updater::CURRENT_VERSION);
        let _ = slint::invoke_from_event_loop(move || {
            with_app(|a| {
                a.state.borrow_mut().update_busy = false;
                a.settings_win.set_update_busy(false);
                match result {
                    Ok(Some(u)) => {
                        a.settings_win
                            .set_update_msg(format!("发现新版 v{}", u.version).into());
                        a.settings_win.set_update_available(true);
                        if let Some(item) = &a.state.borrow().update_menu {
                            item.set_text(format!("升级到 v{}", u.version));
                        }
                        a.state.borrow_mut().update = Some(u);
                    }
                    Ok(None) => {
                        if manual {
                            a.settings_win.set_update_msg("已是最新版本 ✓".into());
                            a.settings_win.set_update_msg_error(false);
                        }
                    }
                    Err(e) => {
                        if manual {
                            a.settings_win
                                .set_update_msg(format!("检查失败:{e}").into());
                            a.settings_win.set_update_msg_error(true);
                        }
                    }
                }
            });
        });
    });
}

/// 下载+校验成功后:落盘桌宠位置 → 起静默安装器 → 退出让路(装完由安装器自启新版)
fn start_update_install(app: &Rc<App>) {
    let update = {
        let st = app.state.borrow();
        if st.update_busy {
            return;
        }
        match &st.update {
            Some(u) => u.clone(),
            None => return,
        }
    };
    app.state.borrow_mut().update_busy = true;
    app.settings_win.set_update_busy(true);
    app.settings_win
        .set_update_msg(format!("正在下载 v{}…", update.version).into());
    app.settings_win.set_update_msg_error(false);
    std::thread::spawn(move || {
        let result = updater::download_and_verify(&update);
        let _ = slint::invoke_from_event_loop(move || {
            with_app(|a| {
                let path = match &result {
                    Ok(p) => p,
                    Err(e) => {
                        a.state.borrow_mut().update_busy = false;
                        a.settings_win.set_update_busy(false);
                        a.settings_win
                            .set_update_msg(format!("更新失败:{e}").into());
                        a.settings_win.set_update_msg_error(true);
                        return;
                    }
                };
                // 与托盘退出同款:位置保存有 500ms 去抖,退出前无条件落一次盘
                let pos = a.pet.window().position();
                a.state.borrow_mut().settings.pet_pos = Some((pos.x, pos.y));
                persist_settings(a);
                match updater::launch_installer(path) {
                    Ok(()) => {
                        let _ = slint::quit_event_loop();
                    }
                    Err(e) => {
                        a.state.borrow_mut().update_busy = false;
                        a.settings_win.set_update_busy(false);
                        a.settings_win.set_update_msg(e.into());
                        a.settings_win.set_update_msg_error(true);
                    }
                }
            });
        });
    });
}

fn autostart_handle() -> Result<auto_launch::AutoLaunch, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    auto_launch::AutoLaunchBuilder::new()
        .set_app_name("PetPhrase")
        .set_app_path(&exe.to_string_lossy())
        .build()
        .map_err(|e| e.to_string())
}

/* ================= 托盘 ================= */

fn setup_tray(app: &Rc<App>) -> Result<tray_icon::TrayIcon, Box<dyn std::error::Error>> {
    use tray_icon::menu::{Menu, MenuItem};

    let icon_png = include_bytes!("../assets/icon.png");
    let rgba = image::load_from_memory(icon_png)?.into_rgba8();
    let (w, h) = rgba.dimensions();
    let icon = tray_icon::Icon::from_rgba(rgba.into_raw(), w, h)?;

    let toggle = MenuItem::new("显示/隐藏宠物", true, None);
    let settings_item = MenuItem::new("设置", true, None);
    let update_item = MenuItem::new("检查更新", true, None);
    let quit = MenuItem::new("退出", true, None);
    let menu = Menu::new();
    menu.append(&toggle)?;
    menu.append(&settings_item)?;
    menu.append(&update_item)?;
    menu.append(&quit)?;

    let tray = tray_icon::TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip("PetPhrase")
        .with_menu(Box::new(menu))
        .build()?;

    let (toggle_id, settings_id, quit_id) = (
        toggle.id().clone(),
        settings_item.id().clone(),
        quit.id().clone(),
    );
    let update_id = update_item.id().clone();
    app.state.borrow_mut().update_menu = Some(update_item);
    let a = app.clone();
    let wake_path = wake_signal_path();
    let poll = Box::leak(Box::new(slint::Timer::default()));
    poll.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(150),
        move || {
            // 二实例唤醒:用户双击了程序图标 = 想看到宠,找回并打招呼
            if wake_path.exists() {
                let _ = std::fs::remove_file(&wake_path);
                recover_pet(&a);
            }
            // 托盘图标左键单击 = 显示/找回宠(桌面软件惯例;菜单只挂在右键)
            while let Ok(ev) = tray_icon::TrayIconEvent::receiver().try_recv() {
                if let tray_icon::TrayIconEvent::Click {
                    button: tray_icon::MouseButton::Left,
                    button_state: tray_icon::MouseButtonState::Up,
                    ..
                } = ev
                {
                    recover_pet(&a);
                }
            }
            while let Ok(ev) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                if ev.id == toggle_id {
                    if a.pet.window().is_visible() {
                        let _ = a.pet.window().hide();
                        hide_panel(&a);
                    } else {
                        recover_pet(&a); // 顺带钳回屏内+重申置顶,屏外宠靠这里找回
                    }
                } else if ev.id == settings_id {
                    open_settings(&a);
                } else if ev.id == update_id {
                    // 打开设置「外观与行为」页给进度/结果反馈
                    open_settings(&a);
                    a.settings_win.set_page(1);
                    if a.state.borrow().update.is_some() {
                        // 已发现新版:菜单项此时文案是「升级到 vX.Y.Z」,点击即下载安装
                        start_update_install(&a);
                    } else {
                        start_update_check(&a, true);
                    }
                } else if ev.id == quit_id {
                    // 拖宠位置保存有 500ms 去抖,退出前无条件落一次盘防丢(隐藏窗口的 position() 依然有效)
                    let pos = a.pet.window().position();
                    a.state.borrow_mut().settings.pet_pos = Some((pos.x, pos.y));
                    persist_settings(&a);
                    let _ = slint::quit_event_loop();
                }
            }
        },
    );

    Ok(tray)
}
