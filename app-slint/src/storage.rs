use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Phrase {
    pub id: String,
    pub text: String,
    /// 复制次数(「按使用频率排序」依据);旧数据缺省 0
    #[serde(default)]
    pub use_count: u32,
}

impl Phrase {
    pub fn new(id: String, text: String) -> Self {
        Phrase {
            id,
            text,
            use_count: 0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Group {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub phrases: Vec<Phrase>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PhraseData {
    pub groups: Vec<Group>,
}

impl Default for PhraseData {
    fn default() -> Self {
        PhraseData {
            groups: vec![Group {
                id: "g-default".into(),
                name: "常用".into(),
                icon: Some("star".into()),
                phrases: vec![
                    Phrase::new("p-1".into(), "收到,马上处理".into()),
                    Phrase::new("p-2".into(), "好的,没问题".into()),
                    Phrase::new(
                        "p-3".into(),
                        "您好,感谢您的反馈,我们已经记录了这个问题,会尽快给您答复。".into(),
                    ),
                ],
            }],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Settings {
    #[serde(default = "default_pet_id")]
    pub pet_id: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub pet_pos: Option<(i32, i32)>,
    #[serde(default)]
    pub last_group: Option<String>,
    #[serde(default)]
    pub custom_pet_dir: Option<String>,
    /// 桌宠渲染缩放(1.0 = 原始 192×208)
    #[serde(default = "default_pet_scale")]
    pub pet_scale: f32,
    /// 启动后自动检查更新(仅版本号查询,失败静默)
    #[serde(default = "default_true")]
    pub auto_check_update: bool,
    /// 面板内短语按 use_count 降序排(仅影响面板展示;设置页保持手动序)
    #[serde(default)]
    pub sort_by_use: bool,
    /// 面板高度(逻辑像素;宽度固定 300 不可调)
    #[serde(default = "default_panel_h")]
    pub panel_h: f32,
}

fn default_pet_id() -> String {
    "default".into()
}
fn default_theme() -> String {
    "acrylic".into()
}
fn default_pet_scale() -> f32 {
    1.0
}
fn default_true() -> bool {
    true
}
fn default_panel_h() -> f32 {
    400.0
}

/// 面板高度可调范围(拖拽手柄与载入时都按此钳制)
pub const PANEL_H_MIN: f32 = 320.0;
pub const PANEL_H_MAX: f32 = 720.0;

impl Default for Settings {
    fn default() -> Self {
        Settings {
            pet_id: default_pet_id(),
            theme: default_theme(),
            pet_pos: None,
            last_group: None,
            custom_pet_dir: None,
            pet_scale: default_pet_scale(),
            auto_check_update: true,
            sort_by_use: false,
            panel_h: default_panel_h(),
        }
    }
}

const PHRASES_FILE: &str = "phrases.json";
const BACKUP_FILE: &str = "phrases.backup.json";
const SETTINGS_FILE: &str = "settings.json";
const SETTINGS_BACKUP_FILE: &str = "settings.backup.json";

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// 主文件读不出时回退 .old(换入被打断留下的上一版)
fn read_json_with_old<T: DeserializeOwned>(path: &Path) -> Option<T> {
    read_json(path).or_else(|| read_json(&path.with_extension("old")))
}

/// 写临时文件后换入:旧文件先挪 .old,新文件 rename 就位,失败回滚 .old。
/// (Windows rename 不覆盖已存在目标,直接删旧再 rename 会留下"旧已删、新未就位"的丢文件窗口;
/// 换入方案保证任意时刻主文件或 .old 至少一份完好,读取侧配合 .old 回退。)
pub fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let result = fs::write(&tmp, contents).and_then(|_| swap_in(path, &tmp));
    if result.is_err() {
        let _ = fs::remove_file(&tmp); // 写入或换入失败都不留 .tmp 垃圾
    }
    result
}

fn swap_in(path: &Path, tmp: &Path) -> io::Result<()> {
    if !path.exists() {
        return fs::rename(tmp, path);
    }
    let old = path.with_extension("old");
    let _ = fs::remove_file(&old);
    fs::rename(path, &old)?;
    if let Err(e) = fs::rename(tmp, path) {
        let _ = fs::rename(&old, path); // 回滚
        return Err(e);
    }
    let _ = fs::remove_file(&old);
    Ok(())
}

/// 主文件 → .old → 备份 → 默认;从备份恢复时回写主文件。
pub fn load_phrases(dir: &Path) -> PhraseData {
    let main = dir.join(PHRASES_FILE);
    if let Some(data) = read_json_with_old::<PhraseData>(&main) {
        return data;
    }
    if let Some(data) = read_json_with_old::<PhraseData>(&dir.join(BACKUP_FILE)) {
        let _ = save_phrases(dir, &data); // 恢复主文件
        return data;
    }
    PhraseData::default()
}

pub fn save_phrases(dir: &Path, data: &PhraseData) -> io::Result<()> {
    let json = serde_json::to_string_pretty(data).map_err(io::Error::other)?;
    atomic_write(&dir.join(PHRASES_FILE), &json)
}

/// 主文件语义可读才写备份;备份本身也走原子换入,防拷贝中断毁掉上一份好备份。
fn backup_json<T: DeserializeOwned>(main: &Path, backup: &Path) {
    let Ok(text) = fs::read_to_string(main) else {
        return;
    };
    if serde_json::from_str::<T>(&text).is_ok() {
        let _ = atomic_write(backup, &text);
    }
}

/// 启动时调用:当前主文件可读则存为备份。
pub fn backup_phrases(dir: &Path) {
    backup_json::<PhraseData>(&dir.join(PHRASES_FILE), &dir.join(BACKUP_FILE));
}

/// 启动时调用:当前设置可读则存为备份(与 phrases 同等保护)。
pub fn backup_settings(dir: &Path) {
    backup_json::<Settings>(&dir.join(SETTINGS_FILE), &dir.join(SETTINGS_BACKUP_FILE));
}

/// 主文件 → .old → 备份 → 默认;从备份恢复时回写主文件。
pub fn load_settings(dir: &Path) -> Settings {
    let main = dir.join(SETTINGS_FILE);
    if let Some(s) = read_json_with_old::<Settings>(&main) {
        return s;
    }
    if let Some(s) = read_json_with_old::<Settings>(&dir.join(SETTINGS_BACKUP_FILE)) {
        let _ = save_settings(dir, &s);
        return s;
    }
    Settings::default()
}

pub fn save_settings(dir: &Path, s: &Settings) -> io::Result<()> {
    let json = serde_json::to_string_pretty(s).map_err(io::Error::other)?;
    atomic_write(&dir.join(SETTINGS_FILE), &json)
}

/// 导出内存数据到用户指定路径。同目录临时文件写完再 rename 换入:
/// 失败不碰旧导出;不用 .old 换入方案(会误删用户同名 .old);
/// Windows 下 std 的 rename 带 MOVEFILE_REPLACE_EXISTING,可直接覆盖已有目标。
pub fn export_phrases(data: &PhraseData, dest: &Path) -> io::Result<()> {
    let json = serde_json::to_string_pretty(data).map_err(io::Error::other)?;
    let tmp = dest.with_extension("petphrase-export-tmp");
    let result = fs::write(&tmp, &json).and_then(|_| fs::rename(&tmp, dest));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub const MAX_GROUPS: usize = 500;
pub const MAX_PHRASES_PER_GROUP: usize = 5000;
pub const MAX_TEXT_CHARS: usize = 10_000;
/// 导入文件大小上限:读入内存前先查 metadata,防异常大文件撑爆内存
pub const MAX_IMPORT_BYTES: u64 = 20 * 1024 * 1024;

/// 导入数据的语义校验+修补:规模/超长文本报错;空名、空短语、缺失/重复 id 就地修补。
fn sanitize_import(data: &mut PhraseData) -> Result<(), String> {
    if data.groups.len() > MAX_GROUPS {
        return Err(format!("分组过多({},上限 {MAX_GROUPS})", data.groups.len()));
    }
    let mut ids = std::collections::HashSet::new();
    let mut n = 0usize;
    for g in &mut data.groups {
        let name = g.name.trim();
        g.name = if name.is_empty() {
            "未命名".into()
        } else {
            name.to_string()
        };
        while g.id.trim().is_empty() || !ids.insert(g.id.clone()) {
            n += 1;
            g.id = format!("g-imp{n}");
        }
        if g.phrases.len() > MAX_PHRASES_PER_GROUP {
            return Err(format!(
                "分组「{}」短语过多({},上限 {MAX_PHRASES_PER_GROUP})",
                g.name,
                g.phrases.len()
            ));
        }
        if let Some(p) = g
            .phrases
            .iter()
            .find(|p| p.text.chars().count() > MAX_TEXT_CHARS)
        {
            return Err(format!(
                "存在超长短语({} 字,上限 {MAX_TEXT_CHARS})",
                p.text.chars().count()
            ));
        }
        g.phrases.retain(|p| !p.text.trim().is_empty());
        for p in &mut g.phrases {
            while p.id.trim().is_empty() || !ids.insert(p.id.clone()) {
                n += 1;
                p.id = format!("p-imp{n}");
            }
        }
    }
    Ok(())
}

/// 读取并校验外部导入文件,不落盘 —— 覆盖前由调用方弹确认、写快照。
pub fn read_import(src: &Path) -> io::Result<PhraseData> {
    let size = fs::metadata(src)?.len();
    if size > MAX_IMPORT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "文件过大({}MB,上限 {}MB)",
                size >> 20,
                MAX_IMPORT_BYTES >> 20
            ),
        ));
    }
    let text = fs::read_to_string(src)?;
    let mut data: PhraseData = serde_json::from_str(&text)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("文件格式不正确: {e}")))?;
    sanitize_import(&mut data).map_err(|m| io::Error::new(io::ErrorKind::InvalidData, m))?;
    Ok(data)
}

const PRE_IMPORT_FILE: &str = "phrases.pre-import.json";

/// 导入覆盖前把当前内存数据存一份快照,给误导入留退路。
pub fn snapshot_before_import(dir: &Path, data: &PhraseData) -> io::Result<()> {
    let json = serde_json::to_string_pretty(data).map_err(io::Error::other)?;
    atomic_write(&dir.join(PRE_IMPORT_FILE), &json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn save_then_load_roundtrip() {
        let dir = tempdir().unwrap();
        let mut data = PhraseData::default();
        data.groups[0].phrases.push(Phrase::new("x".into(), "测试".into()));
        save_phrases(dir.path(), &data).unwrap();
        assert_eq!(load_phrases(dir.path()), data);
    }

    #[test]
    fn load_missing_returns_default_with_sample_group() {
        let dir = tempdir().unwrap();
        let data = load_phrases(dir.path());
        assert_eq!(data.groups.len(), 1);
        assert_eq!(data.groups[0].name, "常用");
        assert!(!data.groups[0].phrases.is_empty());
    }

    #[test]
    fn corrupt_json_recovers_from_backup_and_rewrites_main() {
        let dir = tempdir().unwrap();
        let data = PhraseData::default();
        save_phrases(dir.path(), &data).unwrap();
        backup_phrases(dir.path());
        fs::write(dir.path().join("phrases.json"), "{broken!!").unwrap();
        assert_eq!(load_phrases(dir.path()), data);
        // 主文件已被修复
        let repaired = fs::read_to_string(dir.path().join("phrases.json")).unwrap();
        assert!(serde_json::from_str::<PhraseData>(&repaired).is_ok());
    }

    #[test]
    fn atomic_write_leaves_no_tmp_or_old() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("phrases.json");
        atomic_write(&target, "{}").unwrap();
        atomic_write(&target, "{\"groups\":[]}").unwrap(); // 覆盖已有文件
        assert!(!target.with_extension("tmp").exists());
        assert!(!target.with_extension("old").exists());
        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"groups\":[]}");
    }

