"use client"

import { useState, useEffect } from "react"
import type { Metadata } from "next"
import Link from "next/link"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { DashboardHeader } from "@/components/dashboard-header"
import { DashboardShell } from "@/components/dashboard-shell"
import { DataChangesList } from "@/components/data-changes-list"
import { DataTimeline } from "@/components/data-timeline"
import { DataVersionComparison } from "@/components/data-version-comparison"
import { RecentChangesOverview } from "@/components/recent-changes-overview"
import { SessionList } from "@/components/session-list"
import { getChangesOverview } from "@/lib/surrealdb"
import { initSurrealDB } from "@/lib/surrealdb"

export default function DashboardPage() {
  const [overview, setOverview] = useState<any>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    const init = async () => {
      // 初始化SurrealDB连接
      await initSurrealDB()
      
      // 加载概览数据
      setLoading(true)
      try {
        const data = await getChangesOverview()
        setOverview(data)
      } catch (error) {
        console.error("获取概览数据失败:", error)
      } finally {
        setLoading(false)
      }
    }

    init()
  }, [])

  // 计算变化百分比
  const getChangePercentage = (today: number, yesterday: number) => {
    if (yesterday === 0) return "+100%"
    const change = ((today - yesterday) / yesterday) * 100
    return change >= 0 ? `+${change.toFixed(0)}%` : `${change.toFixed(0)}%`
  }

  return (
    <>
      <DashboardShell>
        <DashboardHeader heading="E3D 数据监控面板" description="实时监控 E3D 数据变化，查看历史版本数据">
          <Link href="/settings">
            <Button variant="outline">设置</Button>
          </Link>
        </DashboardHeader>
        <Tabs defaultValue="overview" className="space-y-4">
          <TabsList>
            <TabsTrigger value="overview">概览</TabsTrigger>
            <TabsTrigger value="changes">变更记录</TabsTrigger>
            <TabsTrigger value="sessions">会话信息</TabsTrigger>
            <TabsTrigger value="timeline">时间线</TabsTrigger>
            <TabsTrigger value="comparison">版本对比</TabsTrigger>
          </TabsList>
          <TabsContent value="overview" className="space-y-4">
            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
              <Card>
                <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                  <CardTitle className="text-sm font-medium">今日新增</CardTitle>
                  <svg
                    xmlns="http://www.w3.org/2000/svg"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth="2"
                    className="h-4 w-4 text-green-500"
                  >
                    <path d="M12 5v14M5 12h14" />
                  </svg>
                </CardHeader>
                <CardContent>
                  <div className="text-2xl font-bold">
                    {loading ? "..." : overview?.today?.add_count || 0}
                  </div>
                  <p className="text-xs text-muted-foreground">
                    {loading ? "加载中..." : 
                      overview ? getChangePercentage(
                        overview.today.add_count, 
                        overview.yesterday.add_count
                      ) : "无数据"}
                  </p>
                </CardContent>
              </Card>
              <Card>
                <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                  <CardTitle className="text-sm font-medium">今日修改</CardTitle>
                  <svg
                    xmlns="http://www.w3.org/2000/svg"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth="2"
                    className="h-4 w-4 text-yellow-500"
                  >
                    <path d="M20 7h-9m9 10h-9M3 7h2m-2 10h2" />
                  </svg>
                </CardHeader>
                <CardContent>
                  <div className="text-2xl font-bold">
                    {loading ? "..." : overview?.today?.modify_count || 0}
                  </div>
                  <p className="text-xs text-muted-foreground">
                    {loading ? "加载中..." : 
                      overview ? getChangePercentage(
                        overview.today.modify_count, 
                        overview.yesterday.modify_count
                      ) : "无数据"}
                  </p>
                </CardContent>
              </Card>
              <Card>
                <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                  <CardTitle className="text-sm font-medium">今日删除</CardTitle>
                  <svg
                    xmlns="http://www.w3.org/2000/svg"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth="2"
                    className="h-4 w-4 text-red-500"
                  >
                    <path d="M5 12h14" />
                  </svg>
                </CardHeader>
                <CardContent>
                  <div className="text-2xl font-bold">
                    {loading ? "..." : overview?.today?.delete_count || 0}
                  </div>
                  <p className="text-xs text-muted-foreground">
                    {loading ? "加载中..." : 
                      overview ? getChangePercentage(
                        overview.today.delete_count, 
                        overview.yesterday.delete_count
                      ) : "无数据"}
                  </p>
                </CardContent>
              </Card>
              <Card>
                <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                  <CardTitle className="text-sm font-medium">总数据量</CardTitle>
                  <svg
                    xmlns="http://www.w3.org/2000/svg"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth="2"
                    className="h-4 w-4 text-blue-500"
                  >
                    <path d="M12 2v20M2 12h20" />
                  </svg>
                </CardHeader>
                <CardContent>
                  <div className="text-2xl font-bold">
                    {loading ? "..." : overview?.total?.toLocaleString() || 0}
                  </div>
                  <p className="text-xs text-muted-foreground">
                    全部记录
                  </p>
                </CardContent>
              </Card>
            </div>
            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-7">
              <Card className="col-span-4">
                <CardHeader>
                  <CardTitle>近期变更趋势</CardTitle>
                </CardHeader>
                <CardContent className="pl-2">
                  <RecentChangesOverview />
                </CardContent>
              </Card>
              <Card className="col-span-3">
                <CardHeader>
                  <CardTitle>最近变更</CardTitle>
                  <CardDescription>过去24小时内的数据变更</CardDescription>
                </CardHeader>
                <CardContent>
                  <DataChangesList limit={5} />
                </CardContent>
              </Card>
            </div>
          </TabsContent>
          <TabsContent value="changes" className="space-y-4">
            <Card>
              <CardHeader>
                <CardTitle>数据变更记录</CardTitle>
                <CardDescription>查看所有数据的增删改记录</CardDescription>
              </CardHeader>
              <CardContent>
                <DataChangesList limit={20} />
              </CardContent>
            </Card>
          </TabsContent>
          <TabsContent value="sessions" className="space-y-4">
            <Card>
              <CardHeader>
                <CardTitle>会话信息</CardTitle>
                <CardDescription>查看会话信息及其相关的变更</CardDescription>
              </CardHeader>
              <CardContent>
                <SessionList />
              </CardContent>
            </Card>
          </TabsContent>
          <TabsContent value="timeline" className="space-y-4">
            <Card>
              <CardHeader>
                <CardTitle>数据时间线</CardTitle>
                <CardDescription>查看数据变更的历史时间线</CardDescription>
              </CardHeader>
              <CardContent>
                <DataTimeline />
              </CardContent>
            </Card>
          </TabsContent>
          <TabsContent value="comparison" className="space-y-4">
            <Card>
              <CardHeader>
                <CardTitle>版本对比</CardTitle>
                <CardDescription>对比不同时间点的数据版本</CardDescription>
              </CardHeader>
              <CardContent>
                <DataVersionComparison />
              </CardContent>
            </Card>
          </TabsContent>
        </Tabs>
      </DashboardShell>
    </>
  )
}
