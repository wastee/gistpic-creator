//! 构建脚本：目标平台是 Windows 时，把图标资源写进 exe。
//!
//! Windows 资源管理器、任务栏和 Alt+Tab 显示的是 exe 文件里嵌进去的图标，
//! 运行时的托盘图标管不到这里，只能在编译期写。
//!
//! 判断用 `CARGO_CFG_TARGET_OS` 环境变量，不能用 `cfg!(target_os)`：
//! 构建脚本本身是按宿主平台编译的，在 Linux 上交叉编译 Windows 时
//! `cfg!(target_os = "windows")` 是 false，但这里确实要写 Windows 资源。

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/icon.ico");
    resource.compile().expect("把图标写进 exe 失败");
}
