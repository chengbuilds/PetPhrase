use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct PetInfo {
    pub id: String,
    pub name: String,
    /// spritesheet 绝对路径;error 非空时为空串
    pub spritesheet: String,
    pub error: Option<String>,
    /// 包目录(删除用;损坏的包没有 spritesheet 也要能删)
    pub dir: PathBuf,
}

/// 单个宠物目录 → PetInfo。宽松校验:pet.json 可缺 name(用目录名),
/// 雪碧图优先 pet.json 的 spritesheetPath(与官方桌面端一致),再回退 spritesheet.webp/png。
/// 尺寸/网格在解码时按 anim::Atlas 推算。
fn load_pet(dir: &Path) -> Option<PetInfo> {
    if !dir.is_dir() {
        return None;
    }
    let id = dir.file_name()?.to_string_lossy().to_string();
    let mut name = id.clone();
    let mut error: Option<String> = None;
    let mut declared: Option<String> = None;

    match fs::read_to_string(dir.join("pet.json")) {
        Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(meta) => {
                // 兼容两种字段:官方素材用 displayName,规范文档用 name
                if let Some(n) = meta
                    .get("displayName")
                    .or_else(|| meta.get("name"))
                    .and_then(|v| v.as_str())
                {
                    name = n.to_string();
                }
                // 只取文件名部分:清单是外部文件,不让它指到包目录之外
                declared = meta
                    .get("spritesheetPath")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Path::new(s).file_name())
                    .map(|f| f.to_string_lossy().to_string());
            }
            Err(e) => error = Some(format!("pet.json 解析失败: {e}")),
        },
        Err(_) => error = Some("缺少 pet.json".into()),
    }

    let spritesheet = declared
        .iter()
        .map(String::as_str)
        .chain(["spritesheet.webp", "spritesheet.png"])
        .map(|f| dir.join(f))
        .find(|p| p.is_file());

    let spritesheet = match spritesheet {
        Some(p) => p.to_string_lossy().to_string(),
        None => {
            error.get_or_insert("缺少 spritesheet.webp/png".into());
            String::new()
        }
    };

    Some(PetInfo {
        id,
        name,
        spritesheet,
        error,
        dir: dir.to_path_buf(),
    })
}

/// 解码雪碧图并裁出 idle 首帧作缩略图。
/// 整张 RGBA 约 11.5MB/宠,只在此函数栈上短暂存在;返回的单帧 ~160KB,可安全常驻缓存。
pub fn load_thumb(path: &str) -> slint::Image {
    let Ok(img) = image::open(Path::new(path)) else {
        return slint::Image::default();
    };
    thumb_from_sheet(&img)
}

/// 按图集几何裁首格(等比缩放的素材格子不是 192×208)
pub fn thumb_from_sheet(img: &image::DynamicImage) -> slint::Image {
    let atlas = crate::anim::Atlas::from_size(img.width(), img.height());
    let w = atlas.cell_w.min(img.width());
    let h = atlas.cell_h.min(img.height());
    let frame = img.crop_imm(0, 0, w, h).into_rgba8();
    let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        frame.as_raw(),
        frame.width(),
        frame.height(),
    );
    slint::Image::from_rgba8(buf)
}

