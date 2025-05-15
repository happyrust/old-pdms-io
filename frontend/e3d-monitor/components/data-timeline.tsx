"use client"

import { useState } from "react"
import { Calendar, Clock, PlusCircle, MinusCircle, Edit, ChevronLeft, ChevronRight } from "lucide-react"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Button } from "@/components/ui/button"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Calendar as CalendarComponent } from "@/components/ui/calendar"
import { cn } from "@/lib/utils"
import { Badge } from "@/components/ui/badge"
import { Slider } from "@/components/ui/slider"

// 模拟数据 - 时间线数据
const timelineData = [
  {
    date: "2025-05-09",
    versions: [
      {
        id: "v1",
        time: "09:23:45",
        changes: [
          { type: "add", count: 8 },
          { type: "modify", count: 12 },
          { type: "delete", count: 4 },
        ],
      },
      {
        id: "v2",
        time: "14:15:30",
        changes: [
          { type: "add", count: 5 },
          { type: "modify", count: 10 },
          { type: "delete", count: 2 },
        ],
      },
    ],
  },
  {
    date: "2025-05-08",
    versions: [
      {
        id: "v3",
        time: "10:30:22",
        changes: [
          { type: "add", count: 10 },
          { type: "modify", count: 15 },
          { type: "delete", count: 5 },
        ],
      },
      {
        id: "v4",
        time: "16:45:12",
        changes: [
          { type: "add", count: 7 },
          { type: "modify", count: 9 },
          { type: "delete", count: 3 },
        ],
      },
    ],
  },
  {
    date: "2025-05-07",
    versions: [
      {
        id: "v5",
        time: "11:20:33",
        changes: [
          { type: "add", count: 12 },
          { type: "modify", count: 18 },
          { type: "delete", count: 6 },
        ],
      },
    ],
  },
  {
    date: "2025-05-06",
    versions: [
      {
        id: "v6",
        time: "09:15:45",
        changes: [
          { type: "add", count: 9 },
          { type: "modify", count: 14 },
          { type: "delete", count: 4 },
        ],
      },
      {
        id: "v7",
        time: "15:30:10",
        changes: [
          { type: "add", count: 6 },
          { type: "modify", count: 11 },
          { type: "delete", count: 3 },
        ],
      },
    ],
  },
  {
    date: "2025-05-05",
    versions: [
      {
        id: "v8",
        time: "10:45:22",
        changes: [
          { type: "add", count: 11 },
          { type: "modify", count: 16 },
          { type: "delete", count: 5 },
        ],
      },
    ],
  },
]

export function DataTimeline() {
  const [date, setDate] = useState<Date | undefined>(new Date())
  const [selectedVersion, setSelectedVersion] = useState<string | null>(null)

  const formatDate = (dateString: string) => {
    const date = new Date(dateString)
    return new Intl.DateTimeFormat("zh-CN", {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    }).format(date)
  }

  const getTypeIcon = (type: string) => {
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

  const getTypeLabel = (type: string) => {
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

  return (
    <div className="space-y-6">
      <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4">
        <div className="flex items-center gap-2">
          <Popover>
            <PopoverTrigger asChild>
              <Button variant="outline" className="flex items-center gap-2">
                <Calendar className="h-4 w-4" />
                {date ? new Intl.DateTimeFormat("zh-CN").format(date) : <span>选择日期</span>}
              </Button>
            </PopoverTrigger>
            <PopoverContent className="w-auto p-0">
              <CalendarComponent mode="single" selected={date} onSelect={setDate} initialFocus />
            </PopoverContent>
          </Popover>

          <Select>
            <SelectTrigger className="w-[180px]">
              <SelectValue placeholder="筛选变更类型" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">全部变更</SelectItem>
              <SelectItem value="add">仅新增</SelectItem>
              <SelectItem value="modify">仅修改</SelectItem>
              <SelectItem value="delete">仅删除</SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div className="flex items-center gap-2">
          <Button variant="outline" size="icon">
            <ChevronLeft className="h-4 w-4" />
          </Button>
          <Button variant="outline">回到当前版本</Button>
          <Button variant="outline" size="icon">
            <ChevronRight className="h-4 w-4" />
          </Button>
        </div>
      </div>

      <div className="space-y-1">
        <div className="text-sm font-medium">时间轴</div>
        <div className="relative pt-6 pb-2">
          <div className="absolute left-0 right-0 h-1 bg-muted rounded-full">
            <div className="absolute left-0 h-1 w-1/3 bg-primary rounded-full"></div>
          </div>
          <Slider defaultValue={[33]} max={100} step={1} className="absolute left-0 right-0 top-4" />
          <div className="flex justify-between text-xs text-muted-foreground mt-4">
            <div>2025-05-05</div>
            <div>2025-05-07</div>
            <div>2025-05-09</div>
          </div>
        </div>
      </div>

      <div className="space-y-6">
        {timelineData.map((day) => (
          <div key={day.date} className="space-y-2">
            <div className="flex items-center gap-2">
              <div className="h-8 w-8 rounded-full bg-muted flex items-center justify-center">
                <Calendar className="h-4 w-4" />
              </div>
              <h3 className="font-medium">{formatDate(day.date)}</h3>
            </div>

            <div className="ml-4 border-l pl-6 space-y-6">
              {day.versions.map((version) => (
                <div
                  key={version.id}
                  className={cn(
                    "p-4 rounded-lg border",
                    selectedVersion === version.id ? "border-primary bg-muted/50" : "",
                  )}
                  onClick={() => setSelectedVersion(version.id)}
                >
                  <div className="flex items-center justify-between mb-4">
                    <div className="flex items-center gap-2">
                      <div className="h-6 w-6 rounded-full bg-muted flex items-center justify-center">
                        <Clock className="h-3 w-3" />
                      </div>
                      <span className="text-sm font-medium">{version.time}</span>
                    </div>
                    <Button variant="outline" size="sm" className="text-xs">
                      查看此版本
                    </Button>
                  </div>

                  <div className="flex flex-wrap gap-3">
                    {version.changes.map((change, idx) => (
                      <div key={idx} className="flex items-center gap-1 bg-background rounded-full px-3 py-1 text-sm">
                        {getTypeIcon(change.type)}
                        <span>{getTypeLabel(change.type)}</span>
                        <Badge variant="outline" className="ml-1">
                          {change.count}
                        </Badge>
                      </div>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
