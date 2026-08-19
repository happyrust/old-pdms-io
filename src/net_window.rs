//! 净窗口收集（ADR-022）：由会话索引差分给出窗口净三态，再按窗口两端记录版本
//! 合成与逐会话回放**相同形状**的操作流——`BTreeMap<sesno, Vec<EleOperationData>>`，
//! 每个 refno 恰一条：
//!
//! * 净新增 → `Add(终稿 EleData)`（终稿记录一次解析）；
//! * 净删除 → `Deleted`（挂窗口终点会话：净差分判不出删除动作发生在哪个会话，
//!   墓碑语义在终点已成立）；
//! * 净修改 → `Modified(ModifiedElement)`：base / 终稿两端版本各解析一次、
//!   **一次 diff** 合成（[`diff_ele_data`]）。两端内容相同（记录被原样重写换页）
//!   时不发操作——真无事发生，计入 [`NetWindowOutcome::unchanged_rewrites`]。
//!
//! 于是模型计划、交付单元归并、ref_rev 维护、MySQL 同步、语句渲染的输入形状
//! **零改动**；回放路径的 `fold_window` 与两个终态补丁在本路径上没有输入。
//!
//! 与会话索引差分同一条**纯文件纪律**：不查库、不读水位，窗口由调用方显式给定
//! （见 `the_net_window_module_never_touches_the_database` 源码断言）。

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ops::RangeInclusive;

use aios_core::pdms_types::RefU64;
use parse_pdms_db::parse::{EleData, RawElementIdentity};

pub use crate::io::diff_ele_data;
use crate::io::{EleOperationData, EleOperationDetail, PdmsIO};
use crate::session_index_diff::{self, RecordLoc};
use crate::snapshot::{DabaconSnapshot, SnapshotToken};

#[derive(Debug)]
pub struct NetWindowError {
    stage: &'static str,
    source: anyhow::Error,
}

impl NetWindowError {
    fn incomplete(stage: &'static str, source: impl Into<anyhow::Error>) -> Self {
        Self {
            stage,
            source: source.into(),
        }
    }
}

impl std::fmt::Display for NetWindowError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "dabacon 窗口在 {} 阶段不完整: {:#}",
            self.stage, self.source
        )
    }
}

impl std::error::Error for NetWindowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoredFinalRecord {
    pub refno: RefU64,
    pub noun: String,
    pub reason: String,
}

/// 一次净窗口收集的产物。
#[derive(Debug)]
pub struct NetWindowOutcome {
    /// Token proving which open file handle and target root produced the window.
    pub snapshot_token: Option<SnapshotToken>,
    /// 与回放收集同形状的操作流（每 refno 恰一条，挂 last-touch 会话）。
    pub window: BTreeMap<u32, Vec<EleOperationData>>,
    /// 必须进回执的收集警告（如「基版本解析失败，按新增全量处理」）——
    /// 静默失效是最高级别缺陷，调用方不得丢弃。
    pub warnings: Vec<String>,
    /// 记录位置变了但内容逐字段相同（原样重写换页）的条目数：不发操作，
    /// 但账要看得见。
    pub unchanged_rewrites: usize,
    /// 最小身份明确为 MNUM、按代码白名单跳过的终稿数。其他 noun 的终稿失败
    /// 无法构造 `NetWindowOutcome`。
    pub unparseable_finals: usize,
    /// The only accepted final-decode omissions. This list is produced by the
    /// code allowlist below and cannot be extended through runtime options.
    pub ignored_finals: Vec<IgnoredFinalRecord>,
    /// 双根差分之外，由父成员净减少 + 目标 OWNER 成员关系补出的删除数。
    ///
    /// E3D 删除后旧物理记录可能仍被索引遍历触达；只有这个计数能把“索引仍见、
    /// 成员关系已死”的收口显式暴露给上层口径日志。
    pub membership_deleted: usize,
    /// 窗口内诞生又被删、因此整条丢弃的新增数（既不发 Add 也不发 Deleted）。
    ///
    /// 净口径是「窗口内加了又删不出现」，但索引侧分不清「新建且活着」与「新建后
    /// 被删、孤儿记录留在索引里」——不显式记账的话，这种丢弃与「本来就没有这个
    /// 元素」在证据里长得一模一样。
    pub dropped_phantom_adds: usize,
    /// 差分统计（页读数/剪枝/耗时），随回执与日志透出。
    pub stats: session_index_diff::NetChangeStats,
}

/// 成员收口的两项产出。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct MemberReconciliation {
    /// 双根差分没判出、由目标端成员关系补出的删除数。
    membership_deleted: usize,
    /// 被整条丢弃的幽灵新增数，见 [`NetWindowOutcome::dropped_phantom_adds`]。
    dropped_phantom_adds: usize,
}

/// 对一个已打开（或可打开）的库文件做净窗口收集。
///
/// 失败语义（与回放口径逐条对齐，不许静默）：
///
/// * 净新增 / 净修改的**终稿**记录解析失败 → 先解最小身份；只有代码白名单
///   `MNUM` 可跳过并记录结构化诊断，其他 noun 立即令窗口失败。
/// * 净修改的**基版本**解析失败（终稿可读）→ 按 spec §Edge Cases 保守处理：
///   当作新增全量覆盖（模型侧整根重生成），warnings 逐条点名。
/// * 净修改条目缺 `base_loc` → **硬失败**：那是差分分类的不变量被破坏，不是
///   现场异常，不许降级。
pub fn collect_net_window(
    snapshot: &mut DabaconSnapshot,
    sesno_range: RangeInclusive<i32>,
) -> Result<NetWindowOutcome, NetWindowError> {
    snapshot
        .verify_path_identity()
        .map_err(|error| NetWindowError::incomplete("冻结文件身份校验", error))?;
    let requested_target = *sesno_range.end();
    if requested_target < 0 || requested_target as u32 != snapshot.token().target_sesno() {
        return Err(NetWindowError::incomplete(
            "冻结会话校验",
            anyhow::anyhow!(
                "窗口终点 {requested_target} 与快照冻结会话 {} 不一致",
                snapshot.token().target_sesno()
            ),
        ));
    }
    let token = snapshot.token().clone();
    let io = snapshot.io_mut();
    let net = session_index_diff::collect_net_changes(io, sesno_range, false)
        .map_err(|error| NetWindowError::incomplete("双根差分", error))?;
    let base_sesno = net.base_sesno;
    let target_sesno = net.target_sesno;
    let mut resolver = PdmsRecordResolver { io };
    let mut outcome = synthesize_net_window_with_resolver(net, token.clone(), &mut resolver)
        .map_err(|error| NetWindowError::incomplete("终稿合成", error))?;
    let reconciliation = reconcile_member_deletions(
        &mut outcome.window,
        base_sesno,
        target_sesno,
        &mut PdmsMembershipResolver { io: resolver.io },
    )
    .map_err(|error| NetWindowError::incomplete("成员删除收口", error))?;
    outcome.membership_deleted = reconciliation.membership_deleted;
    outcome.dropped_phantom_adds = reconciliation.dropped_phantom_adds;
    snapshot
        .verify_path_identity()
        .map_err(|error| NetWindowError::incomplete("提交前文件身份校验", error))?;
    Ok(outcome)
}

