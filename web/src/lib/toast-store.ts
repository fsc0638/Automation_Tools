"use client";
import { create } from "zustand";

export type ToastTone = "success" | "info" | "warning" | "error";

export interface ToastItem {
  id: string;
  title: string;
  description?: string;
  tone: ToastTone;
}

interface ToastStore {
  toasts: ToastItem[];
  pushToast: (toast: Omit<ToastItem, "id">) => string;
  removeToast: (id: string) => void;
}

export const useToastStore = create<ToastStore>((set) => ({
  toasts: [],
  pushToast: (toast) => {
    const id = crypto.randomUUID();
    set((state) => ({ toasts: [...state.toasts, { ...toast, id }] }));
    window.setTimeout(() => {
      set((state) => ({ toasts: state.toasts.filter((item) => item.id !== id) }));
    }, 3600);
    return id;
  },
  removeToast: (id) => {
    set((state) => ({ toasts: state.toasts.filter((item) => item.id !== id) }));
  },
}));