    #[test]
    fn load_falls_back_to_old_when_main_corrupt() {
        let dir = tempdir().unwrap();
        let data = PhraseData::default();
        // 模拟换入中断:.old 完好、主文件损坏
        let json = serde_json::to_string(&data).unwrap();
        fs::write(dir.path().join("phrases.old"), &json).unwrap();
        fs::write(dir.path().join("phrases.json"), "{broken").unwrap();
        assert_eq!(load_phrases(dir.path()), data);
    }

    #[test]
    fn load_falls_back_to_backup_old_when_backup_swap_interrupted() {
        let dir = tempdir().unwrap();
        let data = PhraseData::default();
        // 模拟备份换入中断:主文件缺失,备份只剩 .old
        let json = serde_json::to_string(&data).unwrap();
        fs::write(dir.path().join("phrases.backup.old"), &json).unwrap();
        assert_eq!(load_phrases(dir.path()), data);
    }

    #[test]
    fn settings_recover_from_backup_when_main_corrupt() {
        let dir = tempdir().unwrap();
        let s = Settings {
            theme: "solid".into(),
            ..Settings::default()
        };
        save_settings(dir.path(), &s).unwrap();
        backup_settings(dir.path());
        fs::write(dir.path().join("settings.json"), "{broken").unwrap();
        assert_eq!(load_settings(dir.path()), s);
        // 主文件已被修复
        assert!(read_json::<Settings>(&dir.path().join("settings.json")).is_some());
    }

