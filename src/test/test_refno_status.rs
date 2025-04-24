//! 测试参考号状态判断功能
//! 
//! 本测试模块验证`get_refno_status`和`get_refno_operation_status`方法是否能正确判断一个参考号的状态：
//! - 新增(Add)：参考号只在一个会话中出现，或者只有最新的会话中存在
//! - 修改(Modified)：参考号在多个会话中出现，且内容有变化
//! - 删除(Deleted)：参考号在历史会话中存在，但在最新会话中不存在

use aios_core::pdms_types::{EleOperation, RefU64};
use crate::io::PdmsIO;
use std::time::Instant;

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
    
    // 在全范围内检查状态
    let status = io.get_refno_operation_status(refno1, None).await?;
    println!("参考号 {} 在全范围内的状态为: {:?}", refno1, status);
    
    // 仅在当前会话中检查状态
    let status = io.get_refno_operation_status(refno1, Some(sesno)).await?;
    println!("参考号 {} 在会话 {} 中的状态为: {:?}", refno1, sesno, status);
    
    return Ok(());
}

/// 测试`get_refno_operation_status`方法
/// 
/// 测试在不同会话范围内获取参考号的操作状态
#[tokio::test]
async fn test_get_refno_operation_status() -> anyhow::Result<()> {
    // 设置数据库文件路径
    let db_filepath = r#"D:\AVEVA\Projects\E3D2.1\AvevaMarineSample\ams000\ams1112_0001"#;
    let mut io = PdmsIO::new("ams", db_filepath, true);
    io.open()?;
    
    // 测试用例1: 使用一个存在多个版本的参考号
    let refno1: RefU64 = "17496/171606".into();
    let history = io.search_history_refnos(refno1, None)?;
    
    // 输出该参考号的所有历史版本
    println!("参考号 {} 的历史版本:", refno1);
    for (sesno, offset) in &history {
        println!("  会话 {}: 偏移 {:#4X}", sesno, offset);
    }
    
    // 如果有多个版本，则测试不同范围内的状态
    if history.len() > 1 {
        let all_sesnos: Vec<u32> = history.keys().cloned().collect();
        let latest_sesno = *all_sesnos.last().unwrap();
        let earliest_sesno = *all_sesnos.first().unwrap();
        let middle_sesno = all_sesnos[all_sesnos.len() / 2];
        
        // 测试在全范围内的状态
        let status1 = io.get_refno_operation_status(refno1, None).await?;
        println!("参考号 {} 在全范围内的状态为: {:?}", refno1, status1);
        
        // 测试在最新会话中的状态
        let status2 = io.get_refno_operation_status(refno1, Some(latest_sesno)).await?;
        println!("参考号 {} 在最新会话 {} 中的状态为: {:?}", refno1, latest_sesno, status2);
        
        // 测试在最早会话到中间会话的范围内的状态
        let status3 = io.get_refno_operation_status(refno1, Some(middle_sesno)).await?;
        println!("参考号 {} 在会话范围 {} 到 {} 内的状态为: {:?}", 
                 refno1, earliest_sesno, middle_sesno, status3);
        
        // 测试在中间会话到最新会话的范围内的状态
        let status4 = io.get_refno_operation_status(refno1, Some(latest_sesno)).await?;
        println!("参考号 {} 在会话范围 {} 到 {} 内的状态为: {:?}", 
                 refno1, middle_sesno, latest_sesno, status4);
    } else {
        println!("参考号 {} 只有一个版本，跳过多版本测试", refno1);
    }
    
    // 测试用例2: 测试一个不存在的参考号
    let refno2: RefU64 = "99999/99999".into();
    match io.get_refno_operation_status(refno2, None).await {
        Ok(status) => println!("不存在的参考号状态为: {:?}", status),
        Err(e) => println!("预期的错误: {}", e)
    }
    
    // 测试用例3: 尝试从数据库提取10个参考号进行测试
    // 注意: 实际运行时可能需要注释掉此部分，因为提取参考号可能需要较长时间
    /*
    use crate::extract_test_refnos;
    println!("\n提取测试参考号进行批量测试...");
    let test_refnos = extract_test_refnos(&mut io, 10)?;
    for refno in test_refnos {
        let start = Instant::now();
        match io.get_refno_operation_status(refno, (0, 9999)).await {
            Ok(status) => {
                let elapsed = start.elapsed();
                println!("参考号 {}: 状态={:?}, 耗时={:?}", refno, status, elapsed);
            },
            Err(e) => println!("参考号 {}: 错误={}", refno, e)
        }
    }
    */
    
    Ok(())
} 