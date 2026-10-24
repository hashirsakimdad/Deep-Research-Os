// Minimal Server-Sent Events client built on fetch, so requests can carry an access-key header
// (the browser's EventSource cannot set headers, and a key in the URL would leak into logs).

export interface SseMessage {
  id?: string;
  data: string;
}

/**
 * Splits a text buffer into complete SSE messages. Returns the messages and the unconsumed remainder
 * (an incomplete trailing message). Comment lines (": keep-alive") are dropped.
 */
export function parseSse(buffer: string): { messages: SseMessage[]; rest: string } {
  const normalized = buffer.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  const blocks = normalized.split("\n\n");
  const rest = blocks.pop() ?? "";
  const messages: SseMessage[] = [];
  for (const block of blocks) {
    let id: string | undefined;
    const data: string[] = [];
    for (const line of block.split("\n")) {
      if (line === "" || line.startsWith(":")) continue;
      const colon = line.indexOf(":");
      const field = colon === -1 ? line : line.slice(0, colon);
      let value = colon === -1 ? "" : line.slice(colon + 1);
      if (value.startsWith(" ")) value = value.slice(1);
      if (field === "id") id = value;
      else if (field === "data") data.push(value);
    }
    if (data.length > 0) messages.push({ id, data: data.join("\n") });
  }
  return { messages, rest };
}

export class StreamError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

/**
 * Opens an SSE stream and calls `onMessage` for each message until the stream ends or `signal` aborts.
 * Resolves when the server closes the stream; rejects with StreamError on a non-2xx status.
 */
export async function readSse(
  url: string,
  options: { headers?: Record<string, string>; lastEventId?: string; signal?: AbortSignal },
  onOpen: () => void,
  onMessage: (m: SseMessage) => void,
): Promise<void> {
  const headers: Record<string, string> = { Accept: "text/event-stream", ...options.headers };
  if (options.lastEventId) headers["Last-Event-ID"] = options.lastEventId;
  const res = await fetch(url, { headers, signal: options.signal, cache: "no-store" });
  if (!res.ok || !res.body) throw new StreamError(res.status, res.statusText);
  onOpen();
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const { messages, rest } = parseSse(buffer);
    buffer = rest;
    messages.forEach(onMessage);
  }
}
