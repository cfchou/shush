import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

export { decodeBase64Bytes } from "./stream_utils";

export class TerminalView {
  private readonly term: Terminal;
  private readonly fitAddon: FitAddon;

  constructor(container: HTMLElement) {
    this.term = new Terminal({
      cols: 220,
      rows: 50,
      theme: {
        background: "#101318",
        foreground: "#d7dde8",
        cursor: "#7bdff2",
      },
    });
    this.fitAddon = new FitAddon();
    this.term.loadAddon(this.fitAddon);
    this.term.open(container);
    this.fit();
  }

  write(data: Uint8Array): void {
    this.term.write(data);
  }

  clear(): void {
    this.term.reset();
  }

  fit(): void {
    this.fitAddon.fit();
  }
}
