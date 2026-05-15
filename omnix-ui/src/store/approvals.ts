import { create } from "zustand";
import { PendingApproval } from "@/lib/types";

interface ApprovalState {
  approvals: Record<string, PendingApproval>;
  addApproval: (a: PendingApproval) => void;
  removeApproval: (sessionId: string, callId: string) => void;
}

function approvalKey(sessionId: string, callId: string) {
  return `${sessionId}:${callId}`;
}

export const useApprovalStore = create<ApprovalState>((set) => ({
  approvals: {},
  addApproval: (a) => set((s) => ({ approvals: { ...s.approvals, [approvalKey(a.sessionId, a.callId)]: a } })),
  removeApproval: (sessionId, callId) => set((s) => {
    const copy = { ...s.approvals };
    delete copy[approvalKey(sessionId, callId)];
    return { approvals: copy };
  }),
}));
