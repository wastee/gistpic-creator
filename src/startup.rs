//! 启动阶段的计时和启动测量开关。
//!
//! 正式使用时这里只打日志，不改变程序行为。传入 `--startup-bench` 时，
//! main 会在主窗口画完后直接退出，方便重复测量。

use std::time::Instant;

/// 启动测量命令行参数。用参数而不是环境变量，避免环境变量一直留着，
/// 导致之后正常启动也跳过托盘。
pub const BENCH_FLAG: &str = "--startup-bench";

/// 是否处于启动测量模式。
pub fn bench_enabled() -> bool {
    std::env::args_os()
        .skip(1)
        .any(|argument| argument == BENCH_FLAG)
}

/// 输出一个启动阶段从起点到现在的耗时。
pub fn log_phase(start: Instant, phase: &str) {
    log::info!("启动阶段 {phase}: 用时 {:?}", start.elapsed());
}