/// 固定会话下的元素解析窄接口：生产走 dabacon 点查，纯测试用内存桩。
trait MembershipResolver {
    fn element_at(&mut self, refno: RefU64, sesno: i32) -> anyhow::Result<Option<EleData>>;
}

struct PdmsMembershipResolver<'a> {
    io: &'a mut PdmsIO,
}

/// 维护审计入口：判断非 WORL 元素在指定会话是否仍被 OWNER 成员表接纳。
pub fn member_alive_at(io: &mut PdmsIO, refno: RefU64, target_sesno: i32) -> anyhow::Result<bool> {
    is_target_member(&mut PdmsMembershipResolver { io }, refno, target_sesno)
}

/// 维护审计入口：将已确认不可达的根按基会话成员树展开为完整删除集。
pub fn expand_deleted_membership_roots(
    io: &mut PdmsIO,
    roots: &BTreeSet<RefU64>,
    base_sesno: i32,
    target_sesno: i32,
) -> anyhow::Result<BTreeSet<RefU64>> {
    let target = u32::try_from(target_sesno)
        .map_err(|_| anyhow::anyhow!("成员审计目标会话非法: {target_sesno}"))?;
    let mut window = BTreeMap::from([(
        target,
        roots
            .iter()
            .copied()
            .map(|refno| EleOperationData::new(refno, target, EleOperationDetail::Deleted))
            .collect(),
    )]);
    reconcile_member_deletions(
        &mut window,
        Some(base_sesno),
        target_sesno,
        &mut PdmsMembershipResolver { io },
    )?;
    Ok(window
        .values()
        .flatten()
        .map(|operation| operation.refno)
        .collect())
}

impl MembershipResolver for PdmsMembershipResolver<'_> {
    fn element_at(&mut self, refno: RefU64, sesno: i32) -> anyhow::Result<Option<EleData>> {
        let sesno =
            u32::try_from(sesno).map_err(|_| anyhow::anyhow!("成员仲裁会话号非法: {sesno}"))?;
        let Some((_, offset)) = self.io.search_latest_refno(refno, Some(sesno)) else {
            return Ok(None);
        };
        self.io
            .parse_raw_element(offset)
            .map(Some)
            .map_err(|error| {
                anyhow::anyhow!("解析 {refno} 在 sesno={sesno} 的成员记录失败: {error}")
            })
    }
}

/// 从父成员净变化里收集 `old - new` 与 `new - old`。
fn member_deltas(
    window: &BTreeMap<u32, Vec<EleOperationData>>,
) -> (BTreeSet<RefU64>, BTreeSet<RefU64>) {
    let mut removed = BTreeSet::new();
    let mut attached = BTreeSet::new();
    for modified in window.values().flatten().filter_map(|operation| {
        let EleOperationDetail::Modified(modified) = &operation.detail else {
            return None;
        };
        modified.children_changed.as_ref()
    }) {
        let (old, new) = modified;
        let old = old.iter().copied().collect::<BTreeSet<_>>();
        let new = new.iter().copied().collect::<BTreeSet<_>>();
        removed.extend(old.difference(&new).copied());
        attached.extend(new.difference(&old).copied());
    }
    (removed, attached)
}

/// 判断元素在目标会话是否仍被其目标 OWNER 的成员表接纳。
fn is_target_member<R: MembershipResolver>(
    resolver: &mut R,
    refno: RefU64,
    target_sesno: i32,
) -> anyhow::Result<bool> {
    let Some(element) = resolver.element_at(refno, target_sesno)? else {
        return Ok(false);
    };
    anyhow::ensure!(
        element.owner != RefU64::default(),
        "成员删除候选 {refno} 在 sesno={target_sesno} 的 OWNER 为空"
    );
    let owner = resolver
        .element_at(element.owner, target_sesno)?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "成员删除候选 {refno} 的 OWNER {} 在 sesno={target_sesno} 不存在",
                element.owner
            )
        })?;
    Ok(owner.children.contains(&refno))
}

/// 窗口内诞生的元素（本窗口里有 `Add`）。
fn window_born_refnos(window: &BTreeMap<u32, Vec<EleOperationData>>) -> BTreeSet<RefU64> {
    window
        .values()
        .flatten()
        .filter(|operation| matches!(operation.detail, EleOperationDetail::Add(_)))
        .map(|operation| operation.refno)
        .collect()
}

/// 窗口塌缩收口：元素在窗口内诞生**又**被删时，OWNER 的成员表在窗口两端都没有
/// 它，`children_changed` 因此为空——[`member_deltas`] 那条判据看不见；而索引侧
/// 分不清「新建且活着」与「新建后被删、孤儿记录留在索引里」，一律判 `Added`。
///
/// 生产上这不是边界情形：窗口是 `[水位+1, 观察到的最新]`，用户建了件、存盘、发现
/// 不对再删掉、再存盘，两个会话落进同一次扫描是常态。
///
/// 这里对每条 `Added` 问一次目标端 OWNER 认不认它，按 OWNER 去重（一个 OWNER 只查
/// 一次记录）。只在能证伪时下结论：OWNER 为空、OWNER 在目标端读不到，都当活着
/// ——宁可漏判，也不凭空删。
fn orphaned_adds<R: MembershipResolver>(
    window: &BTreeMap<u32, Vec<EleOperationData>>,
    target_sesno: i32,
    resolver: &mut R,
) -> anyhow::Result<BTreeSet<RefU64>> {
    let mut by_owner: BTreeMap<RefU64, BTreeSet<RefU64>> = BTreeMap::new();
    for operation in window.values().flatten() {
        let EleOperationDetail::Add(added) = &operation.detail else {
            continue;
        };
        if added.owner == RefU64::default() {
            continue;
        }
        by_owner
            .entry(added.owner)
            .or_default()
            .insert(operation.refno);
    }
    let mut orphans = BTreeSet::new();
    for (owner, members) in by_owner {
        // OWNER 自己在目标端都读不到：这批的死活由 OWNER 那条判据连坐收口。
        let Some(owner_element) = resolver.element_at(owner, target_sesno)? else {
            continue;
        };
        orphans.extend(
            members
                .into_iter()
                .filter(|refno| !owner_element.children.contains(refno)),
        );
    }
    Ok(orphans)
}

