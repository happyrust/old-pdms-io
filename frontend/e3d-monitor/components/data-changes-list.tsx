"use client"

import { useState, useEffect } from "react"
import { PlusCircle, MinusCircle, Edit, Clock, User, Search, ChevronDown, Hash } from "lucide-react"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Badge } from "@/components/ui/badge"
import { getElementChanges } from "@/lib/surrealdb"

interface DataChangesListProps {
  limit?: number
}

// 定义变更记录类型
interface ChangeRecord {
  id: string
  refno: string
  operation_type: string
  entity_type: string
  timestamp: string
  sesno: number
  project?: string
  session_number?: number
  details?: any
}

export function DataChangesList({ limit = 20 }: DataChangesListProps) {
  const [searchTerm, setSearchTerm] = useState("")
  const [filterType, setFilterType] = useState<string | null>(null)
  const [changes, setChanges] = useState<ChangeRecord[]>([])
  const [loading, setLoading] = useState(true)
  const [offset, setOffset] = useState(0)
  const [hasMore, setHasMore] = useState(true)

  // 加载数据
  useEffect(() => {
    const fetchData = async () => {
      setLoading(true)
      try {
        const data = await getElementChanges(limit, offset)
        setChanges(prevChanges => offset === 0 ? data : [...prevChanges, ...data])
        setHasMore(data.length === limit)
      } catch (error) {
        console.error("获取变更数据失败:", error)
      } finally {
        setLoading(false)
      }
    }

    fetchData()
  }, [limit, offset])

  const formatDate = (dateString: string) => {
    const date = new Date(dateString)
    return new Intl.DateTimeFormat("zh-CN", {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    }).format(date)
  }

  const getTypeIcon = (type: string) => {
    switch (type) {
      case "新增":
        return <PlusCircle className="h-4 w-4 text-green-500" />
      case "删除":
        return <MinusCircle className="h-4 w-4 text-red-500" />
      case "修改":
        return <Edit className="h-4 w-4 text-yellow-500" />
      default:
        return null
    }
  }

  const getTypeBadge = (type: string) => {
    switch (type) {
      case "新增":
        return <Badge className="bg-green-500 hover:bg-green-600">新增</Badge>
      case "删除":
        return <Badge className="bg-red-500 hover:bg-red-600">删除</Badge>
      case "修改":
        return <Badge className="bg-yellow-500 hover:bg-yellow-600">修改</Badge>
      default:
        return null
    }
  }

  const filteredChanges = changes
    .filter((change) => {
      if (filterType && change.operation_type !== filterType) return false
      if (
        searchTerm &&
        !change.entity_type.toLowerCase().includes(searchTerm.toLowerCase()) &&
        !change.refno.toLowerCase().includes(searchTerm.toLowerCase())
      ) {
        return false
      }
      return true
    })

  const loadMore = () => {
    setOffset(prevOffset => prevOffset + limit)
  }

  return (
    <div className="space-y-4">
      <div className="flex flex-col space-y-2 md:flex-row md:space-x-2 md:space-y-0">
        <div className="relative flex-1">
          <Search className="absolute left-2 top-1/2 h-4 w-4 -translate-y-1/2 transform text-muted-foreground" />
          <Input
            placeholder="搜索参考号或实体类型..."
            value={searchTerm}
            onChange={(e) => setSearchTerm(e.target.value)}
            className="pl-8"
          />
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="outline" className="ml-auto">
              {filterType ? `类型: ${filterType}` : "所有类型"}
              <ChevronDown className="ml-2 h-4 w-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onClick={() => setFilterType(null)}>所有类型</DropdownMenuItem>
            <DropdownMenuItem onClick={() => setFilterType("新增")}>新增</DropdownMenuItem>
            <DropdownMenuItem onClick={() => setFilterType("修改")}>修改</DropdownMenuItem>
            <DropdownMenuItem onClick={() => setFilterType("删除")}>删除</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      {loading && changes.length === 0 ? (
        <div className="flex items-center justify-center py-8">
          <div className="text-center">
            <p className="text-muted-foreground">加载中...</p>
          </div>
        </div>
      ) : filteredChanges.length === 0 ? (
        <div className="flex items-center justify-center py-8">
          <div className="text-center">
            <p className="text-muted-foreground">暂无符合条件的数据变更记录</p>
          </div>
        </div>
      ) : (
        <>
          <div className="rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-[80px]">类型</TableHead>
                  <TableHead className="w-[120px]">参考号</TableHead>
                  <TableHead>实体类型</TableHead>
                  <TableHead className="w-[90px]">会话号</TableHead>
                  <TableHead className="w-[180px]">时间</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filteredChanges.map((change) => (
                  <TableRow key={change.id}>
                    <TableCell>
                      <div className="flex items-center gap-2">
                        {getTypeIcon(change.operation_type)}
                        {getTypeBadge(change.operation_type)}
                      </div>
                    </TableCell>
                    <TableCell className="font-mono">{change.refno}</TableCell>
                    <TableCell>{change.entity_type}</TableCell>
                    <TableCell>
                      <div className="flex items-center gap-1">
                        <Hash className="h-3 w-3 text-muted-foreground" />
                        <span className="text-xs">{change.project || ""}</span>
                        <span>{change.sesno}</span>
                      </div>
                    </TableCell>
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

          {hasMore && (
            <div className="flex justify-center">
              <Button variant="outline" onClick={loadMore} disabled={loading}>
                {loading ? "加载中..." : "加载更多"}
              </Button>
            </div>
          )}
        </>
      )}
    </div>
  )
}
