use aios_core::{init_test_surreal, RefU64};

use crate::io::PdmsIO;



#[tokio::test]
async fn test_query_refno_sesno() -> anyhow::Result<()>{
    init_test_surreal().await;
    let refno = "17496_171715".into();
    let sesno = aios_core::query_refno_sesno(refno, 1, 1112).await?;
    dbg!(sesno);

    Ok(())
}
