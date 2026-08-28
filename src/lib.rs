pub mod common;
pub mod config;
pub mod defines;
#[allow(warnings)]
pub mod io;
/// 净窗口收集：会话索引差分的净三态 → 与逐会话回放同形状的操作流。
pub mod net_window;
pub mod search;
/// 会话索引双根差分：给定库文件与 sesno 窗口，只靠文件本身判净增删改。
pub mod session_index_diff;
pub mod snapshot;
pub mod test;

pub mod sync;

pub mod watch;

pub mod io_log;

// 重新导出常用函数，使其可以直接从crate根访问
pub use io::PdmsIO;
#[cfg(any(test, feature = "legacy_session_replay"))]
pub use io::benchmark_increment_eles;

/// 生产构建中逐会话实体回放 API 必须在类型层面不存在。
///
/// ```compile_fail
/// use pdms_io::PdmsIO;
/// let _ = PdmsIO::collect_increment_eles;
/// ```
#[cfg(not(feature = "legacy_session_replay"))]
pub struct LegacySessionReplayUnavailableInProduction;

#[cfg(all(test, feature = "legacy_session_replay"))]
mod legacy_session_replay_feature_tests {
    use super::PdmsIO;

    #[test]
    fn feature_exports_every_legacy_entity_replay_entrypoint() {
        let _ = PdmsIO::get_refno_operation_status;
        let _ = PdmsIO::get_refno_primary_operation_status;
        let _ = PdmsIO::collect_eles_in_session;
        let _ = PdmsIO::collect_increment_eles;
        let _ = PdmsIO::collect_increment_eles_optimized;
        let _ = PdmsIO::collect_recent_n_sessions_eles;
        let _ = PdmsIO::collect_latest_eles;
        let _ = PdmsIO::collect_and_save_latest_data;
    }
}

// 重新导出配置管理功能
pub use config::{Config, ConfigInfo};

// 重新导出日志配置功能
pub use io_log::{LogConfig, init_log, init_log_advanced, init_log_with_file};

#[cfg(test)]
pub mod tests;

pub mod surql;
