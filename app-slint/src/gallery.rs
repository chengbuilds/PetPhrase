//! 在线宠物库:petdex 公开 manifest(免登录)浏览 + 一键安装。
//! 网络走 updater 的 curl.exe 通道(零 HTTP 依赖);全部阻塞函数只在后台线程调用。
//! 安装行为对齐官方 CLI:同时写入 ~/.petdex/pets 与 ~/.codex/pets,文件名统一为
//! pet.json + spritesheet.{webp,png}。

use crate::updater::{download, run_curl};
use std::path::{Path, PathBuf};

const MANIFEST_URL: &str = "https://petdex.dev/api/manifest/v2";
/// 只信任官方资源域(官方 CLI 同样拒绝其它 host)
const ASSET_BASE: &str = "https://assets.petdex.dev";
pub const PAGE_SIZE: usize = 15;

#[derive(Debug, Clone, PartialEq)]
pub struct RemotePet {
    pub slug: String,
    pub name: String,
    sprite_url: String,
    petjson_url: String,
}

impl RemotePet {
    /// 官方 80×80 预生成缩略图(~2.5KB),不必下整张雪碧图(~2MB)再裁
    pub fn thumb_url(&self) -> String {
        format!("{ASSET_BASE}/pets/{}/thumb.webp", self.slug)
    }
}

/// 与上游 isValidPetSlug 一致;slug 会拼进本地路径,必须挡住 ../ 之类
fn valid_slug(s: &str) -> bool {
    (1..=80).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// 解析 compact manifest v2:{assetBase, fields:[...], pets:[[...]]}。
/// 按 fields 取列而非写死下标;非法 slug / 非官方资源的条目直接丢弃
pub fn parse_manifest(text: &str) -> Result<Vec<RemotePet>, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("宠物库数据格式不正确:{e}"))?;
    let base = v["assetBase"]
        .as_str()
        .unwrap_or(ASSET_BASE)
        .trim_end_matches('/');
    if base != ASSET_BASE {
        return Err("宠物库资源地址不受信任".into());
    }
    let fields: Vec<&str> = v["fields"]
        .as_array()
        .ok_or("宠物库数据缺少 fields")?
        .iter()
        .filter_map(|f| f.as_str())
        .collect();
    let col = |name: &str| fields.iter().position(|f| *f == name);
    let (Some(slug), Some(name), Some(sprite), Some(pj)) = (
        col("slug"),
        col("displayName"),
        col("spritesheet"),
        col("petJson"),
    ) else {
        return Err("宠物库数据缺少必要字段".into());
    };
    let rows = v["pets"].as_array().ok_or("宠物库数据缺少 pets")?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            let get = |i: usize| r.get(i).and_then(|x| x.as_str());
            let slug = get(slug)?;
            let sprite = get(sprite)?;
            let pj = get(pj)?;
            if !valid_slug(slug) || sprite.contains("..") || pj.contains("..") {
                return None;
            }
            Some(RemotePet {
                slug: slug.to_string(),
                name: get(name)
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or(slug)
                    .to_string(),
                sprite_url: format!("{base}/{}", sprite.trim_start_matches('/')),
                petjson_url: format!("{base}/{}", pj.trim_start_matches('/')),
            })
        })
        .collect())
}

pub fn fetch_manifest() -> Result<Vec<RemotePet>, String> {
    let text = run_curl(&["-L", "--max-time", "30", MANIFEST_URL])
        .map_err(|_| "无法连接 petdex 宠物库".to_string())?;
    parse_manifest(&text)
}

/// 名称/slug 不区分大小写包含匹配,返回下标
pub fn search(pets: &[RemotePet], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    pets.iter()
        .enumerate()
        .filter(|(_, p)| q.is_empty() || p.name.to_lowercase().contains(&q) || p.slug.contains(&q))
        .map(|(i, _)| i)
        .collect()
}

