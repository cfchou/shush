export function decodeBase64Bytes(data: string): Uint8Array {
  const binary = atob(data);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    out[i] = binary.charCodeAt(i);
  }
  return out;
}

export function normalizeLineEndings(data: Uint8Array): Uint8Array {
  let extra = 0;
  for (let i = 0; i < data.length; i += 1) {
    if (data[i] === 0x0a && (i === 0 || data[i - 1] !== 0x0d)) {
      extra += 1;
    }
  }

  if (extra === 0) {
    return data;
  }

  const out = new Uint8Array(data.length + extra);
  let j = 0;
  for (let i = 0; i < data.length; i += 1) {
    if (data[i] === 0x0a && (i === 0 || data[i - 1] !== 0x0d)) {
      out[j] = 0x0d;
      j += 1;
    }
    out[j] = data[i];
    j += 1;
  }
  return out;
}

export function reconnectDelayMs(attempt: number): number {
  return Math.min(30000, 1000 * 2 ** attempt);
}
