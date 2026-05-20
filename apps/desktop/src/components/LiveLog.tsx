import { Button, ScrollShadow } from "@heroui/react";
import { AnimatePresence, motion } from "framer-motion";
import { RefreshCw, Trash2 } from "lucide-react";
import { useEffect, useRef } from "react";
import { logLineVariants, quickTransition } from "../motionPresets";
import type { LogLine } from "../types";

type LiveLogProps = {
  lines: LogLine[];
  onRefresh: () => void;
  onClear: () => void;
};

export function LiveLog({ lines, onRefresh, onClear }: LiveLogProps) {
  const scrollAreaRef = useRef<HTMLDivElement | null>(null);
  const bottomRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    window.requestAnimationFrame(() => {
      if (scrollAreaRef.current) {
        scrollAreaRef.current.scrollTop = scrollAreaRef.current.scrollHeight;
      }

      bottomRef.current?.scrollIntoView({ block: "end" });
    });
  }, [lines]);

  return (
    <section className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-[#3b2819] bg-[#15100c]/95 shadow-[0_18px_45px_rgba(0,0,0,0.22)]">
      <div className="flex flex-none items-center justify-between gap-2 border-b border-[#3b2819] bg-[#1d1510]/95 px-3 py-2">
        <div className="flex items-center gap-2">
          <span className="size-2 rounded-full bg-orange-400 shadow-[0_0_12px_rgba(251,146,60,0.8)]" />
          <span className="text-[0.76rem] font-black text-[#b8a494]">Live log</span>
        </div>
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
      <ScrollShadow
        className="min-h-0 flex-1 overflow-auto bg-[#0f0b08] p-3"
        hideScrollBar={false}
        ref={scrollAreaRef}
      >
        <div className="grid gap-1 font-mono text-[0.76rem] leading-relaxed">
          <AnimatePresence initial={false}>
            {lines.length > 0 ? (
              lines.map((line) => (
                <motion.div
                  animate="visible"
                  className={line.level === "error" ? "break-words text-red-200" : "break-words text-[#f6d8bf]"}
                  exit="exit"
                  initial="hidden"
                  key={line.id}
                  transition={quickTransition}
                  variants={logLineVariants}
                >
                  {line.text}
                </motion.div>
              ))
            ) : (
              <motion.div
                animate="visible"
                className="text-[#b8a494]"
                initial="hidden"
                key="ready"
                transition={quickTransition}
                variants={logLineVariants}
              >
                Ready.
              </motion.div>
            )}
          </AnimatePresence>
          <div ref={bottomRef} />
        </div>
      </ScrollShadow>
    </section>
  );
}
