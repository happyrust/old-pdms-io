"use client"

import { useState } from "react"
import { Calendar, ArrowRight, PlusCircle, MinusCircle, Edit, Search, SplitSquareVertical } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Calendar as CalendarComponent } from "@/components/ui/calendar"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { Badge } from "@/components/ui/badge"

// 模拟数据 - 版本对比数据
const mockVersions = [
  { id: "v1", label: "2025-05-09 09:23:45" },
  { id: "v2", label: "2025-05-09 14:15:30" },
  { id: "v3", label: "2025-05-08 10:30:22" },
  { id: "v4", label: "2025-05-08 16:45:12" },
  { id: "v5", label: "2025-05-07 11:20:33" },
  { id: "v6", label: "2025-05-06 09:15:45" },
  { id: "v7", label: "2025-05-06 15:30:10" },
  { id: "v8", label: "2025-05-05 10:45:22" },
]

// 模拟数据 - 对比结果
const mockComparisonResults = [
  {
    id: "E001",
    name: "管道组件A",
    category: "管道",
    changeType: "modify",
    oldValue: "直径: 100mm, 材质: 碳钢",
    newValue: "直径: 120mm, 材质: 不锈钢",
  },
  {
    id: "E002",
    name: "阀门B",
    category: "阀门",
    changeType: "modify",
    oldValue: "型号: V-100, 压力等级: PN16",
    newValue: "型号: V-120, 压力等级: PN25",
  },
  {
    id: "E003",
    name: "连接件C",
    category: "连接件",
    changeType: "delete",
    oldValue: "型号: C-50, 材质: 铝合金",
    newValue: "",
  },
  {
    id: "E004",
    name: "支架D",
    category: "支架",
    changeType: "add",
    oldValue: "",
    newValue: "型号: S-30, 承重: 500kg",
  },
  {
    id: "E005",
    name: "法兰E",
    category: "法兰",
    changeType: "modify",
    oldValue: "直径: 150mm, 孔数: 8",
    newValue: "直径: 150mm, 孔数: 12",
  },
]

