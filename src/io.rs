use aios_core::get_default_pdms_db_info;
use aios_core::pdms_types::{PdmsElement, RefU64};
use std::collections::{BTreeMap, BTreeSet};
use std::convert::TryInto;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::Arc;
// use crate::common::get_parsed_data;
use crate::defines::{
    DbPageBasicInfo, IndexPageData, PdmsHeader, RefnoIndexPage, RootIndexPage, SessionPageData,
};
use parse_pdms_db::parse::EleData;
use parse_pdms_db::parse::{parse_attr_members, parse_ele_data, parse_ele_membs};

#[derive(Debug)]
pub struct PdmsIO {
    pub path: PathBuf,
    pub readonly: bool,
    pub file: Option<File>,
}

impl PdmsIO {
    ///新建一个PdmsIO
    pub fn new<P: AsRef<Path>>(path: P, readonly: bool) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            readonly,
            file: None,
        }
    }

    pub fn open(&mut self) -> anyhow::Result<()> {
        let mut file = File::options().read(self.readonly).open(&self.path)?;
        self.file = Some(file);
        Ok(())
    }

    ///获取单个element数据
    pub async fn get_element(&mut self, refno_offset: u64) -> anyhow::Result<EleData> {
        let mut file = self.file.as_mut().unwrap();
        let mut data = vec![0u8; 0x800];
        file.seek(SeekFrom::Start(refno_offset))?;
        file.read_exact(&mut data)?;

        let input = if data[..4] == [0, 0, 0, 0x7] {
            &data[4..]
        } else {
            &data[..]
        };
        parse_ele_data(input).await
    }

    ///获得page的信息
    pub fn get_page_basic_info(&mut self) -> anyhow::Result<DbPageBasicInfo> {
        let pdms_header = self.read_pdms_header()?;
        // println!("{:#04X?}", &pdms_header);
        let latest_ses_pageno = pdms_header.page_no;
        let latest_ses_data = self.read_ses_data(latest_ses_pageno)?;
        let mut file = self.file.as_mut().unwrap();
        Ok(DbPageBasicInfo {
            pdms_header,
            latest_ses_pageno,
            latest_ses_data,
            file_size: file.metadata().unwrap().len(),
        })
    }

    #[inline]
    pub fn read_pdms_header(&mut self) -> anyhow::Result<PdmsHeader> {
        let mut file = self.file.as_mut().unwrap();
        file.seek(SeekFrom::Start(0u64))?;
        let mut head_data = vec![];
        head_data.resize(size_of::<PdmsHeader>(), 0u8);
        file.read_exact(&mut head_data)?;
        let pdms_header = PdmsHeader::try_from(head_data.as_ref())?;
        Ok(pdms_header)
    }

    #[inline]
    pub fn read_ses_data(&mut self, ses_pageno: u32) -> anyhow::Result<SessionPageData> {
        let mut file = self.file.as_mut().unwrap();
        let mut ses_data = vec![];
        ses_data.resize(size_of::<SessionPageData>(), 0u8);
        file.seek(SeekFrom::Start(ses_pageno as u64 * 0x800))?;
        file.read_exact(&mut ses_data)?;
        let ses_page_data = SessionPageData::try_from(ses_data.as_ref())?;
        Ok(ses_page_data)
    }

    #[inline]
    pub fn read_index_data(&mut self, index_pageno: u32) -> anyhow::Result<IndexPageData> {
        let mut file = self.file.as_mut().unwrap();
        let mut ses_data = vec![];
        ses_data.resize(0x800, 0u8);
        file.seek(SeekFrom::Start(index_pageno as u64 * 0x800))?;
        file.read_exact(&mut ses_data)?;
        let ses_page_data = IndexPageData::try_from(ses_data.as_ref())?;
        Ok(ses_page_data)
    }

    ///收集发生修改的参考号直到某个pageno为止,
    pub async fn collect_increment_eles(
        &mut self,
        basic_info: &DbPageBasicInfo,
        till_pageno: Option<u32>,
    ) -> anyhow::Result<Vec<EleData>> {
        let mut ses_info = self.get_page_basic_info()?;
        let mut cur_ses_page = ses_info.latest_ses_data.clone();
        let cur_page = ses_info.pdms_header.page_no;
        let session_addr = cur_page as u64 * 0x800;
        let mut cur_index_page = cur_ses_page.index_root_pageno;
        // let mut cur_index_addr = info.ses_start.index_root_pageno as u64 * 0x800;
        let mut last_ses_page_no = cur_ses_page.last_ses_pageno;
         //查询到所有大于当前pageno的参考号，即是修改的参考号
         let latest_index_page = self.read_index_data(cur_index_page)?;
        #[cfg(debug_assertions)]
        {
            println!("Till pageno: {:#04X?}", till_pageno);
            println!("pageno: {:#04X}, index addr: ({:#04X}, {:#04X}), session addr: ({:#04X}, {:#04X})",
                     cur_page, cur_index_page, cur_index_page * 0x800,
                     session_addr, session_addr/0x800);
            println!("{:#04X?}", &latest_index_page.level);
        }
        let count = latest_index_page.level;
        let mut i = 0;
        let mut refno_data_offsets_map = BTreeMap::new();
        loop {
            let offset_page = cur_index_page - i;
            #[cfg(debug_assertions)]
            println!("offset_page: {:#04X}", offset_page);
            // let offset =  offset_page * 0x800;
            if let Some(till) = till_pageno {
                //达到目标页，跳出循环
                if offset_page <= till {
                    println!("Scan to {:#04X} end.", till);
                    break;
                }
            }
            if let Ok(cur_index_page_data) = self.read_index_data(offset_page) {
                #[cfg(debug_assertions)]
                {
                    dbg!(&cur_index_page_data.level);
                    dbg!(&last_ses_page_no);
                }

                //找到最近的索引
                if cur_index_page_data.level == 0 {
                    // println!("offset: ({:#04X?}, {:#04X?})", offset_page, offset_page * 0x800);
                    cur_index_page_data
                        .refno_locs
                        .iter()
                        //todo 需要弄清楚 00 02 C9 59， 这里的00 02 是什么含义
                        .filter(|x| {
                            x.page_no > last_ses_page_no && x.page_no < basic_info.pdms_header.page_no
                        })
                        .for_each(|x| {
                            #[cfg(debug_assertions)]
                            println!("Found loc: {:#04X?}", x);
                            let data_page_offset = x.page_no as u64 * 0x800;
                            let refno_att_offset = data_page_offset + x.offset as u64 * 2;
                            refno_data_offsets_map.insert(
                                RefU64::from_two_nums(x.refno_0, x.refno_1),
                                refno_att_offset,
                            );
                            #[cfg(debug_assertions)]
                            println!("data_offset: {:#04X?}", refno_att_offset);
                        });
                }
            } else {
                let last_ses_pageno = cur_ses_page.last_ses_pageno;
                cur_ses_page = self.read_ses_data(last_ses_pageno)?;
                cur_index_page = cur_ses_page.index_root_pageno;
                #[cfg(debug_assertions)]
                println!("jump to index: {:#04X?}", cur_index_page);
                if till_pageno.is_some() {
                    //重置索引
                    i = 0;
                    continue;
                } else {
                    break;
                }
            }
            i += 1;
        }

        let mut eles = Vec::with_capacity(refno_data_offsets_map.len());
        for (refno, offset) in refno_data_offsets_map {
            // dbg!(refno);
            match self.get_element(offset).await {
                Ok(ele) => {
                    eles.push(ele);
                }
                Err(e) => {
                    dbg!(e);
                    dbg!(offset);
                    dbg!(refno);
                }
            }
        }
        Ok(eles)
    }

    pub fn search_refno(&mut self, refno: RefU64) -> anyhow::Result<bool> {
        let mut file = self.file.as_mut().unwrap();
        file.seek(SeekFrom::Start(0u64))?;
        let mut head_data = vec![];
        head_data.resize(size_of::<PdmsHeader>(), 0u8);
        file.read_exact(&mut head_data)?;
        // dbg!(data[]);
        let pdms_header = PdmsHeader::try_from(head_data.as_ref()).unwrap();
        // println!("{:#04X?}", &pdms_header);

        let ses_addr = pdms_header.page_no * 0x800;
        // println!("Ses addr: {:#04X}", ses_addr);

        let mut ses_data = vec![];
        ses_data.resize(size_of::<SessionPageData>(), 0u8);
        file.seek(SeekFrom::Start(ses_addr as u64))?;
        file.read_exact(&mut ses_data)?;
        let ses_start_part = SessionPageData::try_from(ses_data.as_ref()).unwrap();
        // println!("{:#04X?}", &ses_start_part);

        //todo find this refno
        //when not found in current session page, should search back
        //=23584/5653
        //00 00 5C 20 00 00 16 15
        let indx_addr = ses_start_part.index_root_pageno * 0x800;
        // println!("index addr: {:#04X}", indx_addr);
        let mut data = vec![];
        data.resize(size_of::<RootIndexPage>(), 0u8);
        file.seek(SeekFrom::Start(indx_addr as u64))?;
        file.read_exact(&mut data)?;
        let index_root = RootIndexPage::try_from(data.as_ref()).unwrap();
        // println!("{:#04X?}", &index_root);

        let l = &index_root.lower_root;
        let u = &index_root.upper_root;

        // dbg!(l);
        // dbg!(u);
        // println!("{:#4X?} {:#4X?}", l.refno_0, l.refno_1);
        // println!("{:#4X?} {:#4X?}", u.refno_0, u.refno_1);

        //先搜索这个最大的范围，然后再缩小范围
        //here we need a loop to find the right location range
        let root_loc = if refno.get_0() == l.refno_0
            && (refno.get_1() >= l.refno_1 && refno.get_1() < u.refno_1)
        {
            Some(l)
        } else {
            None
        };

        let root_loc = root_loc.unwrap();

        ///一步步找到目标refno
        let target_root_addr = root_loc.page_no * 0x800;
        println!("cur index addr: {:#04X}", target_root_addr);
        let mut data = vec![0u8; 0x800];
        file.seek(SeekFrom::Start(target_root_addr as u64))?;
        file.read_exact(&mut data)?;
        let index_pgid = RefnoIndexPage::try_from(data.as_ref()).unwrap();
        // println!("{:#04X?}", &index_pgid);

        //todo 使用memchr直接去找到目标refno
        let target_refno_loc = index_pgid
            .data_locs
            .iter()
            .position(|x| x.refno_0 == refno.get_0() && x.refno_1 > refno.get_1());
        let target_refno_pgid = if let Some(mut t) = target_refno_loc {
            if t > 0 {
                t = t - 1;
            }
            let target_refno_pgid = &index_pgid.data_locs[t];
            println!("target_refno_pgid: {:#04X?}", target_refno_pgid);
            Some(target_refno_pgid)
        } else {
            None
        };

        //todo temp
        let target_refno_pgid = target_refno_pgid.unwrap();

        let target_loc = target_refno_pgid.page_no * 0x800;
        println!("cur index addr: {:#04X}", target_loc);
        let mut data = vec![0u8; 0x800];
        file.seek(SeekFrom::Start(target_loc as u64))?;
        file.read_exact(&mut data)?;
        let index_data = IndexPageData::try_from(data.as_ref()).unwrap();
        // println!("{:#04X?}", &index_data);

        let target_data_loc = index_data
            .refno_locs
            .iter()
            .find(|x| x.refno_0 == refno.get_0() && x.refno_1 == refno.get_1());
        // println!("{:#04X?}", &target_data_loc);

        if let Some(l) = target_data_loc {
            let _loc = l.page_no * 0x800 + l.offset as u32 * 2;
            // println!("data loc {:#04X?}", loc);
            // let element = self.get_element( loc)?;
            // dbg!(&element);
        }

        let _upper_root_addr = u.page_no * 0x800;
        // println!("last index addr: {:#04X}", upper_root_addr);

        Ok(true)
    }
}
