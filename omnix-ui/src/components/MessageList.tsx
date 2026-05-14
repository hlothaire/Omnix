import { useRef, useCallback, useEffect } from "react";
import { Virtuoso, VirtuosoHandle } from "react-virtuoso";
import { useChatStore } from "@/store/chat";
import { MessageBubble } from "./MessageBubble";

export function MessageList() {
  const { messages, isStreaming } = useChatStore();
  const ref = useRef<VirtuosoHandle>(null);
  const isAtBottomRef = useRef(true);

  const handleAtBottomStateChange = useCallback((atBottom: boolean) => {
    isAtBottomRef.current = atBottom;
  }, []);

  useEffect(() => {
    if (isStreaming && isAtBottomRef.current && messages.length > 0) {
      const id = setTimeout(() => {
        ref.current?.scrollToIndex({ index: messages.length - 1, behavior: "smooth" });
      }, 16);
      return () => clearTimeout(id);
    }
  }, [isStreaming, messages.length]);

  return (
    <Virtuoso
      ref={ref}
      data={messages}
      totalCount={messages.length}
      itemContent={(index) => (
        <MessageBubble message={messages[index]} index={index} />
      )}
      atBottomStateChange={handleAtBottomStateChange}
      followOutput="smooth"
      className="h-full"
      style={{ overflowX: "hidden" }}
    />
  );
}
