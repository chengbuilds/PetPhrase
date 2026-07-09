//! 一键更新:curl.exe 查 GitHub 最新版 → 比版本 → 下载 setup.exe → certutil 校验 sha256 → 静默重装。
//! 不引 HTTP/哈希库:curl.exe(Win10 1803+ 自带)与 certutil 都是系统组件,零二进制增量。
//! 版本检查走 releases/latest 网页 302 跳转的 Location 头,而非 api.github.com:
//! 匿名 API 按出口 IP 限额 60 次/时,共享代理/公司 NAT 用户常态 403(实测踩中);网页跳转无此限制。
//! 本模块全部函数都是阻塞调用,只能在后台线程跑;结果回 UI 走 slint::invoke_from_event_loop。

use std::path::Path;
use std::process::Command;

const RELEASES_LATEST_URL: &str = "https://github.com/chengbuilds/PetPhrase/releases/latest";
const DOWNLOAD_BASE: &str = "https://github.com/chengbuilds/PetPhrase/releases/download";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CHECK_TIMEOUT_SECS: &str = "10";
const DOWNLOAD_TIMEOUT_SECS: &str = "300";

#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    /// 不带 v 前缀,如 "0.6.0"
    pub version: String,
    pub setup_url: String,
    /// 同名 .sha256 资产(发版脚本上传);拉取失败跳过校验(HTTPS + NSIS 自带 CRC 兜底)
    pub sha256_url: String,
}

/// "v0.6.0" / "0.6.0" → [0,6,0];非数字段计 0,长度不齐按字典序(足够 X.Y.Z)
fn parse_version(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split('.')
        .map(|s| s.trim().parse().unwrap_or(0))
        .collect()
}

pub fn is_newer(remote: &str, current: &str) -> bool {
    parse_version(remote) > parse_version(current)
}

/// 从 releases/latest 响应头取 302 Location 里的 tag(…/releases/tag/vX.Y.Z)
pub fn parse_redirect_tag(headers: &str) -> Option<String> {
    headers.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        if !k.trim().eq_ignore_ascii_case("location") {
            return None;
        }
        let tag = v.trim().rsplit_once("/tag/")?.1.trim();
        (!tag.is_empty()).then(|| tag.to_string())
    })
}

/// tag → 新版描述;下载地址按发版命名约定构造(installer.nsi OutFile,build.rs 保证版本一致)
pub fn update_from_tag(tag: &str, current: &str) -> Option<Update> {
    if !is_newer(tag, current) {
        return None;
    }
    let version = tag.trim_start_matches(['v', 'V']).to_string();
    let name = format!("PetPhrase_{version}_x64-setup.exe");
    Some(Update {
        setup_url: format!("{DOWNLOAD_BASE}/{tag}/{name}"),
        sha256_url: format!("{DOWNLOAD_BASE}/{tag}/{name}.sha256"),
        version,
    })
}

/// .sha256 文件里取第一个 64 位十六进制 token(兼容 "hash *filename" 格式)
pub fn parse_sha256(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|t| t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|s| s.to_ascii_lowercase())
}

/// 子进程不弹黑框(CREATE_NO_WINDOW)
fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

