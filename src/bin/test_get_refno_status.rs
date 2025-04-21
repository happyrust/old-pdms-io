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
    let refno_str = "17496/184133";
    
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
    match io.search_refno_pgno(refno) {
        Ok(loc) => {
            println!("参考号存在！位置信息: 页号={}, 偏移={}, 地址={:#4X}", loc.pgno, loc.offset, loc.get_att_offset());
        },
        Err(e) => {
            println!("参考号不存在: {}", e);
            return Ok(());
        }
    }

    return Ok(());
    
    // 构建索引映射表
    println!("构建索引映射表...");
    let start_time = Instant::now();
    let index_map = io.build_index_map()?;
    let build_time = start_time.elapsed();
    println!("索引构建完成，耗时: {:?}, 索引大小: {} 项", build_time, index_map.len());
    
    // 使用索引映射表查找参考号
    println!("使用索引映射表查找参考号...");
    match io.fast_lookup_refno(&refno, &index_map) {
        Some(locs) => {
            for loc in locs {
                println!("在索引中找到参考号！位置信息: {:#4X}", loc);
            }
        },
        None => {
            println!("在索引中未找到参考号！");
            return Ok(());
        }
    }
    
    // 获取元素数据
    println!("获取元素数据...");
    let ele_data = io.fast_get_element(refno, &index_map).await?;
    println!("元素数据获取成功！");
    println!("子元素数量: {}", ele_data.children.len());
    println!("属性类型: {}", ele_data.att_map().get_type());
    
    // 方法1: 使用传统方法检查参考号状态
    println!("\n方法1: 使用传统方法检查参考号状态...");
    let trad_start = Instant::now();
    let trad_status = io.get_refno_status(refno).await?;
    let trad_time = trad_start.elapsed();
    
    println!("传统方法状态检查完成，耗时: {:?}", trad_time);
    println!("参考号状态: {:?}", trad_status);
    
    // 方法2: 使用优化方法检查参考号状态
    println!("\n方法2: 使用索引映射表优化方法检查参考号状态...");
    let fast_start = Instant::now();
    let fast_status = io.fast_get_refno_status(refno, &index_map).await?;
    let fast_time = fast_start.elapsed();
    
    println!("优化方法状态检查完成，耗时: {:?}", fast_time);
    println!("参考号状态: {:?}", fast_status);
    
    // 比较两种方法的速度差异
    let speedup = trad_time.as_micros() as f64 / fast_time.as_micros() as f64;
    println!("\n性能比较:");
    println!("传统方法耗时: {:?}", trad_time);
    println!("优化方法耗时: {:?}", fast_time);
    println!("速度提升: {:.2}倍", speedup);
    
    // 验证两种方法结果是否一致
    if trad_status == fast_status {
        println!("结果验证: 两种方法的结果一致 ✓");
    } else {
        println!("结果验证: 警告! 两种方法的结果不一致 ⚠");
        println!("  传统方法结果: {:?}", trad_status);
        println!("  优化方法结果: {:?}", fast_status);
    }
    
    // 根据状态输出详细信息
    println!("\n参考号状态详情:");
    match fast_status {
        EleOperation::Add => {
            println!("该参考号为新增状态。");
        },
        EleOperation::Modified => {
            println!("该参考号为修改状态。");
            println!("该参考号有历史修改记录。");
            
            // 获取当前元素和历史版本的信息
            let element = io.fast_get_element(refno, &index_map).await?;
            println!("当前版本信息:");
            println!("  类型: {}", element.att_map().get_type());
            println!("  子元素数量: {}", element.children.len());
            
            // 如果需要详细的历史信息，可以通过其他公共API获取
            // 或者请求添加一个公共方法来访问历史版本信息
        },
        EleOperation::Deleted => {
            println!("该参考号已被删除。");
        },
        _ => {
            println!("未知状态。");
        }
    }
    
    // 获取子元素信息
    if !ele_data.children.is_empty() {
        println!("\n子元素信息:");
        for (i, child_refno) in ele_data.children.iter().enumerate().take(5) {
            println!("  子元素 {}: {}", i+1, child_refno);
            
            // 检查子元素是否在索引中
            if let Some(child_loc) = io.fast_lookup_refno(child_refno, &index_map) {
                for loc in child_loc {
                    println!("    位置: {:#4X}", loc);
                }
                
                // 可选: 检查子元素状态
                if i < 2 { // 只检查前两个子元素，避免输出过多
                    match io.fast_get_refno_status(*child_refno, &index_map).await {
                        Ok(status) => {
                            println!("    状态: {:?}", status);
                        },
                        Err(_) => {
                            println!("    状态: 无法确定");
                        }
                    }
                }
            } else {
                println!("    未在索引中找到此子元素");
            }
        }
        
        if ele_data.children.len() > 5 {
            println!("  (仅显示前5个，共{}个子元素)", ele_data.children.len());
        }
    }
    
    println!("\n测试完成!");
    Ok(())
} 