fn main() {
    // 版本双源校验:Cargo.toml 版本是更新检查的比对基准,installer.nsi 是安装包实际版本,
    // 两者不一致会让一键更新永远「已是最新」或死循环提示,构建期直接拦下
    let nsi = std::fs::read_to_string("installer.nsi").expect("read installer.nsi failed");
    let cargo_ver = std::env::var("CARGO_PKG_VERSION").unwrap();
    let nsi_ver = nsi
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("!define APP_VERSION \"")
                .and_then(|r| r.strip_suffix('"'))
        })
        .expect("installer.nsi missing APP_VERSION define");
    assert_eq!(
        nsi_ver, cargo_ver,
        "版本不一致:Cargo.toml = {cargo_ver},installer.nsi APP_VERSION = {nsi_ver}"
    );
    println!("cargo:rerun-if-changed=installer.nsi");

    slint_build::compile("ui/app.slint").expect("slint build failed");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "PetPhrase");
        res.set("FileDescription", "PetPhrase 桌宠常用语工具");
        res.compile().expect("winresource failed");
    }
}
