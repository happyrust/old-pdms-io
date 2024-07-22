use aios_core::{init_test_surreal, RefU64};
use aios_core::tool::db_tool::db1_dehash;
use crate::io::PdmsIO;

#[tokio::test]
async fn test_parse_ele(){
    let refno: RefU64 = "17496/269393".into();
    //首先要根据参考号的索引结构找到这个数据
    // let refno_loc =
    let db_path = "D:/AVEVA/Projects/E3D2.1/AvevaMarineSample/ams000/ams1112_0001";
    let mut io = PdmsIO::new("ams", db_path, true);
    let att = io.auto_get_elements_deep(refno).await;
    dbg!(att);
}


#[tokio::test]
async fn test_read_all_sessions(){
    // let db_path = "D:/AVEVA/Projects/E3D2.1/AvevaMarineSample/ams000/ams1112_0001";
    init_test_surreal().await;
    let db_path = "/Users/dongpengcheng/Documents/models/e3d_models/AvevaMarineSample/ams000/ams1112_0001";
    let mut io = PdmsIO::new("ams", db_path, true);
    io.total_sync_sessions_to_db().await.unwrap();
    // io.collect_refno_locs_in_session(0x5B1E).await;
    // // let mut cur_ses_page  = io.get_page_basic_info().unwrap().latest_ses_data;
    // let pdms_header = io.read_pdms_header().unwrap();
    // let mut cur_ses_pgno = pdms_header.latest_ses_pgno;
    // let all_ents_in_ses = io.collect_eles_in_session(cur_ses_pgno as _).await;
    // dbg!(&all_ents_in_ses);
    // //显示出有哪些修改，使用 json diff 工具
    //
    //
    // while cur_ses_pgno >= 4 {
    //     let cur_ses_page  = io.read_ses_data(cur_ses_pgno as _).unwrap();
    //     // let offset = cur_ses_page.end_pgno * 0x800 + 0x4;
    //     // let bytes = io.read_bytes(offset, 4).unwrap();
    //     // let type_name = db1_dehash(u32::from_be_bytes(bytes.try_into().unwrap()));
    //     // dbg!(type_name);
    //     // println!("session pgno {:#4X}: {:#4X}", cur_ses_pgno, offset / 0x800);
    //     // dbg!((cur_ses_no, offset));
    //     if cur_ses_page.last_ses_pageno < 0{
    //         break;
    //     }
    //     cur_ses_pgno = cur_ses_page.last_ses_pageno as _;
    //     // dbg!(last_ses_no);
    //     // dbg!(cur_ses_page.get_timestamp());
    //     // dbg!(cur_ses_page.get_computer_name());
    //     // dbg!(cur_ses_page.get_comments_name());
    // }

    // let refno: RefU64 = "17496_100000".into();
    // //首先要根据参考号的索引结构找到这个数据
    // let att = io.auto_get_element(refno).await.unwrap();
    // dbg!(&att);

    // dbg!(&basic_info.latest_ses_data.get_computer_name());
    // dbg!(&basic_info.latest_ses_data.get_comments_name());
    // dbg!(&basic_info.latest_ses_data.get_timestamp());

}