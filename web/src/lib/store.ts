"use client";
import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { UserInfo } from "./api";

interface AuthStore {
  token: string | null;
  user: UserInfo | null;
  /** False until zustand-persist has read localStorage. AppLayout uses
   *  this to avoid kicking the user to /login during the SSR→hydrate
   *  window when token is still default-null. */
  hasHydrated: boolean;
  setAuth: (token: string, user: UserInfo) => void;
  logout: () => void;
}

interface WorkspaceChromeStore {
  showAppSidebar: boolean;
  setShowAppSidebar: (show: boolean) => void;
}

export const useAuthStore = create<AuthStore>()(
  persist(
    (set) => ({
      token: null,
      user: null,
      hasHydrated: false,
      setAuth: (token, user) => {
        localStorage.setItem("kway_token", token);
        set({ token, user });
      },
      logout: () => {
        localStorage.removeItem("kway_token");
        set({ token: null, user: null });
      },
    }),
    {
      name: "kway-auth",
      // hasHydrated isn't persisted — it's a transient runtime flag.
      // We flip it true once rehydration finishes (or fails) so consumers
      // know the localStorage read has completed.
      partialize: (state) => ({ token: state.token, user: state.user }),
      onRehydrateStorage: () => (state) => {
        if (state) {
          // Sync the legacy mirror so api.ts's getToken() also sees it
          // immediately after a fresh tab — without this, the first
          // request after F5 might still send no Authorization header.
          if (state.token) localStorage.setItem("kway_token", state.token);
          state.hasHydrated = true;
        } else {
          // Even on failure we still need consumers to stop waiting.
          useAuthStore.setState({ hasHydrated: true });
        }
      },
    }
  )
);

export const useWorkspaceChromeStore = create<WorkspaceChromeStore>((set) => ({
  showAppSidebar: true,
  setShowAppSidebar: (show) => set({ showAppSidebar: show }),
}));