fn run_curl(args: &[&str]) -> Result<String, String> {
    let out = no_window(
        Command::new("curl.exe")
            .args(["-fsS", "-H", "User-Agent: PetPhrase"])
            .args(args),
    )
    .output()
    .map_err(|e| format!("无法启动 curl:{e}"))?;
    if !out.status.success() {
        return Err("网络请求失败".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 查最新 release 并与当前版本比对;Ok(None) = 已是最新
pub fn fetch_latest(current: &str) -> Result<Option<Update>, String> {
    // -I 只拿响应头;GitHub 对 releases/latest 回 302 → Location 含 tag。
    // -L 跟随跳转链并输出每一跳的头:仓库改名/迁移会先插一跳 301(Location 不含 /tag/),
    // 不跟随就取不到 tag——2026-07 用户名改名把线上 0.6.0 的更新检查打断过,教训。
    let headers = run_curl(&[
        "-I",
        "-L",
        "--max-time",
        CHECK_TIMEOUT_SECS,
        RELEASES_LATEST_URL,
    ])?;
    let tag = parse_redirect_tag(&headers).ok_or("响应里没有版本信息")?;
    Ok(update_from_tag(&tag, current))
}

pub fn download(url: &str, dest: &Path) -> Result<(), String> {
    run_curl(&[
        "-L",
        "--max-time",
        DOWNLOAD_TIMEOUT_SECS,
        "-o",
        &dest.to_string_lossy(),
        url,
    ])
    .map(|_| ())
    .map_err(|_| "下载失败".into())
}

/// certutil 算文件 sha256(输出中找 64 位十六进制行)
pub fn sha256_of(path: &Path) -> Result<String, String> {
    let out = no_window(Command::new("certutil.exe").args([
        "-hashfile",
        &path.to_string_lossy(),
        "SHA256",
    ]))
    .output()
    .map_err(|e| format!("无法启动 certutil:{e}"))?;
    if !out.status.success() {
        return Err("计算文件校验和失败".into());
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    parse_sha256(&text).ok_or_else(|| "certutil 输出无法解析".into())
}

/// 下载安装包;.sha256 拉得到就强校验,拉不到(老版本 release 没传)跳过
pub fn download_and_verify(update: &Update) -> Result<std::path::PathBuf, String> {
    let dest = std::env::temp_dir().join(format!("PetPhrase_{}_x64-setup.exe", update.version));
    download(&update.setup_url, &dest)?;
    if let Ok(text) = run_curl(&["-L", "--max-time", CHECK_TIMEOUT_SECS, &update.sha256_url]) {
        let expected = parse_sha256(&text).ok_or("sha256 文件格式不正确")?;
        let actual = sha256_of(&dest)?;
        if actual != expected {
            let _ = std::fs::remove_file(&dest);
            return Err("安装包校验失败(sha256 不匹配)".into());
        }
    }
    Ok(dest)
}

/// 静默安装:安装器自带 taskkill 关旧实例并在装完自启新版;调用方随后 quit_event_loop
pub fn launch_installer(setup: &Path) -> Result<(), String> {
    no_window(Command::new(setup).arg("/S"))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("启动安装器失败:{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare_basics() {
        assert!(is_newer("v0.6.0", "0.5.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("v0.5.10", "0.5.9"));
        assert!(!is_newer("v0.5.0", "0.5.0"));
        assert!(!is_newer("0.4.9", "0.5.0"));
        assert!(!is_newer("garbage", "0.5.0"), "解析不出的 tag 不算新版本");
    }

    #[test]
    fn redirect_tag_parsed_from_headers() {
        let headers = "HTTP/1.1 200 Connection established\r\n\
                       HTTP/1.1 302 Found\r\n\
                       Server: GitHub.com\r\n\
                       Location: https://github.com/chengbuilds/PetPhrase/releases/tag/v0.6.0\r\n\
                       Content-Length: 0\r\n";
        assert_eq!(parse_redirect_tag(headers).as_deref(), Some("v0.6.0"));
        assert_eq!(
            parse_redirect_tag("HTTP/1.1 200 OK\r\n"),
            None,
            "无跳转头返回 None"
        );
        assert_eq!(
            parse_redirect_tag("location: https://github.com/x/y/releases\r\n"),
            None,
            "Location 不含 /tag/ 视为解析失败"
        );
    }

    #[test]
    fn redirect_tag_survives_rename_hop() {
        // curl -I -L 输出每一跳的头:仓库改名先回 301(Location 指向新仓库 latest,无 /tag/),
        // 再 302 到 tag 页。解析必须跳过第一跳、取到第二跳的 tag。
        let headers = "HTTP/1.1 301 Moved Permanently\r\n\
                       Location: https://github.com/chengbuilds/PetPhrase/releases/latest\r\n\
                       \r\n\
                       HTTP/1.1 302 Found\r\n\
                       Location: https://github.com/chengbuilds/PetPhrase/releases/tag/v0.6.1\r\n\
                       Content-Length: 0\r\n";
        assert_eq!(parse_redirect_tag(headers).as_deref(), Some("v0.6.1"));
    }

    #[test]
    fn update_from_tag_builds_asset_urls() {
        let u = update_from_tag("v0.6.0", "0.5.0").unwrap();
        assert_eq!(u.version, "0.6.0");
        assert_eq!(
            u.setup_url,
            "https://github.com/chengbuilds/PetPhrase/releases/download/v0.6.0/PetPhrase_0.6.0_x64-setup.exe"
        );
        assert_eq!(u.sha256_url, format!("{}.sha256", u.setup_url));
    }

    #[test]
    fn update_from_tag_none_when_up_to_date() {
        assert_eq!(update_from_tag("v0.6.0", "0.6.0"), None);
        assert_eq!(update_from_tag("v0.6.0", "0.7.0"), None);
    }

    #[test]
    fn parse_sha256_tolerates_formats() {
        let h = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert_eq!(parse_sha256(h).as_deref(), Some(h));
        assert_eq!(
            parse_sha256(&format!("{h} *PetPhrase_0.6.0_x64-setup.exe\n")).as_deref(),
            Some(h)
        );
        assert_eq!(
            parse_sha256(&h.to_uppercase()).as_deref(),
            Some(h),
            "统一转小写"
        );
        assert_eq!(parse_sha256("no hash here"), None);
    }

    #[test]
    #[cfg(windows)]
    fn sha256_of_known_content_via_certutil() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("hello.txt");
        std::fs::write(&f, "hello").unwrap();
        assert_eq!(
            sha256_of(&f).unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
