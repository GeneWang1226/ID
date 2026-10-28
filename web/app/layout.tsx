import type { Metadata } from "next";
import type { ReactNode } from "react";

export const metadata: Metadata = {
  title: "ID 身份管理",
  description: "查看和编辑各身份的非机密个人信息",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="zh-CN">
      <body
        style={{
          margin: 0,
          fontFamily:
            'system-ui, -apple-system, "PingFang SC", "Microsoft YaHei", sans-serif',
          background: "#0f172a",
          color: "#e2e8f0",
          minHeight: "100vh",
        }}
      >
        {children}
      </body>
    </html>
  );
}
