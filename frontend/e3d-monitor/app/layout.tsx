import type { Metadata } from 'next'
import './globals.css'

export const metadata: Metadata = {
  title: 'E3D 数据监控面板',
  description: '监控 E3D 数据变化，查看历史版本',
  generator: 'v0.dev',
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang="zh-CN">
      <body>{children}</body>
    </html>
  )
}
