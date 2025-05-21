"use client"

import { useState, useEffect, useMemo } from "react"
import { Clock, BookOpen, Hash, Search } from "lucide-react"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import { getSessions, getChangesBySession, checkDatabaseConnection, isDatabaseConnected } from "@/lib/surrealdb"
import { Badge } from "@/components/ui/badge"
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog"

interface Session {
  id: string
  sesno: number
  project: string
  timestamp: string
  dbnum: number
  add_count: number
  modify_count: number
  delete_count: number
}

interface SessionChangeRecord {
  id: string
  refno: string
  operation_type: string
  entity_type: string
  timestamp: string
  sesno: number
  details?: any
}

export function SessionList() {
  const [sessions, setSessions] = useState<Session[]>([])
  const [loading, setLoading] = useState(true)
  const [searchTerm, setSearchTerm] = useState("")
  const [selectedSession, setSelectedSession] = useState<Session | null>(null)
  const [sessionChanges, setSessionChanges] = useState<SessionChangeRecord[]>([])
  const [loadingChanges, setLoadingChanges] = useState(false)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [dbConnected, setDbConnected] = useState(false)
  const [connectionError, setConnectionError] = useState<string | null>(null)

  // 检查数据库连接状态
  useEffect(() => {
    let isMounted = true;
    const checkConnection = async () => {
      try {
        const isConnected = await checkDatabaseConnection()
        if (isMounted) {
          setDbConnected(isConnected)
          setConnectionError(null)
        }
      } catch (error) {
        if (isMounted) {
          setDbConnected(false)
          setConnectionError("无法连接到数据库，请检查连接配置")
          console.error("数据库连接检查失败:", error)
        }
      }
    }

    checkConnection()
    // 每30秒检查一次连接状态
    const interval = setInterval(checkConnection, 30000)
    return () => {
      isMounted = false;
      clearInterval(interval)
    }
  }, [])

  // 加载会话数据
  useEffect(() => {
    let isMounted = true;
    const fetchData = async () => {
      if (!dbConnected) {
        return
      }
      
      setLoading(true)
      try {
        const data = await getSessions()
        if (isMounted) {
          setSessions(data as unknown as Session[])
        }
      } catch (error) {
        console.error("获取会话数据失败:", error)
      } finally {
        if (isMounted) {
          setLoading(false)
        }
      }
    }

    if (dbConnected) {
      fetchData()
    }
    
    return () => {
      isMounted = false;
    }
  }, [dbConnected])

  // 查看会话变更详情
  const viewSessionChanges = async (session: Session) => {
    if (!dbConnected) {
      setConnectionError("数据库未连接，无法获取会话详情")
      return
    }
    
    setSelectedSession(session)
    setLoadingChanges(true)
    setDialogOpen(true)
    
    try {
      const sessionId = session.id.split(':')[1]
      const changes = await getChangesBySession(sessionId)
      setSessionChanges(changes as unknown as SessionChangeRecord[])
    } catch (error) {
      console.error("获取会话变更数据失败:", error)
    } finally {
      setLoadingChanges(false)
    }
  }

  const formatDate = (dateString: string): string => {
    if (!dateString) {
      return "日期不可用";
    }
    const date = new Date(dateString);
    if (isNaN(date.getTime())) {
      console.warn(`Invalid time value received in SessionList: ${dateString}`);
      return "无效日期";
    }
    return new Intl.DateTimeFormat("zh-CN", {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    }).format(date);
  };

  // 使用useMemo缓存过滤后的会话数据，避免每次渲染时重新计算
  const filteredSessions = useMemo(() => {
    return sessions.filter((session) => {
      if (
        searchTerm &&
        !session.project.toLowerCase().includes(searchTerm.toLowerCase()) &&
        !session.sesno.toString().includes(searchTerm)
      ) {
        return false
      }
      return true
    })
  }, [sessions, searchTerm])

  return (
    <div className="space-y-4">
      <div className="relative flex-1 mb-4">
        <Search className="absolute left-2 top-1/2 h-4 w-4 -translate-y-1/2 transform text-muted-foreground" />
        <Input
          placeholder="搜索项目或会话号..."
          value={searchTerm}
          onChange={(e) => setSearchTerm(e.target.value)}
          className="pl-8"
        />
      </div>

      {connectionError && (
        <div className="flex items-center justify-center py-4">
          <div className="text-center bg-red-50 p-4 rounded-md border border-red-200">
            <p className="text-red-500">{connectionError}</p>
          </div>
        </div>
      )}

      {loading ? (
        <div className="flex items-center justify-center py-8">
          <div className="text-center">
            <p className="text-muted-foreground">加载中...</p>
          </div>
        </div>
      ) : !dbConnected ? (
        <div className="flex items-center justify-center py-8">
          <div className="text-center">
            <p className="text-muted-foreground">等待数据库连接...</p>
          </div>
        </div>
      ) : filteredSessions.length === 0 ? (
        <div className="flex items-center justify-center py-8">
          <div className="text-center">
            <p className="text-muted-foreground">暂无会话记录</p>
          </div>
        </div>
      ) : (
        <>
          <div className="rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-[100px]">会话号</TableHead>
                  <TableHead className="w-[120px]">项目</TableHead>
                  <TableHead className="w-[180px]">时间</TableHead>
                  <TableHead className="w-[180px]">变更统计</TableHead>
                  <TableHead className="w-[100px]">操作</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filteredSessions.map((session, index) => (
                  <TableRow key={`${session.id}-${index}`}>
                    <TableCell>
                      <div className="flex items-center gap-1">
                        <Hash className="h-3 w-3 text-muted-foreground" />
                        {session.sesno}
                      </div>
                    </TableCell>
                    <TableCell>{session.project}</TableCell>
                    <TableCell>
                      <div className="flex items-center gap-1">
                        <Clock className="h-3 w-3 text-muted-foreground" />
                        {formatDate(session.timestamp)}
                      </div>
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-2">
                        <Badge className="bg-green-500">{session.add_count}新增</Badge>
                        <Badge className="bg-yellow-500">{session.modify_count}修改</Badge>
                        <Badge className="bg-red-500">{session.delete_count}删除</Badge>
                      </div>
                    </TableCell>
                    <TableCell>
                      <Button variant="outline" size="sm" onClick={() => viewSessionChanges(session)}>
                        <BookOpen className="h-4 w-4 mr-1" />
                        查看详情
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>

          <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
            <DialogContent className="max-w-4xl max-h-[80vh] overflow-y-auto">
              <DialogHeader>
                <DialogTitle>
                  会话 {selectedSession?.project} #{selectedSession?.sesno} 的变更详情
                </DialogTitle>
              </DialogHeader>
              
              {loadingChanges ? (
                <div className="flex items-center justify-center py-8">
                  <div className="text-center">
                    <p className="text-muted-foreground">加载中...</p>
                  </div>
                </div>
              ) : sessionChanges.length === 0 ? (
                <div className="flex items-center justify-center py-8">
                  <div className="text-center">
                    <p className="text-muted-foreground">该会话中没有变更记录</p>
                  </div>
                </div>
              ) : (
                <div className="rounded-md border">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead className="w-[100px]">操作类型</TableHead>
                        <TableHead className="w-[150px]">参考号</TableHead>
                        <TableHead>实体类型</TableHead>
                        <TableHead className="w-[180px]">时间</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {sessionChanges.map((change, index) => (
                        <TableRow key={`${change.id}-${index}`}>
                          <TableCell>
                            <Badge className={
                              change.operation_type === "新增" ? "bg-green-500" : 
                              change.operation_type === "修改" ? "bg-yellow-500" : 
                              "bg-red-500"
                            }>
                              {change.operation_type}
                            </Badge>
                          </TableCell>
                          <TableCell className="font-mono">{change.refno}</TableCell>
                          <TableCell>{change.entity_type}</TableCell>
                          <TableCell>
                            <div className="flex items-center gap-1">
                              <Clock className="h-3 w-3 text-muted-foreground" />
                              {formatDate(change.timestamp)}
                            </div>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              )}
            </DialogContent>
          </Dialog>
        </>
      )}
    </div>
  )
} 