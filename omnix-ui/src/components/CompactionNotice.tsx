import { ArchiveRestore } from "lucide-react";
import type { CompactionNotice as CompactionNoticeData } from "@/lib/types";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";

interface Props {
  notice: CompactionNoticeData;
}

function formatTokens(value: number) {
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(value);
}

function formatPercent(notice: CompactionNoticeData) {
  if (notice.inputBudget <= 0) return null;
  return Math.round((notice.tokensBefore / notice.inputBudget) * 100);
}

export function CompactionNotice({ notice }: Props) {
  const percent = formatPercent(notice);

  return (
    <div className="px-5 py-3 animate-message-appear">
      <Alert className="mx-auto max-w-3xl border-dashed bg-secondary/40">
        <ArchiveRestore />
        <AlertTitle>Session compacted</AlertTitle>
        <AlertDescription>
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <Badge variant="secondary">
              {notice.removedCount} messages summarized
            </Badge>
            <Badge variant="outline">
              {formatTokens(notice.tokensBefore)} tokens before
            </Badge>
            <Badge variant="outline">
              {formatTokens(notice.inputBudget)} token budget
            </Badge>
            {percent !== null && (
              <Badge variant="outline">{percent}% of budget</Badge>
            )}
          </div>
          <p className="mt-2 text-xs">
            Older context was compressed into a checkpoint summary. The latest
            turn and recent tool state were kept after message #{notice.firstKeptIndex}.
          </p>
        </AlertDescription>
      </Alert>
    </div>
  );
}
