#[allow(warnings)]

pub mod io;
pub mod defines;
pub mod common;
pub mod test;

pub mod sync;

pub mod watch;

pub mod io_log;

// 重新导出常用函数，使其可以直接从crate根访问
pub use io::{PdmsIO, benchmark_increment_eles};






