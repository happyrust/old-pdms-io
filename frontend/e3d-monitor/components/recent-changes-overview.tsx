"use client"

import { useState, useEffect } from "react"
import { Bar, BarChart, ResponsiveContainer, XAxis, YAxis, Tooltip, Legend } from "recharts"
import { getRecentChangesStats, checkDatabaseConnection } from "@/lib/surrealdb"

interface ChangeStatsData {
  date: string
  total: number
  add_count: number
  modify_count: number
  delete_count: number
}

export function RecentChangesOverview() {
  const [data, setData] = useState<ChangeStatsData[]>([])
  const [loading, setLoading] = useState(false)
  const [dbConnected, setDbConnected] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // 检查数据库连接
  useEffect(() => {
    let isMounted = true;
    const checkConnection = async () => {
      try {
        const isConnected = await checkDatabaseConnection()
        if (isMounted) {
          setDbConnected(isConnected)
          if (!isConnected) {
            setError("数据库未连接，无法获取统计数据")
          } else {
            setError(null)
          }
        }
      } catch (error) {
        console.error("检查数据库连接失败:", error)
        if (isMounted) {
          setDbConnected(false)
          setError("检查数据库连接失败")
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

  // 获取统计数据
  useEffect(() => {
    let isMounted = true;
    const fetchData = async () => {
      if (!dbConnected) {
        return
      }

      setLoading(true)
      try {
        const stats = await getRecentChangesStats(7)
        if (isMounted) {
          // 格式化日期
          const formattedData = stats.map((item: any) => ({
            ...item,
            date: new Date(item.date).toLocaleDateString("zh-CN", { month: "short", day: "numeric" }),
          }))
          setData(formattedData as unknown as ChangeStatsData[])
          setError(null)
        }
      } catch (error) {
        console.error("获取变更统计数据失败:", error)
        if (isMounted) {
          setError("获取统计数据失败，请稍后重试")
        }
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

  if (error) {
    return (
      <div className="flex h-[350px] items-center justify-center">
        <div className="text-center bg-red-50 p-4 rounded-md border border-red-200">
          <p className="text-red-500">{error}</p>
        </div>
      </div>
    )
  }

  if (!dbConnected) {
    return (
      <div className="flex h-[350px] items-center justify-center">
        <p className="text-muted-foreground">等待数据库连接...</p>
      </div>
    )
  }

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