/// 用目标成员关系收口删除，并沿成员树展开不可达子树。
///
/// 两项产出见 [`MemberReconciliation`]：双根差分本来已有的删除不计入补删数；
/// 窗口内诞生又死掉的元素整条丢弃，不发 `Deleted`——下游从来没存过它，一条删除
/// 只会凭空造出一行墓碑。
fn reconcile_member_deletions<R: MembershipResolver>(
    window: &mut BTreeMap<u32, Vec<EleOperationData>>,
    base_sesno: Option<i32>,
    target_sesno: i32,
    resolver: &mut R,
) -> anyhow::Result<MemberReconciliation> {
    let (removed, attached) = member_deltas(window);
    let existing_deleted = window
        .values()
        .flatten()
        .filter(|operation| matches!(operation.detail, EleOperationDetail::Deleted))
        .map(|operation| operation.refno)
        .collect::<BTreeSet<_>>();
    let window_born = window_born_refnos(window);
    let phantoms = orphaned_adds(window, target_sesno, resolver)?;
    let roots = removed
        .difference(&attached)
        .copied()
        .chain(existing_deleted.iter().copied())
        .chain(phantoms.iter().copied())
        .collect::<BTreeSet<_>>();
    if roots.is_empty() {
        return Ok(MemberReconciliation::default());
    }
    let base_sesno = base_sesno
        .ok_or_else(|| anyhow::anyhow!("成员删除候选存在，但窗口没有可展开删除子树的基会话"))?;

    let mut dead = BTreeSet::new();
    let mut queue = VecDeque::new();
    for root in roots {
        if existing_deleted.contains(&root)
            || phantoms.contains(&root)
            || !is_target_member(resolver, root, target_sesno)?
        {
            queue.push_back(root);
        }
    }

    while let Some(refno) = queue.pop_front() {
        if !dead.insert(refno) {
            continue;
        }
        // 展开子树的依据：窗口外就存在的元素看基会话那份成员表；窗口内诞生的幽灵
        // 在基会话根本没有记录，只能看它创建时写下、之后再没被改过的那一份。
        let expand_from = match resolver.element_at(refno, base_sesno)? {
            Some(base) => base,
            None => {
                anyhow::ensure!(
                    window_born.contains(&refno),
                    "删除子树元素 {refno} 在基会话 sesno={base_sesno} 不存在，拒绝猜测"
                );
                let Some(target) = resolver.element_at(refno, target_sesno)? else {
                    continue;
                };
                target
            }
        };
        for child in expand_from.children.iter().copied() {
            if dead.contains(&child) {
                continue;
            }
            let target = resolver.element_at(child, target_sesno)?;
            let moved_to_live_owner = match target {
                None => false,
                Some(target) if dead.contains(&target.owner) => false,
                Some(target) => {
                    anyhow::ensure!(
                        target.owner != RefU64::default(),
                        "删除子树成员 {child} 在 sesno={target_sesno} 的 OWNER 为空"
                    );
                    resolver
                        .element_at(target.owner, target_sesno)?
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "删除子树成员 {child} 的目标 OWNER {} 不存在",
                                target.owner
                            )
                        })?
                        .children
                        .contains(&child)
                }
            };
            if !moved_to_live_owner {
                queue.push_back(child);
            }
        }
    }

    if dead.is_empty() {
        return Ok(MemberReconciliation::default());
    }
    // 终态唯一：成员删除覆盖同 refno 的 Add/Modified，也与索引 Deleted 去重。
    for operations in window.values_mut() {
        operations.retain(|operation| !dead.contains(&operation.refno));
    }
    // 窗口内诞生又死掉的只丢不删（净口径「加了又删不出现」）；窗口外就存在的
    // 才发 Deleted。
    let (phantom_dead, prior_dead): (BTreeSet<_>, BTreeSet<_>) = dead
        .into_iter()
        .partition(|refno| window_born.contains(refno));
    let membership_deleted = prior_dead.difference(&existing_deleted).count();
    let target = u32::try_from(target_sesno)
        .map_err(|_| anyhow::anyhow!("成员删除目标会话非法: {target_sesno}"))?;
    let operations = window.entry(target).or_default();
    operations.extend(
        prior_dead
            .into_iter()
            .map(|refno| EleOperationData::new(refno, target, EleOperationDetail::Deleted)),
    );
    Ok(MemberReconciliation {
        membership_deleted,
        dropped_phantom_adds: phantom_dead.len(),
    })
}

trait RecordResolver {
    fn element_at(&mut self, loc: RecordLoc) -> anyhow::Result<EleData>;
    fn identity_at(&mut self, loc: RecordLoc) -> anyhow::Result<RawElementIdentity>;
}

struct PdmsRecordResolver<'a> {
    io: &'a mut PdmsIO,
}

impl RecordResolver for PdmsRecordResolver<'_> {
    fn element_at(&mut self, loc: RecordLoc) -> anyhow::Result<EleData> {
        self.io.parse_raw_element(loc.att_offset())
    }

    fn identity_at(&mut self, loc: RecordLoc) -> anyhow::Result<RawElementIdentity> {
        self.io.parse_raw_element_identity(loc.att_offset())
    }
}

#[cfg(test)]
struct ClosureRecordResolver<F> {
    resolve: F,
}

#[cfg(test)]
impl<F> RecordResolver for ClosureRecordResolver<F>
where
    F: FnMut(RecordLoc) -> anyhow::Result<EleData>,
{
    fn element_at(&mut self, loc: RecordLoc) -> anyhow::Result<EleData> {
        (self.resolve)(loc)
    }

    fn identity_at(&mut self, loc: RecordLoc) -> anyhow::Result<RawElementIdentity> {
        let data = (self.resolve)(loc)?;
        Ok(RawElementIdentity {
            refno: data.refno,
            noun_hash: data.noun as i32,
            noun_name: aios_core::tool::db_tool::db1_dehash(data.noun),
            owner: data.owner,
        })
    }
}

