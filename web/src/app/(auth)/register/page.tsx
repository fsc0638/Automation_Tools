"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import { auth } from "@/lib/api";
import { useAuthStore } from "@/lib/store";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

export default function RegisterPage() {
  const router = useRouter();
  const setAuth = useAuthStore((s) => s.setAuth);
  const [form, setForm] = useState({ email: "", password: "", display_name: "" });
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    setLoading(true);
    try {
      const res = await auth.register(form);
      setAuth(res.access_token, res.user);
      router.push("/projects");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Registration failed");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-[#F8F9FA]">
      <div className="w-full max-w-md">
        <div className="text-center mb-8">
          <div className="inline-flex items-center gap-3 mb-4">
            <div className="w-10 h-10 rounded-lg bg-[#002D62] flex items-center justify-center">
              <span className="text-white font-bold text-lg">K</span>
            </div>
            <span className="text-2xl font-bold text-[#002D62]">Kway Dev</span>
          </div>
          <p className="text-[#64748B] text-sm">Create your account to get started</p>
        </div>

        <div className="bg-white rounded-xl border border-[#E2E8F0] shadow-sm p-8">
          <h1 className="text-xl font-semibold text-[#1A1A2E] mb-6">Create account</h1>
          <form onSubmit={handleSubmit} className="flex flex-col gap-4">
            <Input id="name" label="Display Name" placeholder="Your name" value={form.display_name}
              onChange={(e) => setForm((f) => ({ ...f, display_name: e.target.value }))} required />
            <Input id="email" label="Email" type="email" placeholder="you@example.com" value={form.email}
              onChange={(e) => setForm((f) => ({ ...f, email: e.target.value }))} required />
            <Input id="password" label="Password" type="password" placeholder="Min 8 characters" value={form.password}
              onChange={(e) => setForm((f) => ({ ...f, password: e.target.value }))} required />
            {error && (
              <p className="text-sm text-[#C8102E] bg-red-50 border border-red-200 rounded-lg px-3 py-2">{error}</p>
            )}
            <Button type="submit" size="lg" loading={loading} className="mt-2 w-full">Create Account</Button>
          </form>
          <p className="text-center text-sm text-[#64748B] mt-6">
            Already have an account?{" "}
            <Link href="/login" className="text-[#0050A0] hover:underline font-medium">Sign in</Link>
          </p>
        </div>
      </div>
    </div>
  );
}
