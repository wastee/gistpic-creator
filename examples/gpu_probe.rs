//! 单独测量 Blade 图形设备初始化时间，不创建窗口。
//!
//! 运行：cargo run --release --example gpu_probe

use std::time::Instant;

use blade_graphics as gpu;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let started = Instant::now();
    let result = unsafe {
        gpu::Context::init(gpu::ContextDesc {
            presentation: false,
            validation: false,
            ..Default::default()
        })
    };
    match result {
        Ok(context) => {
            log::info!(
                "启动阶段 Blade 图形设备初始化: 用时 {:?}",
                started.elapsed()
            );
            drop(context);
        }
        Err(error) => {
            log::error!("图形设备初始化失败：{error:?}");
            std::process::exit(1);
        }
    }
}
