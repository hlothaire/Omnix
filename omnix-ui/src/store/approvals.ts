import { create } from "zustand";
import { PendingApproval } from "@/lib/types";

interface ApprovalState {
  approvals: Record<string, PendingApproval>;
  addApproval: (a: PendingApproval) => void;
  removeApproval: (callId: string) => void;
}

export const useApprovalStore = create<ApprovalState>((set) => ({
  approvals: {},
  addApproval: (a) => set((s) => ({ approvals: { ...s.approvals, [a.callId]: a } })),
  removeApproval: (callId) => set((s) => {
    const copy = { ...s.approvals };
    delete copy[callId];
    return { approvals: copy };
  }),
}));