/// 一页缩略图,一个 curl 进程 --parallel 拉完,落到磁盘缓存(已缓存的跳过)。
/// 单张失败不影响其它;返回 (slug, 本地路径) 仅含存在的文件
pub fn fetch_thumbs(pets: &[RemotePet], cache: &Path) -> Vec<(String, PathBuf)> {
    let _ = std::fs::create_dir_all(cache);
    let path_of = |p: &RemotePet| cache.join(format!("{}.webp", p.slug));
    let missing: Vec<(PathBuf, String)> = pets
        .iter()
        .filter(|p| !path_of(p).is_file())
        .map(|p| (path_of(p), p.thumb_url()))
        .collect();
    if !missing.is_empty() {
        let owned: Vec<String> = missing
            .iter()
            .flat_map(|(path, url)| {
                [
                    "-o".into(),
                    path.to_string_lossy().into_owned(),
                    url.clone(),
                ]
            })
            .collect();
        let mut args: Vec<&str> = vec!["--parallel", "--max-time", "20"];
        args.extend(owned.iter().map(String::as_str));
        let _ = run_curl(&args); // 部分失败时退出码非零,已下载的照用
    }
    pets.iter()
        .map(|p| (p.slug.clone(), path_of(p)))
        .filter(|(_, path)| path.is_file())
        .collect()
}

/// 载入缩略图并裁掉左右整列不透明纯黑的补边:上游把 192:208 的格子塞进 80×80 方图时
/// 用黑色而非透明补宽(实测每张两侧各 3px),直接显示是两条黑竖线
pub fn load_thumb(path: &Path) -> Option<slint::Image> {
    let img = image::open(path).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    // 有损 webp 压缩会在黑边里留零星杂色像素,按 95% 深色判定
    let black_col = |x: u32| {
        let dark = (0..h)
            .filter(|&y| {
                let p = img.get_pixel(x, y).0;
                p[3] > 240 && p[0] < 48 && p[1] < 48 && p[2] < 48
            })
            .count() as u32;
        dark * 20 >= h * 19
    };
    let left = (0..w).take_while(|&x| black_col(x)).count() as u32;
    let right = (0..w.saturating_sub(left))
        .take_while(|&x| black_col(w - 1 - x))
        .count() as u32;
    let frame = image::imageops::crop_imm(&img, left, 0, w - left - right, h).to_image();
    if frame.width() == 0 {
        return None;
    }
    let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        frame.as_raw(),
        frame.width(),
        frame.height(),
    );
    Some(slint::Image::from_rgba8(buf))
}

