use anyhow::Result;
use aios_core::pdms_types::{EleOperation, RefU64};
use pdms_io::io::PdmsIO;
use std::env;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<()> {
    // 获取命令行参数
    // let args: Vec<String> = env::args().collect();
    // if args.len() < 3 {
    //     eprintln!("用法: {} <PDMS数据库文件路径> <参考号>", args[0]);
    //     return Ok(());
    // }

    let db_path = r#"D:\AVEVA\Projects\E3D2.1\AvevaMarineSample\ams000\ams1112_0001"#;
    let refno_str = "17496/497128";
    
    // 解析参考号
    let refno = match RefU64::try_from(refno_str) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("无效的参考号格式 '{}': {}", refno_str, e);
            return Ok(());
        }
    };
    
    println!("测试参考号 {} 的状态", refno);
    
    // 创建并打开数据库
    let mut io = PdmsIO::new("test", db_path, true);
    io.open()?;
    
    // 首先检查参考号是否存在
    println!("检查参考号是否存在...");
    match io.search_latest_refno(refno, None) {
        Ok((sesno, offset)) => {
            println!("参考号存在！会话号={}, 偏移={:#4X}", sesno, offset);
        },
        Err(e) => {
            println!("参考号不存在: {}", e);
            return Ok(());
        }
    }

    println!("搜索参考号的历史记录...");
    match io.search_history_refnos(refno, None) {
        Ok(history) => {
            println!("找到 {} 条历史记录:", history.len());
            for (i, (sesno, offset)) in history.iter().enumerate() {
                println!("  历史记录 {}: 会话号={}, 偏移={:#4X}", i+1, sesno, offset);
            }
        },
        Err(e) => {
            println!("搜索历史记录失败: {}", e);
        }
    }

    Ok(())
} 

//如何指定一个参考号，就能找到它的上一个版本，仅仅是通过索引
//17496/497128