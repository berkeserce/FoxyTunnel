export type ProxyStatus = "Stopped" | "Bootstrapping" | "Running" | "Error";

export type StatusDto = {
  status: ProxyStatus;
  endpoint: string;
  socks_port: number;
  log_connections: boolean;
  exit_country?: string | null;
  bootstrap_timeout_seconds: number;
  last_error?: string | null;
};

export type LogDto = {
  sequence: number;
  level: "info" | "error" | string;
  message: string;
};

export type LogLine = {
  id: number;
  level: "info" | "error" | string;
  text: string;
};

export type LogsDto = {
  entries: LogDto[];
};

export type StartOptions = {
  socks_port: number;
  log_connections: boolean;
  exit_country?: string | null;
  bootstrap_timeout_seconds: number;
};

export type TorCheckDto = {
  status: "tor" | "not_tor" | "unavailable";
  is_tor: boolean;
  ip?: string | null;
  latency_ms?: number | null;
  message: string;
};

export type TorRouteStatus = TorCheckDto["status"] | "untested";

export type ViewMode = "main" | "settings";
