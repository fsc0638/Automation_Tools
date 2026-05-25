"use client";
import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { UserInfo } from "./api";
import { forgetUserKek } from "./clientCrypto";

interface AuthStore {
  token: string | null;
  refreshToken: string | null;
  user: UserInfo | null;
  /** False until zustand-persist has read localStorage. AppLayout uses
   *  this to avoid kicking the user to /login during the SSR→hydrate
   *  window when token is still default-null. */
  hasHydrated: boolean;
  setAuth: (token: string, user: UserInfo, refreshToken?: string) => void;
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
      refreshToken: null,
      user: null,
      hasHydrated: false,
      setAuth: (token, user, refreshToken) => {
        localStorage.setItem("kway_token", token);
        if (refreshToken) localStorage.setItem("kway_refresh_token", refreshToken);
        set({ token, user, refreshToken: refreshToken ?? null });
      },
      logout: () => {
        localStorage.removeItem("kway_token");
        localStorage.removeItem("kway_refresh_token");
        // Drop the in-memory User KEK as well — relevant for the idle-
        // timeout path in (app)/layout.tsx which calls store.logout()
        // directly without going through auth.logout().
        forgetUserKek();
        set({ token: null, user: null, refreshToken: null });
      },
    }),
    {
      name: "kway-auth",
      // hasHydrated isn't persisted — it's a transient runtime flag.
      // We flip it true once rehydration finishes (or fails) so consumers
      // know the localStorage read has completed.
      partialize: (state) => ({
        token: state.token,
        refreshToken: state.refreshToken,
        user: state.user,
      }),
      onRehydrateStorage: () => (state) => {
        if (state) {
          // Sync the legacy mirror so api.ts's getToken() also sees it
          // immediately after a fresh tab — without this, the first
          // request after F5 might still send no Authorization header.
          if (state.token) localStorage.setItem("kway_token", state.token);
          if (state.refreshToken) localStorage.setItem("kway_refresh_token", state.refreshToken);
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

// ---------------------------------------------------------------------
// Per-project Roadmap filter / sort persistence (C2).
//
// Keyed by project_id so filters set on project A don't leak into B.
// Persisted to localStorage so reloading the page restores the state.
// All fields are stored as raw strings; the consumer (RoadmapTab) is
// responsible for re-parsing if needed.
// ---------------------------------------------------------------------
export interface RoadmapFilterState {
  search: string;
  priority: string;   // "all" | "low" | "medium" | "high" | "critical"
  assignee: string;
  label: string;
  sprint: string;     // "all" | "none" | uuid
  overdue: boolean;
  sort: string;       // "priority" | "due" | "newest" | "oldest" | "updated"
}

const defaultFilter: RoadmapFilterState = {
  search: "",
  priority: "all",
  assignee: "all",
  label: "all",
  sprint: "all",
  overdue: false,
  sort: "priority",
};

interface RoadmapFiltersStore {
  byProject: Record<string, RoadmapFilterState>;
  hasHydrated: boolean;
  get: (projectId: string) => RoadmapFilterState;
  set: (projectId: string, patch: Partial<RoadmapFilterState>) => void;
  reset: (projectId: string) => void;
}

export const useRoadmapFiltersStore = create<RoadmapFiltersStore>()(
  persist(
    (set, get) => ({
      byProject: {},
      hasHydrated: false,
      get: (projectId) => get().byProject[projectId] ?? defaultFilter,
      set: (projectId, patch) => set((state) => {
        const current = state.byProject[projectId] ?? defaultFilter;
        return { byProject: { ...state.byProject, [projectId]: { ...current, ...patch } } };
      }),
      reset: (projectId) => set((state) => {
        const next = { ...state.byProject };
        delete next[projectId];
        return { byProject: next };
      }),
    }),
    {
      name: "kway-roadmap-filters",
      partialize: (state) => ({ byProject: state.byProject }),
      onRehydrateStorage: () => (state) => {
        if (state) state.hasHydrated = true;
        else useRoadmapFiltersStore.setState({ hasHydrated: true });
      },
    },
  ),
);
