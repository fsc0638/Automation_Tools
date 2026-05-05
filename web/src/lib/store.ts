"use client";
import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { UserInfo } from "./api";

interface AuthStore {
  token: string | null;
  user: UserInfo | null;
  setAuth: (token: string, user: UserInfo) => void;
  logout: () => void;
}

export const useAuthStore = create<AuthStore>()(
  persist(
    (set) => ({
      token: null,
      user: null,
      setAuth: (token, user) => {
        localStorage.setItem("kway_token", token);
        set({ token, user });
      },
      logout: () => {
        localStorage.removeItem("kway_token");
        set({ token: null, user: null });
      },
    }),
    { name: "kway-auth" }
  )
);
