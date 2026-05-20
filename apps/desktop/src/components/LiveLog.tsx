import { Button, ScrollShadow } from "@heroui/react";
import { RefreshCw, Trash2 } from "lucide-react";

type LiveLogProps = {
  lines: string[];
  onRefresh: () => void;
  onClear: () => void;
};

export function LiveLog({ lines, onRefresh, onClear }: LiveLogProps) {
  return (
    <section className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-[#3b2819] bg-[#15100c]">
      <div className="flex flex-none items-center justify-between gap-2 border-b border-[#3b2819] bg-[#1d1510] px-3 py-2">
        <span className="text-[0.76rem] font-black text-[#b8a494]">Live log</span>
        <div className="flex gap-2">
          <Button className="ghost-button" size="sm" variant="outline" onPress={onRefresh}>
            <RefreshCw size={14} />
            Refresh
          </Button>
          <Button className="ghost-button" size="sm" variant="outline" onPress={onClear}>
            <Trash2 size={14} />
            Clear
          </Button>
        </div>
      </div>
      <ScrollShadow className="min-h-0 flex-1 overflow-auto bg-[#0f0b08] p-3" hideScrollBar={false}>
        <pre className="m-0 whitespace-pre-wrap break-words font-mono text-[0.78rem] leading-relaxed text-[#f6d8bf]">
          {lines.length > 0 ? lines.join("\n") : "Ready."}
        </pre>
      </ScrollShadow>
    </section>
  );
}
