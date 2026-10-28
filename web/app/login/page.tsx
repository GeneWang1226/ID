"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";

export default function LoginPage() {
  const router = useRouter();
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    const res = await fetch("/api/login", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ password }),
    });
    if (res.ok) {
      router.push("/");
      router.refresh();
    } else {
      setError("密码不正确");
    }
  }

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        minHeight: "100vh",
      }}
    >
      <form
        onSubmit={handleSubmit}
        style={{
          background: "#1e293b",
          padding: "2rem",
          borderRadius: "0.75rem",
          width: "320px",
        }}
      >
        <h1 style={{ marginTop: 0, fontSize: "1.25rem" }}>登录</h1>
        <input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          placeholder="访问密码"
          style={{
            width: "100%",
            padding: "0.5rem",
            borderRadius: "0.375rem",
            border: "1px solid #475569",
            background: "#0f172a",
            color: "#e2e8f0",
            marginBottom: "1rem",
          }}
        />
        {error && (
          <p style={{ color: "#f87171", fontSize: "0.875rem", margin: "0 0 1rem" }}>
            {error}
          </p>
        )}
        <button
          type="submit"
          style={{
            width: "100%",
            padding: "0.5rem",
            borderRadius: "0.375rem",
            border: "none",
            background: "#3b82f6",
            color: "#fff",
            cursor: "pointer",
          }}
        >
          进入
        </button>
      </form>
    </div>
  );
}