export function DataVersionComparison() {
  const [fromVersion, setFromVersion] = useState<string>("v5")
  const [toVersion, setToVersion] = useState<string>("v1")
  const [fromDate, setFromDate] = useState<Date | undefined>(new Date("2025-05-07"))
  const [toDate, setToDate] = useState<Date | undefined>(new Date("2025-05-09"))
  const [searchTerm, setSearchTerm] = useState("")
  const [filterType, setFilterType] = useState<string | null>(null)

  const getChangeTypeIcon = (type: string) => {
    switch (type) {
      case "add":
        return <PlusCircle className="h-4 w-4 text-green-500" />
      case "delete":
        return <MinusCircle className="h-4 w-4 text-red-500" />
      case "modify":
        return <Edit className="h-4 w-4 text-yellow-500" />
      default:
        return null
    }
  }

  const getChangeTypeLabel = (type: string) => {
    switch (type) {
      case "add":
        return "新增"
      case "delete":
        return "删除"
      case "modify":
        return "修改"
      default:
        return ""
    }
  }

  const filteredResults = mockComparisonResults.filter((result) => {
    if (filterType && result.changeType !== filterType) return false
    if (
      searchTerm &&
      !result.name.toLowerCase().includes(searchTerm.toLowerCase()) &&
      !result.id.toLowerCase().includes(searchTerm.toLowerCase()) &&
      !result.category.toLowerCase().includes(searchTerm.toLowerCase())
    ) {
      return false
    }
    return true
  })

  return (
    <div className="space-y-6">
      <div className="flex flex-col md:flex-row items-start md:items-center gap-4">
        <div className="flex flex-col sm:flex-row items-start sm:items-center gap-2">
          <Popover>
            <PopoverTrigger asChild>
              <Button variant="outline" className="flex items-center gap-2 w-full sm:w-auto justify-start">
                <Calendar className="h-4 w-4" />
                {fromDate ? new Intl.DateTimeFormat("zh-CN").format(fromDate) : <span>起始日期</span>}
              </Button>
            </PopoverTrigger>
            <PopoverContent className="w-auto p-0">
              <CalendarComponent mode="single" selected={fromDate} onSelect={setFromDate} initialFocus />
            </PopoverContent>
          </Popover>

          <Select value={fromVersion} onValueChange={setFromVersion}>
            <SelectTrigger className="w-full sm:w-[180px]">
              <SelectValue placeholder="选择起始版本" />
            </SelectTrigger>
            <SelectContent>
              {mockVersions.map((version) => (
                <SelectItem key={version.id} value={version.id}>
                  {version.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="flex items-center justify-center">
          <ArrowRight className="h-6 w-6" />
        </div>

        <div className="flex flex-col sm:flex-row items-start sm:items-center gap-2">
          <Popover>
            <PopoverTrigger asChild>
              <Button variant="outline" className="flex items-center gap-2 w-full sm:w-auto justify-start">
                <Calendar className="h-4 w-4" />
                {toDate ? new Intl.DateTimeFormat("zh-CN").format(toDate) : <span>结束日期</span>}
              </Button>
            </PopoverTrigger>
            <PopoverContent className="w-auto p-0">
              <CalendarComponent mode="single" selected={toDate} onSelect={setToDate} initialFocus />
            </PopoverContent>
          </Popover>

          <Select value={toVersion} onValueChange={setToVersion}>
            <SelectTrigger className="w-full sm:w-[180px]">
              <SelectValue placeholder="选择结束版本" />
            </SelectTrigger>
            <SelectContent>
              {mockVersions.map((version) => (
                <SelectItem key={version.id} value={version.id}>
                  {version.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <Button className="ml-auto">对比版本</Button>
      </div>

      <div className="flex flex-col sm:flex-row items-start sm:items-center gap-4">
        <div className="relative flex-1">
          <Search className="absolute left-2 top-2.5 h-4 w-4 text-muted-foreground" />
          <Input
            placeholder="搜索实体ID、名称或类别..."
            className="pl-8"
            value={searchTerm}
            onChange={(e) => setSearchTerm(e.target.value)}
          />
        </div>

        <Select value={filterType || "all"} onValueChange={(value) => setFilterType(value)}>
          <SelectTrigger className="w-[140px]">
            <SelectValue placeholder="筛选变更类型" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">全部变更</SelectItem>
            <SelectItem value="add">新增</SelectItem>
            <SelectItem value="modify">修改</SelectItem>
            <SelectItem value="delete">删除</SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <div className="text-sm font-medium">对比结果 ({filteredResults.length})</div>
          <Button variant="outline" size="sm" className="text-xs">
            <SplitSquareVertical className="h-3 w-3 mr-1" />
            并排对比视图
          </Button>
        </div>

        <div className="rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-[80px]">变更类型</TableHead>
                <TableHead className="w-[100px]">实体ID</TableHead>
                <TableHead>实体名称</TableHead>
                <TableHead className="hidden md:table-cell">类别</TableHead>
                <TableHead className="w-[300px]">旧值</TableHead>
                <TableHead className="w-[300px]">新值</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {filteredResults.map((result) => (
                <TableRow key={result.id}>
                  <TableCell>
                    <div className="flex items-center gap-1">
                      {getChangeTypeIcon(result.changeType)}
                      <span className="text-xs">{getChangeTypeLabel(result.changeType)}</span>
                    </div>
                  </TableCell>
                  <TableCell className="font-medium">{result.id}</TableCell>
                  <TableCell>{result.name}</TableCell>
                  <TableCell className="hidden md:table-cell">
                    <Badge variant="outline">{result.category}</Badge>
                  </TableCell>
                  <TableCell className={result.changeType === "add" ? "text-muted-foreground italic" : ""}>
                    {result.changeType === "add" ? "（不存在）" : result.oldValue}
                  </TableCell>
                  <TableCell className={result.changeType === "delete" ? "text-muted-foreground italic" : ""}>
                    {result.changeType === "delete" ? "（已删除）" : result.newValue}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      </div>
    </div>
  )
}