/// 下载校验后装进各目标目录:要么每个目录都装好,要么全部保持原样。
/// 调用方保证同一 slug 不并发安装(临时目录与暂存目录按 slug 命名)
pub fn install(pet: &RemotePet, targets: &[PathBuf]) -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!("petphrase-install-{}", pet.slug));
    let _ = std::fs::remove_dir_all(&tmp); // 上次进程中途退出的残留,不存在即无事
    std::fs::create_dir_all(&tmp).map_err(|e| format!("创建临时目录失败:{e}"))?;
    let result = download_verified(pet, &tmp)
        .and_then(|sheet| place(&tmp, &["pet.json", sheet], &pet.slug, targets));
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// 下到 tmp 并校验:pet.json 是 JSON、雪碧图是可识别的 petdex 图集。返回雪碧图文件名
fn download_verified(pet: &RemotePet, tmp: &Path) -> Result<&'static str, String> {
    let sheet = if pet.sprite_url.ends_with(".png") {
        "spritesheet.png"
    } else {
        "spritesheet.webp"
    };
    download(&pet.petjson_url, &tmp.join("pet.json"))?;
    download(&pet.sprite_url, &tmp.join(sheet))?;
    serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(tmp.join("pet.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|_| "pet.json 格式不正确")?;
    // 只读文件头取尺寸;非标尺寸装上也只会播错格,直接拒
    let (w, h) = image::image_dimensions(tmp.join(sheet)).map_err(|_| "雪碧图无法识别")?;
    crate::anim::Atlas::detect(w, h).ok_or("雪碧图不是 petdex 标准图集(8 列 × 9 或 11 行)")?;
    Ok(sheet)
}

/// 把 src 里的 files 整包放进每个 `<root>/<slug>`,全成或全不动:
/// 先在每个根下暂存 `.<slug>.part`(同盘,换入是 rename),全部暂存好再逐个换入;
/// 同名旧目录(如损坏的旧包)先挪成 `.<slug>.old`,成功才删。任一步失败就撤掉本次所有改动
fn place(src: &Path, files: &[&str], slug: &str, targets: &[PathBuf]) -> Result<(), String> {
    let mut log = PlaceLog::default();
    let result = place_steps(src, files, slug, targets, &mut log);
    match &result {
        Ok(()) => log.olds.iter().for_each(|(old, _)| {
            let _ = std::fs::remove_dir_all(old); // 已换入成功,旧包删不掉只是多占点盘
        }),
        Err(_) => log.undo(),
    }
    result.map_err(|e| format!("写入失败:{e}"))
}

/// place 已做的改动,失败时按此撤回
#[derive(Default)]
struct PlaceLog {
    parts: Vec<PathBuf>,
    /// (挪开的旧目录, 原位置)
    olds: Vec<(PathBuf, PathBuf)>,
    placed: Vec<PathBuf>,
}

impl PlaceLog {
    fn undo(&self) {
        // 回滚尽力而为:每步各自独立,一步失败不影响其余步骤
        for dir in self.parts.iter().chain(&self.placed) {
            let _ = std::fs::remove_dir_all(dir);
        }
        for (old, dest) in &self.olds {
            let _ = std::fs::rename(old, dest);
        }
    }
}

fn place_steps(
    src: &Path,
    files: &[&str],
    slug: &str,
    targets: &[PathBuf],
    log: &mut PlaceLog,
) -> std::io::Result<()> {
    let mut staged = Vec::new();
    for root in targets {
        let part = root.join(format!(".{slug}.part"));
        let _ = std::fs::remove_dir_all(&part); // 上次中断的残留,不存在即无事
        log.parts.push(part.clone());
        std::fs::create_dir_all(&part)?;
        for f in files {
            std::fs::copy(src.join(f), part.join(f))?;
        }
        staged.push((root, part));
    }
    for (root, part) in staged {
        let dest = root.join(slug);
        if dest.exists() {
            let old = root.join(format!(".{slug}.old"));
            let _ = std::fs::remove_dir_all(&old);
            std::fs::rename(&dest, &old)?;
            log.olds.push((old, dest.clone()));
        }
        std::fs::rename(&part, &dest)?;
        log.parts.retain(|p| p != &part);
        log.placed.push(dest);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"v":2,"assetBase":"https://assets.petdex.dev",
        "fields":["slug","displayName","kind","submittedBy","spritesheet","petJson","zip","spriteVersionNumber"],
        "pets":[
          ["homelander","Homelander","character","x","pets/homelander-1/sprite.webp","pets/homelander-1/petjson.json","z",1],
          ["bad/../slug","Evil","c","x","pets/a/sprite.webp","pets/a/petjson.json","z",1],
          ["png-pet","","c","x","pets/p/sprite.png","pets/p/petjson.json","z",2]
        ]}"#;

    #[test]
    fn manifest_parses_by_field_names_and_drops_unsafe_rows() {
        let pets = parse_manifest(SAMPLE).unwrap();
        assert_eq!(pets.len(), 2, "非法 slug 被丢弃");
        assert_eq!(pets[0].name, "Homelander");
        assert_eq!(
            pets[0].sprite_url,
            "https://assets.petdex.dev/pets/homelander-1/sprite.webp"
        );
        assert_eq!(
            pets[0].thumb_url(),
            "https://assets.petdex.dev/pets/homelander/thumb.webp"
        );
        assert_eq!(pets[1].name, "png-pet", "空显示名回落 slug");
    }

    #[test]
    fn manifest_rejects_untrusted_base() {
        let evil = SAMPLE.replace("https://assets.petdex.dev", "https://evil.example");
        assert!(parse_manifest(&evil).is_err());
    }

    #[test]
    fn search_matches_name_and_slug_case_insensitive() {
        let pets = parse_manifest(SAMPLE).unwrap();
        assert_eq!(search(&pets, "HOME"), vec![0]);
        assert_eq!(search(&pets, "png"), vec![1]);
        assert_eq!(search(&pets, "  ").len(), 2);
    }

    #[test]
    fn thumb_trims_black_side_bars() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.png");
        let mut img = image::RgbaImage::new(80, 80); // 全透明
        for y in 0..80 {
            for x in (0..3).chain(77..80) {
                img.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
            }
        }
        img.put_pixel(1, 50, image::Rgba([20, 0, 6, 255])); // 压缩杂色
        img.put_pixel(40, 40, image::Rgba([0, 0, 0, 255])); // 内容里的黑像素不受影响
        img.save(&path).unwrap();
        let t = load_thumb(&path).unwrap();
        assert_eq!((t.size().width, t.size().height), (74, 80));
    }

    /// 装好的包:两个文件齐全;根下不留暂存/旧包目录
    fn assert_placed(root: &Path) {
        assert!(root.join("p/pet.json").is_file() && root.join("p/s.webp").is_file());
        let names: Vec<_> = std::fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["p"], "不留 .part/.old");
    }

    fn src_files() -> tempfile::TempDir {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("pet.json"), "{}").unwrap();
        std::fs::write(src.path().join("s.webp"), "img").unwrap();
        src
    }

    #[test]
    fn place_installs_into_every_root_and_replaces_broken_old_dir() {
        let src = src_files();
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        std::fs::create_dir_all(b.path().join("p")).unwrap(); // 损坏的旧包:只有个空目录
        let roots = [a.path().to_path_buf(), b.path().to_path_buf()];
        place(src.path(), &["pet.json", "s.webp"], "p", &roots).unwrap();
        assert_placed(a.path());
        assert_placed(b.path());
    }

    #[test]
    fn place_failing_while_staging_leaves_every_root_untouched() {
        let src = src_files();
        let a = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(a.path().join("p")).unwrap();
        std::fs::write(a.path().join("p/keep"), "旧").unwrap();
        let not_dir = a.path().join("plain-file");
        std::fs::write(&not_dir, "").unwrap(); // 第二个根是文件:暂存必然失败
        let roots = [a.path().to_path_buf(), not_dir];
        assert!(place(src.path(), &["pet.json", "s.webp"], "p", &roots).is_err());
        assert!(a.path().join("p/keep").is_file(), "旧包原样保留");
        assert!(!a.path().join(".p.part").exists());
    }

    #[test]
    fn place_failing_while_swapping_rolls_back_roots_already_swapped() {
        use std::os::windows::fs::OpenOptionsExt;
        let src = src_files();
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        std::fs::create_dir_all(b.path().join("p")).unwrap();
        // 不共享地占住 b 的旧包里的文件:旧包挪不走,换入在第二个根失败
        let _lock = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .share_mode(0)
            .open(b.path().join("p/busy"))
            .unwrap();
        let roots = [a.path().to_path_buf(), b.path().to_path_buf()];
        assert!(place(src.path(), &["pet.json", "s.webp"], "p", &roots).is_err());
        assert!(!a.path().join("p").exists(), "已换入的根被撤回");
        assert!(!a.path().join(".p.part").exists() && !b.path().join(".p.part").exists());
        assert!(b.path().join("p/busy").is_file(), "旧包原样保留");
    }

    #[test]
    fn slug_validation_matches_upstream() {
        assert!(valid_slug("kun-like"));
        assert!(!valid_slug("Kun"));
        assert!(!valid_slug("../x"));
        assert!(!valid_slug(""));
    }
}
