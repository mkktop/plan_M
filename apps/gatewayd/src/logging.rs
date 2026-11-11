//! 日志：stdout（带级别过滤）+ 按天滚动文件（data_dir/logs）。

use tracing_appender::non_blocking::WorkerGuard;

/// 初始化日志。返回的 guard 必须在 main 里存活到进程结束。
pub fn init(data_dir: &std::path::Path) -> anyhow::Result<WorkerGuard> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let file_appender = tracing_appender::rolling::daily(data_dir.join("logs"), "gatewayd.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_thread_ids(false)
                .compact(),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(file_writer),
        )
        .try_init()?;
    Ok(guard)
}
