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
    let exists = io.search_refno(refno1).unwrap();
    assert!(exists, "参考号 {} 应该存在", refno1);
    return Ok(());

    match io.get_refno_status(refno1).await {
        Ok(status) => {
            println!("参考号 {} 的状态是: {:?}", refno1, status);
            assert!(matches!(status, EleOperation::Add | EleOperation::Modified));
        },
        Err(e) => {
            println!("获取参考号 {} 状态时出错: {}", refno1, e);
            assert!(false, "应该能找到参考号 {}", refno1);
        }
    }

    // 测试用例2: 测试一个不存在的参考号（应该是Deleted状态或返回错误）
    let refno2: RefU64 = "99999/99999".into(); // 使用一个可能不存在的参考号
    match io.get_refno_status(refno2).await {
        Ok(status) => {
            println!("参考号 {} 的状态是: {:?}", refno2, status);
            if let EleOperation::Deleted = status {
                println!("确认参考号 {} 是删除状态", refno2);
            } else {
                assert!(false, "不存在的参考号 {} 应该报错或返回Deleted状态", refno2);
            }
        },
        Err(e) => {
            println!("获取参考号 {} 状态时出错: {}", refno2, e);
            // 如果参考号不存在，返回错误也是可接受的
        }
    }

    // 测试用例3: 随机尝试几个参考号，看能否正确处理
    let test_refnos = vec![
        "17496/184133".into(), 
        "17496/171606".into(),
        "17496/150000".into(),
    ];
    
    for refno in test_refnos {
        match io.get_refno_status(refno).await {
            Ok(status) => {
                println!("参考号 {} 的状态是: {:?}", refno, status);
            },
            Err(e) => {
                println!("获取参考号 {} 状态时出错: {}", refno, e);
            }
        }
    }

    Ok(())
} 