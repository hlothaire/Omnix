import { useCoreEvents } from "@/hooks/useCoreEvents";
import { SidebarProvider } from "@/components/ui/sidebar";
import { AppSidebar } from "@/components/AppSidebar";
import { TabBar } from "@/components/TabBar";
import { ChatArea } from "@/components/ChatArea";
import { StatusBar } from "@/components/StatusBar";
import { ApprovalModal } from "@/components/ApprovalModal";
import { TitleBar } from "@/components/TitleBar";
import { TooltipProvider } from "@/components/ui/tooltip";

export default function App() {
  useCoreEvents();

  return (
    <TooltipProvider>
      <SidebarProvider defaultOpen={true} className="h-[var(--app-height)]">
        <div className="h-full w-full flex flex-col">
          <TitleBar />
          <div className="flex-1 flex min-h-0">
            <AppSidebar />
            <div className="flex-1 flex flex-col min-w-0">
              <TabBar />
              <ChatArea />
              <StatusBar />
            </div>
          </div>
        </div>
        <ApprovalModal />
      </SidebarProvider>
    </TooltipProvider>
  );
}