fn allowed_ignored_final(
    resolver: &mut impl RecordResolver,
    expected_refno: RefU64,
    loc: RecordLoc,
    decode_error: anyhow::Error,
) -> anyhow::Result<IgnoredFinalRecord> {
    let identity = resolver.identity_at(loc).map_err(|identity_error| {
        anyhow::anyhow!(
            "解析 {expected_refno} 的终稿失败，最小身份也无法解码；终稿错误: {decode_error:#}; 身份错误: {identity_error:#}"
        )
    })?;
    anyhow::ensure!(
        identity.refno == expected_refno,
        "终稿位置返回错误 refno：期望 {expected_refno}，实际 {}",
        identity.refno
    );
    anyhow::ensure!(
        identity.noun_name == "MNUM",
        "非白名单终稿 {expected_refno}（noun={}）解析失败: {decode_error:#}",
        identity.noun_name
    );
    Ok(IgnoredFinalRecord {
        refno: expected_refno,
        noun: identity.noun_name,
        reason: format!("{decode_error:#}"),
    })
}

/// 纯合成层：净三态 → 与回放同形状的操作流。**不碰 IO、不碰库**——记录解析由
/// resolver 注入（生产是 `PdmsIO`，单测是内存桩）。
///
/// `net` 按值接收：`stats` 直接移交 [`NetWindowOutcome`]，条目也不必逐条 clone。
/// resolver 收窄成「给我这个位置的记录」，「谁的记录 / 哪一端 / 页与偏移」由
/// [`resolve_record`] 包装——错误文案只有一处权威，测试不必复刻它。
#[cfg(test)]
fn synthesize_net_window<F>(
    net: session_index_diff::NetChangeSet,
    resolve: F,
) -> anyhow::Result<NetWindowOutcome>
where
    F: FnMut(RecordLoc) -> anyhow::Result<EleData>,
{
    let mut resolver = ClosureRecordResolver { resolve };
    synthesize_net_window_with_resolver(net, SnapshotToken::for_test(), &mut resolver)
}

fn synthesize_net_window_with_resolver(
    net: session_index_diff::NetChangeSet,
    snapshot_token: SnapshotToken,
    resolver: &mut impl RecordResolver,
) -> anyhow::Result<NetWindowOutcome> {
    let target_sesno = net.target_sesno.max(0) as u32;
    let session_index_diff::NetChangeSet {
        added,
        deleted,
        modified,
        stats,
        ..
    } = net;

    let mut window: BTreeMap<u32, Vec<EleOperationData>> = BTreeMap::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut unchanged_rewrites = 0usize;
    let mut ignored_finals: Vec<IgnoredFinalRecord> = Vec::new();
    let mut push = |window: &mut BTreeMap<u32, Vec<EleOperationData>>,
                    sesno: u32,
                    refno: RefU64,
                    detail: EleOperationDetail| {
        window
            .entry(sesno)
            .or_default()
            .push(EleOperationData::new(refno, sesno, detail));
    };

    for entry in added {
        let sesno = u32::try_from(
            entry
                .last_touch_sesno
                .ok_or_else(|| anyhow::anyhow!("净新增 {} 缺 last-touch 会话", entry.refno))?,
        )?;
        match resolve_record(resolver, entry.refno, entry.loc, "终稿") {
            Ok(data) => push(
                &mut window,
                sesno,
                entry.refno,
                EleOperationDetail::Add(data),
            ),
            Err(error) => ignored_finals.push(allowed_ignored_final(
                resolver,
                entry.refno,
                entry.loc,
                error,
            )?),
        }
    }

    for entry in deleted {
        push(
            &mut window,
            target_sesno,
            entry.refno,
            EleOperationDetail::Deleted,
        );
    }

    for entry in modified {
        let sesno = u32::try_from(
            entry
                .last_touch_sesno
                .ok_or_else(|| anyhow::anyhow!("净修改 {} 缺 last-touch 会话", entry.refno))?,
        )?;
        let latest = match resolve_record(resolver, entry.refno, entry.loc, "终稿") {
            Ok(latest) => latest,
            Err(error) => {
                ignored_finals.push(allowed_ignored_final(
                    resolver,
                    entry.refno,
                    entry.loc,
                    error,
                )?);
                continue;
            }
        };
        let base_loc = entry.base_loc.ok_or_else(|| {
            anyhow::anyhow!(
                "净修改条目 {} 缺 base 位置——classify 的不变量被破坏",
                entry.refno
            )
        })?;
        match resolve_record(resolver, entry.refno, base_loc, "基版本") {
            Ok(base) => match diff_ele_data(&base, &latest) {
                Some(modified) => push(
                    &mut window,
                    sesno,
                    entry.refno,
                    EleOperationDetail::Modified(modified),
                ),
                None => unchanged_rewrites += 1,
            },
            Err(error) => {
                warnings.push(format!(
                    "净修改 {} 的基版本解析失败，按新增全量处理（保守整根重生成）: {error:#}",
                    entry.refno
                ));
                push(
                    &mut window,
                    sesno,
                    entry.refno,
                    EleOperationDetail::Add(latest),
                );
            }
        }
    }

    if !ignored_finals.is_empty() {
        let samples = ignored_finals
            .iter()
            .take(5)
            .map(|ignored| format!("{}({}): {}", ignored.refno, ignored.noun, ignored.reason))
            .collect::<Vec<_>>()
            .join("；");
        warnings.push(format!(
            "{} 条 MNUM 系统记录终稿解析失败，按代码白名单跳过且不生成 PE 操作。样例：{samples}",
            ignored_finals.len()
        ));
    }

    Ok(NetWindowOutcome {
        snapshot_token: Some(snapshot_token),
        window,
        warnings,
        unchanged_rewrites,
        unparseable_finals: ignored_finals.len(),
        ignored_finals,
        membership_deleted: 0,
        dropped_phantom_adds: 0,
        stats,
    })
}

/// 给一次记录解析套上「谁的记录、哪一端、页与偏移」——出错时光有底层报错认不出
/// 是哪条元素的哪一端。
fn resolve_record(
    resolver: &mut impl RecordResolver,
    refno: RefU64,
    loc: RecordLoc,
    side: &str,
) -> anyhow::Result<EleData> {
    let decoded = resolver.element_at(loc).map_err(|error| {
        anyhow::anyhow!(
            "解析 {refno} 的{side}记录（页 {} 偏移 {}）失败: {error}",
            loc.pgno,
            loc.offset
        )
    })?;
    anyhow::ensure!(
        decoded.refno == refno,
        "解析 {refno} 的{side}记录（页 {} 偏移 {}）返回错误 refno {}",
        loc.pgno,
        loc.offset,
        decoded.refno
    );
    Ok(decoded)
}

/// The two-version element comparison is owned by `crate::io::diff_ele_data` and
/// re-exported above. Net-window synthesis and legacy session replay therefore share
/// one attribute / explicit-attribute / UDA / ordered-child diff implementation.
/// The cross-collector property tests below pin the rendered payload equivalence.

