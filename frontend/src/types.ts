export type SessionState = "idle" | "pending" | "executing";

export type CommandState =
  | "pending"
  | "executing"
  | { completed: number }
  | "rejected"
  | "aborted";

export interface CommandCard {
  id: string;
  command: string;
  state: CommandState;
  exit_code: number | null;
  output: string;
  created_at: string;
  resolved_at: string | null;
  resolved_by: string | null;
}

export interface Session {
  id: string;
  name: string;
  host: string;
  state: SessionState;
  yolo: boolean;
  current_command: CommandCard | null;
  created_at: string;
}
