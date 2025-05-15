"use client"

import { useState, useEffect } from "react"
import { Bar, BarChart, ResponsiveContainer, XAxis, YAxis, Tooltip, Legend } from "recharts"
import { getRecentChangesStats } from "@/lib/surrealdb"

interface ChangeStatsData {
  date: string
  total: number
  add_count: number
  modify_count: number
  delete_count: number
}

export function RecentChangesOverview() {
  const [data, setData] = useState<ChangeStatsData[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    const fetchData = async () => {
      setLoading(true)
      try {
        const stats = await getRecentChangesStats(7)
        // 格式化日期
        const formattedData = stats.map((item: any) => ({
          ...item,
          date: new Date(item.date).toLocaleDateString("zh-CN", { month: "short", day: "numeric" }),
        }))
        setData(formattedData)
      } catch (error) {
        console.error("获取变更统计数据失败:", error)
      } finally {
        setLoading(false)
      }
    }

    fetchData()
  }, [])

  if (loading && data.length === 0) {
    return (
      <div className="flex h-[350px] items-center justify-center">
        <p className="text-muted-foreground">加载中...</p>
      </div>
    )
  }

  if (data.length === 0) {
    return (
      <div className="flex h-[350px] items-center justify-center">
        <p className="text-muted-foreground">暂无数据</p>
      </div>
    )
  }

  return (
    <ResponsiveContainer width="100%" height={350}>
      <BarChart data={data}>
        <XAxis
          dataKey="date"
          stroke="#888888"
          fontSize={12}
          tickLine={false}
          axisLine={false}
        />
        <YAxis
          stroke="#888888"
          fontSize={12}
          tickLine={false}
          axisLine={false}
          tickFormatter={(value) => `${value}`}
        />
        <Tooltip
          formatter={(value: number, name: string) => {
            const displayName = 
              name === "add_count" ? "新增" : 
              name === "modify_count" ? "修改" : 
              name === "delete_count" ? "删除" : 
              name;
            return [value, displayName];
          }}
          labelFormatter={(label) => `日期: ${label}`}
        />
        <Legend
          formatter={(value) => {
            return value === "add_count" ? "新增" : 
                  value === "modify_count" ? "修改" : 
                  value === "delete_count" ? "删除" : value;
          }}
        />
        <Bar dataKey="add_count" fill="#4ade80" radius={[4, 4, 0, 0]} />
        <Bar dataKey="modify_count" fill="#facc15" radius={[4, 4, 0, 0]} />
        <Bar dataKey="delete_count" fill="#f87171" radius={[4, 4, 0, 0]} />
      </BarChart>
    </ResponsiveContainer>
  )
}