#[cfg(test)]
mod tests {
    use super::*;
    use aios_core::NamedAttrValue;
    use std::collections::{BTreeSet, HashMap};

    /// T14：净窗口不得再长出一份属性/成员 diff；编译期 import 保证共享函数存在，
    /// 源码断言保证这里没有悄悄复制回来一份同名实现。
    #[test]
    fn net_window_uses_the_vendor_element_diff_single_source() {
        let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/net_window.rs"));
        let shared_import = concat!("pub use crate::io::", "diff_ele_data;");
        let local_definition = concat!("pub fn diff_", "ele_data(");
        assert!(
            source.contains(shared_import),
            "净窗口必须直接复用 io 模块的共享元素 diff"
        );
        assert_eq!(
            source.matches(local_definition).count(),
            0,
            "net_window.rs 不得重新定义第二份 diff_ele_data"
        );
    }

    fn element(pairs: &[(&str, &str)], children: &[u64]) -> EleData {
        let mut data = EleData::default();
        for (name, value) in pairs {
            data.att_map_mut().map.insert(
                (*name).to_owned(),
                NamedAttrValue::StringType((*value).into()),
            );
        }
        for &child in children {
            data.children.0.push(RefU64(child));
        }
        data
    }

    #[derive(Default)]
    struct FakeMembershipResolver {
        elements: HashMap<(i32, RefU64), EleData>,
        failures: BTreeSet<(i32, RefU64)>,
    }

    impl MembershipResolver for FakeMembershipResolver {
        fn element_at(&mut self, refno: RefU64, sesno: i32) -> anyhow::Result<Option<EleData>> {
            if self.failures.contains(&(sesno, refno)) {
                anyhow::bail!("injected member parse failure for {refno}@{sesno}");
            }
            Ok(self.elements.get(&(sesno, refno)).cloned())
        }
    }

    fn owned(refno: u64, owner: u64, children: &[u64]) -> EleData {
        let mut data = element(&[("TYPE", "STRU")], children);
        data.refno = RefU64(refno);
        data.owner = RefU64(owner);
        data
    }

    fn parent_modified(
        parent: u64,
        owner: u64,
        old_children: &[u64],
        new_children: &[u64],
        sesno: u32,
    ) -> EleOperationData {
        let before = owned(parent, owner, old_children);
        let after = owned(parent, owner, new_children);
        EleOperationData::new(
            RefU64(parent),
            sesno,
            EleOperationDetail::Modified(diff_ele_data(&before, &after).expect("member delta")),
        )
    }

