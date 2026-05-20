export const viewVariants = {
  enter: { opacity: 0, x: 22, scale: 0.985 },
  center: { opacity: 1, x: 0, scale: 1 },
  exit: { opacity: 0, x: -18, scale: 0.985 },
};

export const cardVariants = {
  hidden: { opacity: 0, y: 10 },
  visible: { opacity: 1, y: 0 },
};

export const logLineVariants = {
  hidden: { opacity: 0, x: -8 },
  visible: { opacity: 1, x: 0 },
  exit: { opacity: 0, x: 8 },
};

export const springTransition = {
  damping: 24,
  stiffness: 260,
  type: "spring",
} as const;

export const quickTransition = {
  duration: 0.18,
  ease: "easeOut",
} as const;
