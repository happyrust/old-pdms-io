import Surreal from 'surrealdb';

// SurrealDB连接配置
const SURREAL_URL = process.env.NEXT_PUBLIC_SURREAL_URL || 'http://127.0.0.1:8000/rpc';
const NAMESPACE = process.env.NEXT_PUBLIC_SURREAL_NS || '1516';
const DATABASE = process.env.NEXT_PUBLIC_SURREAL_DB || 'AvevaMarineSample';
const USERNAME = process.env.NEXT_PUBLIC_SURREAL_USER || 'root';
const PASSWORD = process.env.NEXT_PUBLIC_SURREAL_PASS || 'root';

// 创建SurrealDB连接单例
let db: Surreal | null = null;

/**
 * 初始化SurrealDB连接
 */
export async function initSurrealDB() {
  if (!db) {
    db = new Surreal();
    
    try {
      // 连接到SurrealDB
      await db.connect(SURREAL_URL);
      
      // 使用指定的namespace和database
      await db.use({
        namespace: NAMESPACE,
        database: DATABASE
      });
      
      // 登录账号
      await db.signin({
        username: USERNAME,
        password: PASSWORD,
      });
      
      console.log('SurrealDB连接成功');
    } catch (err) {
      console.error('SurrealDB连接失败:', err);
      db = null;
    }
  }
  
  return db;
}

/**
 * 获取SurrealDB连接实例
 */
export async function getDB() {
  if (!db) {
    await initSurrealDB();
  }
  return db;
}

/**
 * 获取元素变更记录
 * @param limit 限制返回的记录数量
 * @param offset 偏移量
 */
export async function getElementChanges(limit = 20, offset = 0) {
  const db = await getDB();
  if (!db) return [];
  
  try {
    const result = await db.query(`
      SELECT 
        id,
        refno,
        operation_type,
        entity_type,
        timestamp,
        sesno,
        session_id.project as project,
        session_id.sesno as session_number,
        details
      FROM element_changes
      ORDER BY timestamp DESC
      LIMIT $limit
      START $offset
    `, {
      limit,
      offset,
    });
    
    return Array.isArray(result) ? result : [];
  } catch (err) {
    console.error('获取元素变更记录失败:', err);
    return [];
  }
}

/**
 * 获取最近一段时间内的变更统计
 * @param days 天数
 */
export async function getRecentChangesStats(days = 7) {
  const db = await getDB();
  if (!db) return [];
  
  try {
    const result = await db.query(`
      SELECT 
        time::date(timestamp) AS date,
        count(id) AS total,
        count(id WHERE operation_type = '新增') AS add_count,
        count(id WHERE operation_type = '修改') AS modify_count,
        count(id WHERE operation_type = '删除') AS delete_count
      FROM element_changes
      WHERE timestamp > time::now() - ${days}d
      GROUP BY time::date(timestamp)
      ORDER BY date
    `);
    
    return Array.isArray(result) ? result : [];
  } catch (err) {
    console.error('获取变更统计失败:', err);
    return [];
  }
}

/**
 * 根据参考号获取元素的所有变更历史
 * @param refno 参考号
 */
export async function getElementHistory(refno: string) {
  const db = await getDB();
  if (!db) return [];
  
  try {
    const result = await db.query(`
      SELECT 
        id,
        refno,
        operation_type,
        entity_type,
        timestamp,
        sesno,
        session_id,
        details
      FROM element_changes
      WHERE refno = $refno
      ORDER BY timestamp DESC
    `, {
      refno,
    });
    
    return Array.isArray(result) ? result : [];
  } catch (err) {
    console.error('获取元素历史记录失败:', err);
    return [];
  }
}

/**
 * 获取变更总览数据
 */
export async function getChangesOverview() {
  const db = await getDB();
  if (!db) return null;
  
  try {
    // 获取今日变更统计
    const todayResult = await db.query(`
      SELECT 
        count(id) AS total,
        count(id WHERE operation_type = '新增') AS add_count,
        count(id WHERE operation_type = '修改') AS modify_count,
        count(id WHERE operation_type = '删除') AS delete_count
      FROM element_changes
      WHERE timestamp > time::date(time::now())
    `);
    
    // 获取昨日变更统计
    const yesterdayResult = await db.query(`
      SELECT 
        count(id) AS total,
        count(id WHERE operation_type = '新增') AS add_count,
        count(id WHERE operation_type = '修改') AS modify_count,
        count(id WHERE operation_type = '删除') AS delete_count
      FROM element_changes
      WHERE timestamp > time::date(time::now() - 1d) AND timestamp < time::date(time::now())
    `);
    
    // 获取总数据量
    const totalResult = await db.query(`
      SELECT count() AS total FROM pe
    `);
    
    // 默认值
    const defaultStats = { total: 0, add_count: 0, modify_count: 0, delete_count: 0 };
    
    // 安全地获取查询结果
    const today = Array.isArray(todayResult) && todayResult.length > 0 ? 
      (todayResult[0] as any || defaultStats) : defaultStats;
    
    const yesterday = Array.isArray(yesterdayResult) && yesterdayResult.length > 0 ? 
      (yesterdayResult[0] as any || defaultStats) : defaultStats;
    
    const total = Array.isArray(totalResult) && totalResult.length > 0 && totalResult[0] ? 
      ((totalResult[0] as any).total || 0) : 0;
    
    return {
      today,
      yesterday,
      total,
    };
  } catch (err) {
    console.error('获取变更总览数据失败:', err);
    return null;
  }
}

/**
 * 获取所有会话信息
 */
export async function getSessions() {
  const db = await getDB();
  if (!db) return [];
  
  try {
    const result = await db.query(`
      SELECT 
        id,
        sesno,
        project,
        timestamp,
        dbnum,
        add_count,
        modify_count,
        delete_count
      FROM sessions
      ORDER BY sesno DESC
    `);
    
    return Array.isArray(result) ? result : [];
  } catch (err) {
    console.error('获取会话信息失败:', err);
    return [];
  }
}

/**
 * 根据会话ID获取该会话中的所有变更
 * @param sessionId 会话ID
 */
export async function getChangesBySession(sessionId: string) {
  const db = await getDB();
  if (!db) return [];
  
  try {
    const result = await db.query(`
      SELECT 
        id,
        refno,
        operation_type,
        entity_type,
        timestamp,
        sesno,
        details
      FROM element_changes
      WHERE session_id = type::thing('sessions', $sessionId)
      ORDER BY timestamp DESC
    `, {
      sessionId,
    });
    
    return Array.isArray(result) ? result : [];
  } catch (err) {
    console.error('获取会话变更记录失败:', err);
    return [];
  }
} 