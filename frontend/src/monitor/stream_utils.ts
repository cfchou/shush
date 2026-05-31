export function decodeBase64Bytes(data: string): Uint8Array {
  const binary = atob(data);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    out[i] = binary.charCodeAt(i);
  }
  return out;
}

export function reconnectDelayMs(attempt: number): number {
  return Math.min(30000, 1000 * 2 ** attempt);
}
