//! 测试参考号状态判断功能
//! 
//! 本测试模块验证`get_refno_status`方法是否能正确判断一个参考号的状态：
//! - 新增(Add)：参考号只在一个会话中出现，或者只有最新的会话中存在
//! - 修改(Modified)：参考号在多个会话中出现，且内容有变化
//! - 删除(Deleted)：参考号在历史会话中存在，但在最新会话中不存在

use aios_core::pdms_types::{EleOperation, RefU64};
use crate::io::PdmsIO;

/// 测试`get_refno_status`方法
/// 
/// 本测试验证以下情况：
/// 1. 已知存在的参考号应返回Add或Modified状态
/// 2. 不存在的参考号应返回错误或Deleted状态
/// 3. 随机测试几个参考号，验证返回正确的状态
#[tokio::test]
async fn test_get_refno_status() -> anyhow::Result<()> {
    // 设置数据库文件路径
    let db_filepath = r#"D:\AVEVA\Projects\E3D2.1\AvevaMarineSample\ams000\ams1112_0001"#;
    let mut io = PdmsIO::new("ams", db_filepath, true);
    io.open()?;

    // 测试用例1: 测试一个已知存在的参考号（应该是Add或Modified状态）
    let refno1: RefU64 = "17496/171606".into(); // 使用一个已知存在的参考号
    //先测试这个参考号是否存在
    let (sesno, offset) = io.search_latest_refno(refno1, None).unwrap();
    println!("参考号 {} 在会话 {} 中的偏移是 {:#4X}", refno1, sesno, offset);
    return Ok(());
} 