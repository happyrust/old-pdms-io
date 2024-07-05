use aios_core::get_default_pdms_db_info;
use aios_core::pdms_types::{PdmsElement, RefU64};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::convert::TryInto;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use anyhow::anyhow;
use memchr::memmem::rfind_iter;
use crate::defines::*;
use parse_pdms_db::parse::*;

#[derive(Debug)]
pub struct PdmsIO {
    pub path: PathBuf,
    pub readonly: bool,
    pub file: Option<File>,
    pub ses_data_map: HashMap<u32, SessionPageData>,
}

const REFNO_LEAF_INDEX_PAGE: [u8; 16] = [0x00u8, 0x00, 0x00, 0x05, 0x00, 0xCC, 0x47, 0xDF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02];

impl PdmsIO {
    ///新建一个PdmsIO
    pub fn new<P: AsRef<Path>>(path: P, readonly: bool) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            readonly,
            file: None,
            ses_data_map: Default::default(),
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

    //TODO 做一个不处理UDA的方法
    #[inline]
    pub async fn auto_get_element(&mut self, refno: RefU64) -> anyhow::Result<EleData> {
        let loc = self.search_refno_pgno(refno)?;
        self.get_element(loc.get_att_offset()).await
    }

    pub async fn auto_get_elements_deep(&mut self, refno: RefU64) -> anyhow::Result<HashMap<RefU64, EleData>> {
        let mut map = HashMap::new();
        let mut pendings = VecDeque::new();
        pendings.push_back(refno);
        while let Some(refno) = pendings.pop_front() {
            let ele = self.auto_get_element(refno).await?;
            pendings.extend(&*ele.children);
            map.insert(refno, ele);
        }
        Ok(map)
    }

    ///获得page的信息
    pub fn get_page_basic_info(&mut self) -> anyhow::Result<DbPageBasicInfo> {
        let pdms_header = self.read_pdms_header()?;
        // println!("{:#04X?}", &pdms_header);
        let latest_ses_pageno = pdms_header.page_no;
        let latest_ses_data = self.read_ses_data(latest_ses_pageno)?.clone();
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

    ///读取ses data
    #[inline]
    pub fn read_ses_data(&mut self, ses_pgno: u32) -> anyhow::Result<&SessionPageData> {
        if !self.ses_data_map.contains_key(&ses_pgno){
            let file = self.get_file()?;
            let mut ses_data = vec![];
            ses_data.resize(size_of::<SessionPageData>(), 0u8);
            file.seek(SeekFrom::Start(ses_pgno as u64 * 0x800))?;
            file.read_exact(&mut ses_data)?;
            if let Ok(s) = SessionPageData::try_from(ses_data.as_ref())  {
                self.ses_data_map.insert(ses_pgno, s);
            }
        }
        return self.ses_data_map.get(&ses_pgno).ok_or(anyhow!("Can't read ses page with {ses_pgno}."));
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
