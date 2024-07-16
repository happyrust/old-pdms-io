use aios_core::RefU64;
use crate::io::PdmsIO;

#[tokio::test]
async fn test_parse_ele(){
    let refno: RefU64 = "17496/269393".into();
    //首先要根据参考号的索引结构找到这个数据
    // let refno_loc =
    let db_path = "D:/AVEVA/Projects/E3D2.1/AvevaMarineSample/ams000/ams1112_0001";
    let mut io = PdmsIO::new(db_path, true);
    let att = io.auto_get_elements_deep(refno).await;
    dbg!(att);
}


#[test]
fn test_read_all_sessions(){
    // let db_path = "D:/AVEVA/Projects/E3D2.1/AvevaMarineSample/ams000/ams1112_0001";
    let db_path = "/Users/dongpengcheng/Documents/models/e3d_models/AvevaMarineSample/ams000/ams1112_0001";
    let mut io = PdmsIO::new(db_path, true);
    let basic_info = io.get_page_basic_info().unwrap();
    dbg!(&basic_info.latest_ses_data.get_computer_name());
    dbg!(&basic_info.latest_ses_data.get_comments_name());
    dbg!(&basic_info.latest_ses_data.get_timestamp());

}