    #[test]
    fn import_sanitizes_dup_ids_empty_names_and_drops_blank_phrases() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("in.json");
        fs::write(
            &src,
            r#"{"groups":[
                {"id":"a","name":"  ","phrases":[{"id":"x","text":"one"},{"id":"x","text":"   "}]},
                {"id":"a","name":"B","phrases":[{"id":"","text":"two"}]}
            ]}"#,
        )
        .unwrap();
        let data = read_import(&src).unwrap();
        assert_eq!(data.groups[0].name, "未命名");
        assert_eq!(data.groups[0].phrases.len(), 1, "空白短语被丢弃");
        assert_ne!(data.groups[0].id, data.groups[1].id, "重复分组 id 被重建");
        assert!(!data.groups[1].phrases[0].id.is_empty(), "空短语 id 被补齐");
    }

    #[test]
    fn import_rejects_oversized_data() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("in.json");
        let huge = "x".repeat(10_001);
        fs::write(
            &src,
            format!(
                r#"{{"groups":[{{"id":"a","name":"A","phrases":[{{"id":"p","text":"{huge}"}}]}}]}}"#
            ),
        )
        .unwrap();
        let err = read_import(&src).unwrap_err();
        assert!(err.to_string().contains("超长"));
    }

    #[test]
    fn import_rejects_oversized_file_before_reading() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("huge.json");
        let f = fs::File::create(&src).unwrap();
        f.set_len(MAX_IMPORT_BYTES + 1).unwrap(); // 稀疏文件,不真占盘
        let err = read_import(&src).unwrap_err();
        assert!(err.to_string().contains("过大"));
    }

    #[test]
    fn snapshot_before_import_writes_readable_copy() {
        let dir = tempdir().unwrap();
        let data = PhraseData::default();
        snapshot_before_import(dir.path(), &data).unwrap();
        let text = fs::read_to_string(dir.path().join("phrases.pre-import.json")).unwrap();
        assert_eq!(serde_json::from_str::<PhraseData>(&text).unwrap(), data);
    }

    #[test]
    fn settings_roundtrip_and_default() {
        let dir = tempdir().unwrap();
        let s = load_settings(dir.path());
        assert_eq!(s.pet_id, "default");
        assert_eq!(s.theme, "acrylic");
        assert_eq!(s.pet_scale, 1.0);
        let changed = Settings {
            pet_pos: Some((100, 200)),
            pet_scale: 0.75,
            ..s
        };
        save_settings(dir.path(), &changed).unwrap();
        assert_eq!(load_settings(dir.path()), changed);
    }

    #[test]
    fn import_rejects_invalid_json() {
        let dir = tempdir().unwrap();
        let bad = dir.path().join("bad.json");
        fs::write(&bad, "not json").unwrap();
        assert!(read_import(&bad).is_err());
    }

    #[test]
    fn export_then_import_roundtrip() {
        let dir = tempdir().unwrap();
        let data = PhraseData::default();
        let out = dir.path().join("out.json");
        export_phrases(&data, &out).unwrap();
        assert_eq!(read_import(&out).unwrap(), data);
    }

    #[test]
    fn export_overwrites_existing_file_and_leaves_no_tmp() {
        let dir = tempdir().unwrap();
        let out = dir.path().join("out.json");
        fs::write(&out, "旧导出,不该被截断成半截").unwrap();
        let data = PhraseData::default();
        export_phrases(&data, &out).unwrap();
        // rename 覆盖成功:内容是完整新 JSON,且无临时文件残留
        let text = fs::read_to_string(&out).unwrap();
        assert!(serde_json::from_str::<PhraseData>(&text).is_ok());
        assert!(!out.with_extension("petphrase-export-tmp").exists());
    }
}