/// 依序扫描多个根目录,每个根目录下的一级子目录 = 一个宠物包。
/// 同 id 先到先得(内置目录优先级最高);根目录内按名排序,
/// 保证任何文件系统上(FAT/exFAT 不保序)列表顺序稳定。
pub fn scan_pets(roots: &[&Path]) -> Vec<PetInfo> {
    let mut pets: Vec<PetInfo> = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        let mut dirs: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        dirs.sort();
        for dir in dirs {
            if let Some(pet) = load_pet(&dir) {
                if !pets.iter().any(|p| p.id == pet.id) {
                    pets.push(pet);
                }
            }
        }
    }
    pets
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_pet(root: &Path, id: &str, json: Option<&str>, sheet: Option<&str>) {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        if let Some(j) = json {
            fs::write(dir.join("pet.json"), j).unwrap();
        }
        if let Some(f) = sheet {
            fs::write(dir.join(f), b"fake-image").unwrap();
        }
    }

    #[test]
    fn display_name_takes_precedence() {
        let root = tempdir().unwrap();
        make_pet(
            root.path(),
            "kun-like",
            Some(r#"{"id":"Kun-like","displayName":"Kun Like","name":"ignored"}"#),
            Some("spritesheet.webp"),
        );
        let pets = scan_pets(&[root.path()]);
        assert_eq!(pets[0].name, "Kun Like");
    }

    #[test]
    fn declared_spritesheet_path_wins_and_cannot_escape_dir() {
        let root = tempdir().unwrap();
        make_pet(
            root.path(),
            "custom",
            Some(r#"{"displayName":"C","spritesheetPath":"atlas.webp"}"#),
            Some("atlas.webp"),
        );
        fs::write(root.path().join("custom").join("spritesheet.png"), b"x").unwrap();
        let pets = scan_pets(&[root.path()]);
        assert!(pets[0].spritesheet.ends_with("atlas.webp"));

        make_pet(
            root.path(),
            "escape",
            Some(r#"{"spritesheetPath":"../custom/atlas.webp"}"#),
            None,
        );
        let pets = scan_pets(&[root.path()]);
        let esc = pets.iter().find(|p| p.id == "escape").unwrap();
        assert!(esc.error.is_some(), "../ 被截成文件名,包内不存在即报缺失");
    }

    #[test]
    fn scans_valid_pet() {
        let root = tempdir().unwrap();
        make_pet(
            root.path(),
            "kun-like",
            Some(r#"{"name":"Kun"}"#),
            Some("spritesheet.webp"),
        );
        let pets = scan_pets(&[root.path()]);
        assert_eq!(pets.len(), 1);
        assert_eq!(pets[0].name, "Kun");
        assert_eq!(pets[0].id, "kun-like");
        assert!(pets[0].error.is_none());
        assert!(pets[0].spritesheet.ends_with("spritesheet.webp"));
    }

    #[test]
    fn missing_spritesheet_yields_error() {
        let root = tempdir().unwrap();
        make_pet(root.path(), "broken", Some(r#"{"name":"X"}"#), None);
        let pets = scan_pets(&[root.path()]);
        assert_eq!(pets[0].error.as_deref(), Some("缺少 spritesheet.webp/png"));
    }

    #[test]
    fn bad_json_yields_error_with_dirname_as_name() {
        let root = tempdir().unwrap();
        make_pet(root.path(), "oops", Some("{bad"), Some("spritesheet.png"));
        let pets = scan_pets(&[root.path()]);
        assert_eq!(pets[0].name, "oops");
        assert!(pets[0].error.as_deref().unwrap().contains("解析失败"));
    }

    #[test]
    fn merges_roots_in_order_first_wins() {
        let a = tempdir().unwrap();
        let b = tempdir().unwrap();
        make_pet(
            a.path(),
            "dup",
            Some(r#"{"name":"FromA"}"#),
            Some("spritesheet.png"),
        );
        make_pet(
            b.path(),
            "dup",
            Some(r#"{"name":"FromB"}"#),
            Some("spritesheet.png"),
        );
        make_pet(
            b.path(),
            "only-b",
            Some(r#"{"name":"B"}"#),
            Some("spritesheet.png"),
        );
        let pets = scan_pets(&[a.path(), b.path()]);
        assert_eq!(pets.len(), 2);
        assert_eq!(pets.iter().find(|p| p.id == "dup").unwrap().name, "FromA");
    }

    #[test]
    fn nonexistent_root_is_skipped() {
        let pets = scan_pets(&[Path::new("Z:/no/such/dir")]);
        assert!(pets.is_empty());
    }

    #[test]
    fn thumb_crops_first_frame_from_full_sheet() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sheet.png");
        image::RgbaImage::new(1536, 1872).save(&path).unwrap();
        let thumb = load_thumb(&path.to_string_lossy());
        let size = thumb.size();
        assert_eq!((size.width, size.height), (192, 208));

        // 等比缩半的素材按格子裁,不按 192×208 硬切
        image::RgbaImage::new(768, 936).save(&path).unwrap();
        let size = load_thumb(&path.to_string_lossy()).size();
        assert_eq!((size.width, size.height), (96, 104));
    }

    #[test]
    fn thumb_on_unreadable_file_returns_default() {
        let thumb = load_thumb("Z:/no/such/sheet.webp");
        assert_eq!(thumb.size().width, 0);
    }
}
