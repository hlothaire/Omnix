import { useApprovalStore } from "@/store/approvals";
import { respondToApproval } from "@/lib/commands";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

export function ApprovalModal() {
  const { approvals, removeApproval } = useApprovalStore();
  const entries = Object.values(approvals);

  if (entries.length === 0) return null;

  const first = entries[0];

  const handleRespond = (response: string) => {
    respondToApproval(first.sessionId, first.callId, response);
    removeApproval(first.sessionId, first.callId);
  };

  return (
    <Dialog open={entries.length > 0} onOpenChange={() => {}}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Approve Tool Call</DialogTitle>
          <DialogDescription>
            The agent wants to run{" "}
            <span className="font-medium text-primary">
              {first.toolName}
            </span>
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-3">
          {first.description && (
            <p className="text-sm text-muted-foreground">
              {first.description}
            </p>
          )}

          <div>
            <div className="text-xs font-medium text-muted-foreground mb-1">
              Input
            </div>
            <pre className="p-2 rounded bg-secondary text-xs text-muted-foreground font-mono overflow-x-auto max-h-40 overflow-y-auto">
              {JSON.stringify(first.toolInput, null, 2)}
            </pre>
          </div>

          {first.riskLevel && (
            <div className="text-xs">
              Risk level:{" "}
              <span
                className={
                  first.riskLevel === "Destructive"
                    ? "text-destructive font-medium"
                    : first.riskLevel === "WorkspaceWrite"
                      ? "text-yellow-500"
                      : "text-muted-foreground"
                }
              >
                {first.riskLevel}
              </span>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button
            variant="destructive"
            size="sm"
            onClick={() => handleRespond("deny")}
          >
            Deny
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => handleRespond("allow_once")}
          >
            Allow Once
          </Button>
          <Button size="sm" onClick={() => handleRespond("allow_for_session")}>
            Allow For Session
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
