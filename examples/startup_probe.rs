//! 不初始化显卡的启动测量：只创建 GPUI 的 headless App，用来测字体系统耗时。
//!
//! 运行：cargo run --release --example startup_probe

use std::time::Instant;

use gpui::Application;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let started = Instant::now();
    let font_started = Instant::now();
    let _font_bootstrap = gistpic_creator::font_bootstrap::apply();
    log_phase(font_started, "字体配置");
    let app = Application::headless();
    log_phase(started, "headless App（字体和应用基础初始化）");
    drop(app);
}

fn log_phase(start: Instant, phase: &str) {
    log::info!("启动阶段 {phase}: 用时 {:?}", start.elapsed());
}