    fn operation_kinds(
        window: &BTreeMap<u32, Vec<EleOperationData>>,
    ) -> BTreeMap<RefU64, &'static str> {
        window
            .values()
            .flatten()
            .map(|operation| {
                let kind = match operation.detail {
                    EleOperationDetail::Add(_) => "add",
                    EleOperationDetail::Modified(_) => "modified",
                    EleOperationDetail::Deleted => "deleted",
                    EleOperationDetail::None => "none",
                };
                (operation.refno, kind)
            })
            .collect()
    }

    #[test]
    fn stale_index_record_removed_from_owner_becomes_deleted() {
        let (owner, parent, child) = (1, 10, 20);
        let mut window =
            BTreeMap::from([(2, vec![parent_modified(parent, owner, &[child], &[], 2)])]);
        let mut resolver = FakeMembershipResolver::default();
        for (sesno, data) in [
            (1, owned(parent, owner, &[child])),
            (1, owned(child, parent, &[])),
            (2, owned(parent, owner, &[])),
            // 目标索引仍可读到旧 child 记录，但目标 parent 已不再接纳它。
            (2, owned(child, parent, &[])),
        ] {
            resolver.elements.insert((sesno, data.refno), data);
        }

        let supplemented = reconcile_member_deletions(&mut window, Some(1), 2, &mut resolver)
            .expect("membership reconciliation")
            .membership_deleted;

        assert_eq!(supplemented, 1);
        assert_eq!(operation_kinds(&window)[&RefU64(parent)], "modified");
        assert_eq!(operation_kinds(&window)[&RefU64(child)], "deleted");
    }

    #[test]
    fn member_relocated_between_owners_is_not_deleted() {
        let (root, old_owner, new_owner, child) = (1, 10, 11, 20);
        let mut window = BTreeMap::from([(
            2,
            vec![
                parent_modified(old_owner, root, &[child], &[], 2),
                parent_modified(new_owner, root, &[], &[child], 2),
            ],
        )]);
        let mut resolver = FakeMembershipResolver::default();

        let supplemented = reconcile_member_deletions(&mut window, Some(1), 2, &mut resolver)
            .expect("move is decided by the two parent deltas")
            .membership_deleted;

        assert_eq!(supplemented, 0);
        assert!(!operation_kinds(&window).contains_key(&RefU64(child)));
    }

    #[test]
    fn deleted_root_expands_its_unreachable_base_subtree() {
        let (world, parent, root, leaf) = (1, 10, 20, 21);
        let mut window =
            BTreeMap::from([(2, vec![parent_modified(parent, world, &[root], &[], 2)])]);
        let mut resolver = FakeMembershipResolver::default();
        for (sesno, data) in [
            (1, owned(parent, world, &[root])),
            (1, owned(root, parent, &[leaf])),
            (1, owned(leaf, root, &[])),
            (2, owned(parent, world, &[])),
            (2, owned(root, parent, &[leaf])),
            (2, owned(leaf, root, &[])),
        ] {
            resolver.elements.insert((sesno, data.refno), data);
        }

        let supplemented = reconcile_member_deletions(&mut window, Some(1), 2, &mut resolver)
            .expect("subtree reconciliation")
            .membership_deleted;
        let kinds = operation_kinds(&window);

        assert_eq!(supplemented, 2);
        assert_eq!(kinds[&RefU64(root)], "deleted");
        assert_eq!(kinds[&RefU64(leaf)], "deleted");
    }

    #[test]
    fn descendant_relocated_out_of_a_deleted_subtree_stays_live() {
        let (world, parent, root, leaf, new_owner) = (1, 10, 20, 21, 30);
        let mut window =
            BTreeMap::from([(2, vec![parent_modified(parent, world, &[root], &[], 2)])]);
        let mut resolver = FakeMembershipResolver::default();
        for (sesno, data) in [
            (1, owned(parent, world, &[root])),
            (1, owned(root, parent, &[leaf])),
            (1, owned(leaf, root, &[])),
            (2, owned(parent, world, &[])),
            (2, owned(root, parent, &[leaf])),
            (2, owned(leaf, new_owner, &[])),
            (2, owned(new_owner, world, &[leaf])),
        ] {
            resolver.elements.insert((sesno, data.refno), data);
        }

        reconcile_member_deletions(&mut window, Some(1), 2, &mut resolver)
            .expect("moved descendant");
        let kinds = operation_kinds(&window);

        assert_eq!(kinds[&RefU64(root)], "deleted");
        assert!(!kinds.contains_key(&RefU64(leaf)));
    }

    /// 窗口塌缩：元素在窗口内诞生又被删（apply 建、restore 删，两次 SAVEWORK 落进
    /// 同一次扫描）。两端 OWNER 成员表都没有它，`children_changed` 为空，索引侧只
    /// 会判 Added。净口径要求它「不出现」——既不能留成幽灵新增，也不能发一条下游
    /// 从来没存过的 Deleted。
    #[test]
    fn add_created_and_deleted_inside_the_window_is_dropped() {
        let (owner, parent, child) = (1, 10, 20);
        let mut window = BTreeMap::from([(
            2,
            vec![EleOperationData::new(
                RefU64(child),
                2,
                EleOperationDetail::Add(owned(child, parent, &[])),
            )],
        )]);
        let mut resolver = FakeMembershipResolver::default();
        for (sesno, data) in [
            (0, owned(parent, owner, &[])),
            // 目标端仍能点查到孤儿记录，但 parent 的成员表里已经没有它。
            (2, owned(parent, owner, &[])),
            (2, owned(child, parent, &[])),
        ] {
            resolver.elements.insert((sesno, data.refno), data);
        }

        let reconciliation = reconcile_member_deletions(&mut window, Some(0), 2, &mut resolver)
            .expect("collapsed window reconciliation");

        assert_eq!(reconciliation.dropped_phantom_adds, 1);
        assert_eq!(reconciliation.membership_deleted, 0);
        assert!(!operation_kinds(&window).contains_key(&RefU64(child)));
    }

    /// 反向守卫：目标端 OWNER 的成员表里有它，那就是一次普通新增，这条收口不许碰。
    #[test]
    fn add_still_listed_by_its_target_owner_survives() {
        let (owner, parent, child) = (1, 10, 20);
        let mut window = BTreeMap::from([(
            2,
            vec![EleOperationData::new(
                RefU64(child),
                2,
                EleOperationDetail::Add(owned(child, parent, &[])),
            )],
        )]);
        let mut resolver = FakeMembershipResolver::default();
        for (sesno, data) in [
            (0, owned(parent, owner, &[])),
            (2, owned(parent, owner, &[child])),
            (2, owned(child, parent, &[])),
        ] {
            resolver.elements.insert((sesno, data.refno), data);
        }

        let reconciliation = reconcile_member_deletions(&mut window, Some(0), 2, &mut resolver)
            .expect("live add reconciliation");

        assert_eq!(reconciliation, MemberReconciliation::default());
        assert_eq!(operation_kinds(&window)[&RefU64(child)], "add");
    }

    #[test]
    fn member_arbitration_parse_failure_blocks_the_window() {
        let (owner, parent, child) = (1, 10, 20);
        let mut window =
            BTreeMap::from([(2, vec![parent_modified(parent, owner, &[child], &[], 2)])]);
        let mut resolver = FakeMembershipResolver::default();
        resolver.failures.insert((2, RefU64(child)));

        let error = reconcile_member_deletions(&mut window, Some(1), 2, &mut resolver)
            .expect_err("parse failure must block");
        assert!(error.to_string().contains("injected member parse failure"));
    }

    /// 两端逐字段相同 = 原样重写换页，真无事发生：不合成操作。
    #[test]
    fn identical_versions_diff_to_none() {
        let prev = element(&[("TYPE", "BOX"), ("XLEN", "100")], &[7]);
        let latest = element(&[("TYPE", "BOX"), ("XLEN", "100")], &[7]);
        assert!(diff_ele_data(&prev, &latest).is_none());
    }

    /// 三个桶各归各位：改值进 modified（旧值在前）、新键进 added、消失的键进
    /// deleted；noun 与 current_data 取终稿端。
    #[test]
    fn attribute_buckets_carry_old_and_new_values() {
        let prev = element(&[("TYPE", "BOX"), ("XLEN", "100"), ("GONE", "1")], &[]);
        let latest = element(&[("TYPE", "BOX"), ("XLEN", "200"), ("NEW", "9")], &[]);

        let modified = diff_ele_data(&prev, &latest).expect("有净变化");

        assert_eq!(
            modified.modified_attrs.get("XLEN"),
            Some(&(
                NamedAttrValue::StringType("100".into()),
                NamedAttrValue::StringType("200".into())
            )),
            "修改桶必须携带（旧值, 新值）"
        );
        assert_eq!(
            modified.added_attrs.get("NEW"),
            Some(&NamedAttrValue::StringType("9".into()))
        );
        assert_eq!(
            modified.deleted_attrs.get("GONE"),
            Some(&NamedAttrValue::StringType("1".into()))
        );
        assert!(modified.children_changed.is_none());
    }

    /// 纯 children 变化（含重排）也必须发 Modified——渲染端靠 children_changed
    /// 做 pe_owner 全量替换，成员增删的信号只在这里。
    #[test]
    fn children_only_change_still_emits_modified() {
        let prev = element(&[("TYPE", "ZONE")], &[1, 2]);
        let latest = element(&[("TYPE", "ZONE")], &[2, 1]);

        let modified = diff_ele_data(&prev, &latest).expect("children 重排是净变化");
        let (old, new) = modified.children_changed.expect("children 两端都要带");
        assert_eq!(old.0, vec![RefU64(1), RefU64(2)]);
        assert_eq!(new.0, vec![RefU64(2), RefU64(1)]);
        assert!(modified.added_attrs.is_empty());
    }

    // ── 纯合成层：三形状 + 两条降级 + 一条硬失败 + 原样重写（ADR-022 验收 1）──
    //
    // 注入 resolver 之后这些分支全部不碰文件、不碰库，是 CI 里常驻的那一份；
    // 真实负载的等价性另有 `db8000_session_pairs` 性质 i 与 live 对拍兜底。

    use crate::session_index_diff::{NetChangeSet, NetChangeStats, NetEntry};

    fn at(pgno: u32, offset: u32) -> RecordLoc {
        RecordLoc { pgno, offset }
    }

    fn net_entry(
        refno: u64,
        loc: RecordLoc,
        base_loc: Option<RecordLoc>,
        last_touch_sesno: Option<i32>,
    ) -> NetEntry {
        NetEntry {
            refno: RefU64(refno),
            loc,
            base_loc,
            last_touch_sesno,
            noun: None,
        }
    }

    fn change_set(target_sesno: i32) -> NetChangeSet {
        NetChangeSet {
            requested_start: 1,
            requested_end: target_sesno,
            base_sesno: Some(1),
            target_sesno,
            added: Vec::new(),
            deleted: Vec::new(),
            modified: Vec::new(),
            stats: NetChangeStats::default(),
        }
    }

    /// 只认位置的记录桩：桩里没有的位置就是解析失败——真实库里那是字典缺项的
    /// 系统记录（`MNUM not exist in attr_info_map`）。
    fn records(
        known: Vec<(RecordLoc, u64, EleData)>,
    ) -> impl FnMut(RecordLoc) -> anyhow::Result<EleData> {
        move |wanted| {
            known
                .iter()
                .find(|(loc, _, _)| *loc == wanted)
                .map(|(_, refno, data)| {
                    let mut data = data.clone();
                    data.refno = RefU64(*refno);
                    data
                })
                .ok_or_else(|| anyhow::anyhow!("MNUM not exist in attr_info_map"))
        }
    }

    /// 净新增挂在它的 last-touch 会话上，不是窗口终点；`stats` 原样移交回执。
    #[test]
    fn a_net_added_entry_becomes_an_add_on_its_last_touch_session() {
        let mut net = change_set(30);
        net.added.push(net_entry(7, at(10, 0), None, Some(12)));
        net.stats.elapsed_ms = 42;

        let outcome = synthesize_net_window(
            net,
            records(vec![(at(10, 0), 7, element(&[("TYPE", "BOX")], &[]))]),
        )
        .expect("合成");

        let ops = outcome.window.get(&12).expect("挂 last-touch 会话 12");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].refno, RefU64(7));
        assert_eq!(ops[0].sesno, 12);
        match &ops[0].detail {
            EleOperationDetail::Add(data) => assert_eq!(data.att_map().get_type(), "BOX"),
            other => panic!("净新增必须合成 Add，得到 {other:?}"),
        }
        assert!(
            !outcome.window.contains_key(&30),
            "不得挂到窗口终点会话上：{:?}",
            outcome.window.keys().collect::<Vec<_>>()
        );
        assert!(outcome.warnings.is_empty());
        assert_eq!(outcome.stats.elapsed_ms, 42, "差分统计要原样带进回执");
    }

    /// 净删除挂窗口终点会话——净差分判不出删除动作发生在哪个会话，`last_touch`
    /// 说的是**旧版本**所在会话，拿它当删除时刻就是编一个看着像真的数。
    /// 顺带钉住：删除不解析任何记录。
    #[test]
    fn a_net_deleted_entry_hangs_on_the_window_end_session() {
        let mut net = change_set(30);
        net.deleted.push(net_entry(8, at(11, 4), None, Some(12)));

        let outcome = synthesize_net_window(net, records(Vec::new())).expect("合成");

        assert!(
            !outcome.window.contains_key(&12),
            "不得挂在旧版本所在会话上"
        );
        let ops = outcome.window.get(&30).expect("挂窗口终点会话 30");
        assert!(matches!(ops[0].detail, EleOperationDetail::Deleted));
        assert_eq!(ops[0].sesno, 30);
        assert_eq!(
            outcome.unparseable_finals, 0,
            "删除条目不该去解析记录（桩是空的，解析了就会计数）"
        );
    }

    /// 净修改：两端各解析**恰一次**（终稿在前、基版本在后），一次 diff 合成。
    #[test]
    fn a_net_modified_entry_diffs_both_versions_exactly_once() {
        let mut net = change_set(30);
        net.modified
            .push(net_entry(9, at(20, 0), Some(at(19, 0)), Some(25)));
        let base = element(&[("TYPE", "BOX"), ("XLEN", "100")], &[]);
        let latest = element(&[("TYPE", "BOX"), ("XLEN", "200")], &[]);

        let mut seen: Vec<RecordLoc> = Vec::new();
        let outcome = synthesize_net_window(net, |wanted| {
            seen.push(wanted);
            if wanted == at(20, 0) {
                let mut latest = latest.clone();
                latest.refno = RefU64(9);
                Ok(latest)
            } else if wanted == at(19, 0) {
                let mut base = base.clone();
                base.refno = RefU64(9);
                Ok(base)
            } else {
                anyhow::bail!("桩里没有 {wanted:?}")
            }
        })
        .expect("合成");

        assert_eq!(
            seen,
            vec![at(20, 0), at(19, 0)],
            "两端各解析一次、终稿在前；多解析一次就是白付一趟记录解析"
        );
        let ops = outcome.window.get(&25).expect("挂 last-touch 会话 25");
        match &ops[0].detail {
            EleOperationDetail::Modified(modified) => assert_eq!(
                modified.modified_attrs.get("XLEN"),
                Some(&(
                    NamedAttrValue::StringType("100".into()),
                    NamedAttrValue::StringType("200".into())
                ))
            ),
            other => panic!("净修改必须合成 Modified，得到 {other:?}"),
        }
        assert_eq!(outcome.unchanged_rewrites, 0);
        assert!(outcome.warnings.is_empty());
    }

    /// 基版本读不出来 = 拿不到差集，按新增全量覆盖（宁多勿漏，模型侧整根重生成），
    /// 并逐条点名——降级可以，静默不行。
    #[test]
    fn a_base_parse_failure_degrades_to_add_and_names_the_refno() {
        let mut net = change_set(30);
        net.modified
            .push(net_entry(9, at(20, 0), Some(at(19, 0)), Some(25)));

        let outcome = synthesize_net_window(
            net,
            records(vec![(at(20, 0), 9, element(&[("TYPE", "BOX")], &[]))]),
        )
        .expect("合成");

        let ops = outcome.window.get(&25).expect("降级后照样要有操作");
        assert!(
            matches!(ops[0].detail, EleOperationDetail::Add(_)),
            "基版本读不了就整条按新增覆盖，不许退化成不发操作"
        );
        assert_eq!(outcome.warnings.len(), 1);
        let warning = &outcome.warnings[0];
        assert!(
            warning.contains(&RefU64(9).to_string()) && warning.contains("基版本"),
            "降级警告必须点名 refno 与降级原因: {warning}"
        );
        assert_eq!(outcome.unparseable_finals, 0, "失败的是基版本不是终稿");
    }

    struct FailedFinalResolver {
        identities: Vec<(RecordLoc, RawElementIdentity)>,
    }

    impl RecordResolver for FailedFinalResolver {
        fn element_at(&mut self, _loc: RecordLoc) -> anyhow::Result<EleData> {
            anyhow::bail!("injected canonical decode failure")
        }

        fn identity_at(&mut self, loc: RecordLoc) -> anyhow::Result<RawElementIdentity> {
            self.identities
                .iter()
                .find(|(candidate, _)| *candidate == loc)
                .map(|(_, identity)| identity.clone())
                .ok_or_else(|| anyhow::anyhow!("identity missing"))
        }
    }

    fn identity(refno: u64, noun_hash: i32, noun_name: &str) -> RawElementIdentity {
        RawElementIdentity {
            refno: RefU64(refno),
            noun_hash,
            noun_name: noun_name.to_owned(),
            owner: RefU64::default(),
        }
    }

    /// Only MNUM may be omitted after a canonical final decode failure.
    #[test]
    fn mnum_final_failure_is_skipped_counted_and_aggregated() {
        let mut net = change_set(30);
        net.added.push(net_entry(7, at(10, 0), None, Some(12)));
        net.modified
            .push(net_entry(9, at(20, 0), Some(at(19, 0)), Some(25)));
        let mut resolver = FailedFinalResolver {
            identities: vec![
                (at(10, 0), identity(7, 0xC40CC, "MNUM")),
                (at(20, 0), identity(9, 0xC40CC, "MNUM")),
            ],
        };

        let outcome =
            synthesize_net_window_with_resolver(net, SnapshotToken::for_test(), &mut resolver)
                .expect("MNUM 合成");

        assert!(
            outcome.window.is_empty(),
            "解析不出终稿的条目一条都不许入窗口: {:?}",
            outcome.window.keys().collect::<Vec<_>>()
        );
        assert_eq!(outcome.unparseable_finals, 2);
        assert!(
            outcome
                .ignored_finals
                .iter()
                .all(|item| item.noun == "MNUM")
        );
        assert_eq!(outcome.warnings.len(), 1, "明细走聚合警告，不逐条刷屏");
        let warning = &outcome.warnings[0];
        assert!(warning.contains("2 条"), "聚合警告要报条数: {warning}");
        assert!(
            warning.contains(&RefU64(7).to_string()) && warning.contains(&RefU64(9).to_string()),
            "聚合警告要带样例 refno: {warning}"
        );
    }

    #[test]
    fn ordinary_noun_final_failure_rejects_the_whole_window() {
        let mut net = change_set(30);
        net.added.push(net_entry(7, at(10, 0), None, Some(12)));
        let mut resolver = FailedFinalResolver {
            identities: vec![(at(10, 0), identity(7, 0x861E0, "BOX"))],
        };

        let error =
            synthesize_net_window_with_resolver(net, SnapshotToken::for_test(), &mut resolver)
                .expect_err("非白名单终稿必须失败");

        assert!(format!("{error:#}").contains("非白名单终稿"));
    }

    #[test]
    fn resolver_returning_the_wrong_refno_is_a_hard_failure() {
        let mut net = change_set(30);
        net.added.push(net_entry(7, at(10, 0), None, Some(12)));
        let error = synthesize_net_window(
            net,
            records(vec![(at(10, 0), 99, element(&[("TYPE", "BOX")], &[]))]),
        )
        .expect_err("错误 refno 必须失败");

        assert!(format!("{error:#}").contains("错误 refno"));
    }

    #[test]
    fn a_missing_last_touch_session_fails_instead_of_using_the_window_end() {
        let mut net = change_set(30);
        net.added.push(net_entry(7, at(10, 0), None, None));
        let error = synthesize_net_window(
            net,
            records(vec![(at(10, 0), 7, element(&[("TYPE", "BOX")], &[]))]),
        )
        .err()
        .expect("last-touch 缺失必须整窗失败");
        assert!(format!("{error:#}").contains("last-touch"));
    }

    /// `base_loc` 缺失是**差分分类的不变量被破坏**，不是现场异常：硬失败整批，
    /// 不许按新增或跳过降级——那会把一个逻辑缺陷变成一批悄悄错的数据。
    #[test]
    fn a_missing_base_loc_fails_hard_and_names_the_refno() {
        let mut net = change_set(30);
        net.modified.push(net_entry(9, at(20, 0), None, Some(25)));

        let outcome = synthesize_net_window(
            net,
            records(vec![(at(20, 0), 9, element(&[("TYPE", "BOX")], &[]))]),
        );

        let Err(error) = outcome else {
            panic!("缺 base 位置必须硬失败，不许降级成一条看着像真的操作");
        };
        let text = format!("{error:#}");
        assert!(
            text.contains(&RefU64(9).to_string()) && text.contains("不变量"),
            "硬失败必须点名 refno 与原因: {text}"
        );
    }

    /// 记录换了页但两端逐字段相同（Save Work 原样重写）：不发操作——这不是降级，
    /// 是正常判定的正常结果；但账要看得见。
    #[test]
    fn an_identical_rewrite_emits_nothing_but_is_counted() {
        let mut net = change_set(30);
        net.modified
            .push(net_entry(9, at(20, 0), Some(at(19, 0)), Some(25)));
        let same = element(&[("TYPE", "BOX"), ("XLEN", "100")], &[7]);

        let outcome = synthesize_net_window(
            net,
            records(vec![(at(20, 0), 9, same.clone()), (at(19, 0), 9, same)]),
        )
        .expect("合成");

        assert!(outcome.window.is_empty(), "两端相同 = 真无事发生");
        assert_eq!(outcome.unchanged_rewrites, 1);
        assert!(
            outcome.warnings.is_empty(),
            "原样重写不是降级路径，不该有警告: {:?}",
            outcome.warnings
        );
    }

    // 与逐会话回放的 live 负载对拍、以及 T18 release 计时，都留在 aios-database：
    // 参照臂 `IncrementPipeline::collect_changes` 与计时对象 `collect_window`
    // 都在上一层（前者对 Save Work 终稿还有两个补丁，后者含批次口径拼装）。
    // 见 `increment_pipeline.rs` 的
    // `live_ams8000_net_window_payloads_match_replay_on_single_touch_refnos`
    // 与 `live_ams8000_single_caliber_release_timing`。

    /// 纯文件纪律钉死（与 session_index_diff 同款）：净窗口收集不许出现任何
    /// 数据库访问；窗口由调用方给定，不读水位。
    #[test]
    fn the_net_window_module_never_touches_the_database() {
        let source = include_str!("net_window.rs");
        let forbidden = [
            concat!("SUL", "_DB"),
            concat!(".que", "ry("),
            concat!("surreal", "db"),
        ];
        for needle in forbidden {
            assert_eq!(
                source.matches(needle).count(),
                0,
                "净窗口收集必须纯文件：源码里不得出现 {needle}"
            );
        }
    }
}
