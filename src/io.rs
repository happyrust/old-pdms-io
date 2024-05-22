use aios_core::get_default_pdms_db_info;
use aios_core::pdms_types::{PdmsElement, RefU64};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::convert::TryInto;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use anyhow::anyhow;
use memchr::memmem::rfind_iter;
// use crate::common::get_parsed_data;
use crate::defines::{DbPageBasicInfo, IndexPageData, PdmsHeader, RefnoDataLoc, RefnoIndexPage, RootIndexPage, SessionPageData};
use parse_pdms_db::parse::EleData;
use parse_pdms_db::parse::{parse_attr_members, parse_ele_data, parse_ele_membs};

#[derive(Debug)]
pub struct PdmsIO {
    pub path: PathBuf,
    pub readonly: bool,
    pub file: Option<File>,
}

const REFNO_LEAF_INDEX_PAGE: [u8; 16] = [0x00u8, 0x00, 0x00, 0x05, 0x00, 0xCC, 0x47, 0xDF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02];

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
        let file = File::options().read(self.readonly).open(&self.path)?;
        self.file = Some(file);
        Ok(())
    }

    fn get_file(&mut self) -> anyhow::Result<&mut File> {
        if self.file.is_none() {
            self.open()?;
        }
        Ok(self.file.as_mut().unwrap())
    }
    pub fn get_att_latest_pgno(&mut self) -> anyhow::Result<u32> {
        let mut file = self.get_file()?;
        let mut input = vec![];
        file.read_to_end(&mut input)?;
        let file_max_pgno = input.len() as u32 / 0x800;
        let mut pos_iter = rfind_iter(&input, &REFNO_LEAF_INDEX_PAGE[..]);
        let mut max_pgno = 0;
        while let Some(pos) = pos_iter.next() {
            // println!("Found leaf index page at: {:#04X?}", pgno);
            let index_data = self.read_index_data((pos / 0x800) as _)?;
            // dbg!(&index_data);
            max_pgno = index_data.refno_locs.iter().filter(|x|
                x.page_no <= file_max_pgno)
                .map(|x| x.page_no).max().unwrap_or_default().max(max_pgno);
            break;
        }
        Ok(max_pgno)
    }

    //todo 可以提前加载一些索引结构, 加载成BTree，可以很快的定位到page
    pub fn search_refno_pgno(&mut self, refno: RefU64) -> anyhow::Result<RefnoDataLoc> {
        let basic_info = self.get_page_basic_info()?;
        let latest_index_pgno = basic_info.latest_ses_data.index_root_pageno;
        let mut index_data = self.read_index_data(latest_index_pgno)?;
        let mut level = index_data.level as i32;
        let r0 = refno.get_0();
        let r1 = refno.get_1();
        // println!("Target: ({:#4X}, {:#4X}), latest index pgno: {:#4X}", r0, r1, latest_index_pgno);
        while level >= 0 {
            // dbg!(&index_data.refno_locs);
            let mut next_loc_index = if level == 0{
                index_data.refno_locs.iter().position(|x| x.refno_0==r0 && x.refno_1==r1)
            } else {
                index_data.refno_locs.windows(2).position(
                    |x| (x[1].refno_0 > r0 && x[0].refno_0 <= r0)   //r0的范围找到后，可以停止
                        || (
                        (r0 >= x[0].refno_0 && r1 >= x[0].refno_1)
                            && (r0 <= x[1].refno_0 && r1 < x[1].refno_1)
                    )
                )
            };
            if level == 0 && next_loc_index.is_none(){
                break;
            }
            let indx = next_loc_index.unwrap_or(index_data.refno_locs.len() - 1);
            // dbg!(next_loc_index);
            let d = index_data.refno_locs[indx].clone();
            let next_pgno = d.page_no;
            // println!("index level {level}, next_pgno is {:#4X}", next_pgno);
            if level == 0 {
                // println!("index level {level}, found pgno is {:#4X}", next_pgno);
                return Ok(d);
            }else{
                index_data = self.read_index_data(next_pgno)?;
                level -= 1;
            }
        }

        Err(anyhow!("Can't find the att pos loc"))
    }

    ///获取单个element数据
    pub async fn get_element(&mut self, refno_offset: u64) -> anyhow::Result<EleData> {
        let mut file = self.get_file()?;
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

    pub async fn auto_get_element(&mut self, refno: RefU64) -> anyhow::Result<EleData> {
        let loc = self.search_refno_pgno(refno)?;
        self.get_element(loc.get_att_offset()).await
    }

    ///获得page的信息
    pub fn get_page_basic_info(&mut self) -> anyhow::Result<DbPageBasicInfo> {
        let pdms_header = self.read_pdms_header()?;
        // println!("{:#04X?}", &pdms_header);
        let latest_ses_pageno = pdms_header.page_no;
        let latest_ses_data = self.read_ses_data(latest_ses_pageno)?;
        let file = self.get_file()?;
        Ok(DbPageBasicInfo {
            pdms_header,
            latest_ses_pageno,
            latest_ses_data,
            file_size: file.metadata().unwrap().len(),
        })
    }

    #[inline]
    pub fn read_pdms_header(&mut self) -> anyhow::Result<PdmsHeader> {
        let file = self.get_file()?;
        file.seek(SeekFrom::Start(0u64))?;
        let mut head_data = vec![];
        head_data.resize(size_of::<PdmsHeader>(), 0u8);
        file.read_exact(&mut head_data)?;
        let pdms_header = PdmsHeader::try_from(head_data.as_ref())?;
        Ok(pdms_header)
    }

    //todo 增加缓存
    #[inline]
    pub fn read_ses_data(&mut self, ses_pageno: u32) -> anyhow::Result<SessionPageData> {
        let file = self.get_file()?;
        let mut ses_data = vec![];
        ses_data.resize(size_of::<SessionPageData>(), 0u8);
        file.seek(SeekFrom::Start(ses_pageno as u64 * 0x800))?;
        file.read_exact(&mut ses_data)?;
        let ses_page_data = SessionPageData::try_from(ses_data.as_ref())?;
        Ok(ses_page_data)
    }

    #[inline]
    pub fn read_index_data(&mut self, index_pgno: u32) -> anyhow::Result<IndexPageData> {
        let file = self.get_file()?;
        let mut ses_data = vec![];
        ses_data.resize(0x800, 0u8);
        file.seek(SeekFrom::Start(index_pgno as u64 * 0x800))?;
        file.read_exact(&mut ses_data)?;
        let ses_page_data = IndexPageData::try_from(ses_data.as_ref())?;
        Ok(ses_page_data)
    }

    //直接读取中间这段数据的att index table，直接获取所有需要的数据
    pub async fn collect_increment_eles(
        &mut self,
        basic_info: &DbPageBasicInfo,
        till_pageno: u32,
    ) -> anyhow::Result<HashMap<RefU64, EleData>> {
        let mut file = self.get_file()?;
        let mut input = vec![];
        let start = till_pageno as u64 * 0x800;
        #[cfg(feature = "debug_parse")]
        println!("Bytes start at : {:#04X?}", start);
        file.seek(SeekFrom::Start(start)).expect("collect_increment_eles");
        file.read_to_end(&mut input)?;
        // let file_max_pgno = basic_info.latest_ses_data.index_root_pageno;
        let mut pos_iter = rfind_iter(&input, &REFNO_LEAF_INDEX_PAGE[..]);
        // let mut max_pgno = 0;
        let mut refno_data_offsets_map = BTreeMap::new();
        while let Some(mut pos) = pos_iter.next() {
            pos += start as usize;
            #[cfg(feature = "debug_parse")]
            println!("Found leaf index page at: {:#04X?}", pos);
            let index_data = self.read_index_data((pos / 0x800) as _)?;
            for x in index_data.refno_locs {
                if x.page_no < till_pageno {
                    continue;
                }
                let refno_att_offset = x.get_att_offset();
                #[cfg(feature = "debug_parse")]
                println!("Found loc: {:#04X?}, att_offset: {:#04X}", &x, refno_att_offset);
                let refno = RefU64::from_two_nums(x.refno_0, x.refno_1);
                if !refno_data_offsets_map.contains_key(&refno) {
                    refno_data_offsets_map.insert(
                        refno,
                        refno_att_offset,
                    );
                }
            }
        }

        let mut eles_map = HashMap::new();
        for (refno, offset) in refno_data_offsets_map {
            match self.get_element(offset).await {
                Ok(ele) => {
                    eles_map.insert(ele.refno, ele);
                }
                Err(e) => {
                    #[cfg(feature = "debug_parse")]
                    {
                        dbg!((refno, offset, e));
                    }
                }
            }
        }
        Ok(eles_map)
    }

    ///收集发生修改的参考号直到某个pageno为止,
    pub async fn collect_increment_eles_old(
        &mut self,
        basic_info: &DbPageBasicInfo,
        till_pageno: Option<u32>,
    ) -> anyhow::Result<HashMap<RefU64, EleData>> {
        let ses_info = self.get_page_basic_info()?;
        // #[cfg(feature = "debug_parse")]
        // dbg!(&ses_info);
        let mut cur_ses_page = ses_info.latest_ses_data.clone();
        let cur_page = ses_info.pdms_header.page_no;
        let session_addr = cur_page as u64 * 0x800;
        let mut cur_index_page = cur_ses_page.index_root_pageno;
        let mut last_ses_page_no = cur_ses_page.last_ses_pageno.max(0);
        //查询到所有大于当前pageno的参考号，即是修改的参考号
        let latest_index_page = self.read_index_data(cur_index_page)?;
        #[cfg(feature = "debug_parse")]
        {
            println!("Till pageno: {:#04X?}", till_pageno);
            println!("pageno: {:#04X}, index addr: ({:#04X}, {:#04X}), session addr: ({:#04X}, {:#04X})",
                     cur_page, cur_index_page, cur_index_page * 0x800,
                     session_addr / 0x800, session_addr);
            println!("latest_index_page.level: {:#04X?}", &latest_index_page.level);
            dbg!(&last_ses_page_no);
        }
        let count = latest_index_page.level;
        let mut i = 0;
        let mut refno_data_offsets_map = BTreeMap::new();
        //找到所有的index page，只保留这个范围的数据更新, 如果不是index page，直接跳过
        //是否应该从2开头的index page获得目标，而不是一直这样往上找？
        //如果找到了已经存在的参考号？
        //file index version的确定是不是要从数据库里保存，然后做对比
        //index 的信息是否要保存？
        loop {
            let offset_page = cur_index_page - i;
            #[cfg(feature = "debug_parse")]
            println!("offset_page: {:#04X}", offset_page);
            // let offset =  offset_page * 0x800;
            //从后往前的扫描
            if let Some(till) = till_pageno {
                //达到目标页，跳出循环
                if offset_page <= till {
                    #[cfg(feature = "debug_parse")]
                    println!("Scan to {:#04X} end.", till);
                    break;
                }
            }
            if let Ok(cur_index_page_data) = self.read_index_data(offset_page) {
                // #[cfg(feature = "debug_parse")]
                // {
                //     dbg!(&cur_index_page_data.level);
                // }
                //找到最近的索引
                if cur_index_page_data.level == 0 {
                    // println!("offset: ({:#04X?}, {:#04X?})", offset_page, offset_page * 0x800);
                    cur_index_page_data
                        .refno_locs
                        .iter()
                        //todo 需要弄清楚 00 02 C9 59， 这里的00 02 是什么含义
                        .filter(|x| {
                            x.page_no > last_ses_page_no as _
                                && x.page_no < basic_info.pdms_header.page_no
                                && x.page_no > till_pageno.unwrap_or(0)
                        })
                        .for_each(|x| {
                            let refno_att_offset = x.get_att_offset();
                            #[cfg(feature = "debug_parse")]
                            println!("Found loc: {:#04X?}, att_offset: {:#04X}", x, refno_att_offset);
                            refno_data_offsets_map.insert(
                                RefU64::from_two_nums(x.refno_0, x.refno_1),
                                refno_att_offset,
                            );
                        });
                }
            } else {
                let last_ses_pageno = cur_ses_page.last_ses_pageno as _;
                cur_ses_page = self.read_ses_data(last_ses_pageno)?;
                cur_index_page = cur_ses_page.index_root_pageno;
                #[cfg(feature = "debug_parse")]
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

        let mut eles_map = HashMap::new();
        for (refno, offset) in refno_data_offsets_map {
            match self.get_element(offset).await {
                Ok(ele) => {
                    eles_map.insert(ele.refno, ele);
                }
                Err(e) => {
                    #[cfg(feature = "debug_parse")]
                    {
                        dbg!((refno, offset, e));
                    }
                }
            }
        }
        Ok(eles_map)
    }

    pub fn search_refno(&mut self, refno: RefU64) -> anyhow::Result<bool> {
        let file = self.get_file()?;
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
